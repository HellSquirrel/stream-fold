//! The generic browser host for a [`Component`]. One of these per
//! component; the `export_component!` macro wraps it for wasm-bindgen.
//!
//! The DOM is the materialisation of `project(fold(log[..at]))`. The host
//! keeps `at` and the projection at `at`; moving `at`, forwards for an
//! input or backwards for the timeline, is one map diff. Names cross the
//! boundary once, as handles, the first time an id appears in a patch.
//!
//! Effects: after every append the host diffs the component's desired
//! effects against the `in_flight` fold and records a `Started` for each
//! one to start. Performing them is the page's business; today's
//! components only have fire-and-forget effects the fold itself reads.
//!
//! A component with a simulated world gets `frame(dt)`: the world's senses,
//! then a tick, then the effects step. That is one frame of the sim.

use std::collections::BTreeSet;

use logfold_core::{
    Change, Checkpoints, Component, Domain, Event, Fold, Log, Name, Projection, SlotKind, Target,
    diff, diff_effects, in_flight,
};
#[cfg(feature = "bindgen")]
use wasm_bindgen::JsValue;

use crate::due;
#[cfg(feature = "bindgen")]
use crate::label;

const CHECKPOINT_EVERY: usize = 8;

/// The host's checkpointed state: the component's state and what it has started.
type HostState<D, X> = (X, BTreeSet<<D as Domain>::Effect>);

/// Event kinds, as indices into a table of JS string handles.
#[derive(Clone, Copy)]
#[repr(u8)]
enum Kind {
    Input = 0,
    Tick = 1,
    Sense = 2,
    Io = 3,
    Started = 4,
}

impl Kind {
    fn of<D: Domain>(ev: &Event<D>) -> Kind {
        match ev {
            Event::Input { .. } => Kind::Input,
            Event::Tick { .. } => Kind::Tick,
            Event::Sense { .. } => Kind::Sense,
            Event::Io { .. } => Kind::Io,
            Event::Started { .. } => Kind::Started,
        }
    }
}

/// Names crossing the boundary become small integers; the shim fetches
/// each name once and caches the handle.
#[derive(Default)]
struct Names(Vec<Name>);

impl Names {
    fn id(&mut self, name: Name) -> f64 {
        let i = self.0.iter().position(|n| *n == name).unwrap_or_else(|| {
            self.0.push(name);
            self.0.len() - 1
        });
        i as f64
    }

    fn name(&self, id: u32) -> Option<Name> {
        self.0.get(id as usize).copied()
    }
}

pub struct Host<D: Domain, X: Clone + 'static> {
    component: Component<D, X>,
    state: Fold<Event<D>, HostState<D, X>>,
    project: Fold<Event<D>, HostState<D, X>, Projection>,
    log: Log<Event<D>>,
    checkpoints: Checkpoints<HostState<D, X>>,
    /// The log index the DOM currently materialises.
    at: usize,
    /// The projection at `at`, which is what the DOM holds.
    current: Projection,
    /// Virtual time, advanced by `frame`.
    clock: u64,
    names: Names,
    #[cfg(feature = "bindgen")]
    labels: [JsValue; 5],
}

impl<D: Domain, X: Clone + 'static> Host<D, X> {
    pub fn new(component: Component<D, X>) -> Self {
        let state = component.fold.clone().zip(in_flight::<D>());
        let project = {
            let p = component.project.clone();
            state.clone().map(move |(x, _)| p(x))
        };
        // A declared contract fixes the ids: the generated page resolves
        // them from the manifest without asking.
        let mut names = Names::default();
        if let Some(m) = component.manifest {
            let declared: Vec<Name> = component.input_names().collect();
            assert!(
                declared == m.inputs,
                "the component's inputs must match its manifest, in order"
            );
            for n in m.names() {
                names.id(n);
            }
        }
        Self {
            component,
            state,
            project,
            log: Log::new(),
            checkpoints: Checkpoints::new(),
            at: 0,
            current: Projection::new(),
            clock: 0,
            names,
            #[cfg(feature = "bindgen")]
            labels: [
                label("input"),
                label("tick"),
                label("sense"),
                label("io"),
                label("started"),
            ],
        }
    }

    // ---- tables the shim reads once ----

    /// Input names, indexed by the id `dispatch` takes.
    #[cfg(feature = "bindgen")]
    pub fn input_names(&self) -> Vec<JsValue> {
        self.component.input_names().map(label).collect()
    }

    /// The name behind a target or variable id in a patch.
    #[cfg(feature = "bindgen")]
    pub fn name(&self, id: u32) -> JsValue {
        self.names.name(id).map_or(JsValue::UNDEFINED, label)
    }

    /// The name behind an id, as bytes in linear memory. For raw hosts.
    pub fn name_str(&self, id: u32) -> Option<Name> {
        self.names.name(id)
    }

    /// The component's inputs, by name, in dispatch order.
    pub fn inputs(&self) -> impl Iterator<Item = Name> + '_ {
        self.component.input_names()
    }

    /// Event kind at `i` as a small integer: 0 input, 1 tick, 2 sense,
    /// 3 io, 4 started; -1 past the end. For raw hosts.
    pub fn kind_code(&self, i: u32) -> i32 {
        self.log
            .view()
            .get(i as usize)
            .map_or(-1, |e| Kind::of(e) as i32)
    }

    // ---- inputs: each one appends, lets the host act, and returns the patch to the head ----

    /// The user acted. `input` indexes `input_names`; `index` is the family
    /// member it was fired from, or -1; `payload` is the UTF-8 text the
    /// page sent with it, empty for a click. Inputs ignore what they do
    /// not carry; an index input fired from nowhere, and an unknown id,
    /// append nothing. The patch to the head is returned either way.
    pub fn dispatch(&mut self, input: u32, index: i32, payload: &[u8]) -> Vec<f64> {
        let index = u32::try_from(index).ok();
        if let Some(ev) = self.component.event_for(input as usize, index, payload) {
            self.append(ev);
            self.act();
        }
        self.render_at(self.log.len() as u32)
    }

    /// Virtual time advanced by the page's clock. `ms` is absolute.
    pub fn tick(&mut self, ms: f64) -> Vec<f64> {
        self.clock = ms as u64;
        self.append(Event::tick(self.clock));
        self.act();
        self.render_at(self.log.len() as u32)
    }

    /// One frame of the simulated world, `dt` milliseconds long: the
    /// world's senses, a tick, the effects step. A component without a
    /// world just ticks.
    pub fn frame(&mut self, dt: f64) -> Vec<f64> {
        if let Some(simulate) = self.component.simulate.clone() {
            let (x, _) = self.now();
            for ev in simulate(&x, dt as u64) {
                self.append(ev);
            }
        }
        self.clock += dt as u64;
        self.append(Event::tick(self.clock));
        self.act();
        self.render_at(self.log.len() as u32)
    }

    // ---- output: move the DOM to the projection at index n ----

    /// The writes that take the DOM from its current index to `n`, as
    /// `[target, index, kind, name, value]` quintets: `index` is the member
    /// of a family target or -1; `kind` 0 is a custom property, 1 an
    /// attribute; `NaN` means "clear".
    ///
    /// The host assumes every patch it returns is applied: that is what
    /// makes the next diff correct. A shim that drops one desynchronises
    /// the DOM from `at` until the next full `render_at`.
    pub fn render_at(&mut self, n: u32) -> Vec<f64> {
        let n = (n as usize).min(self.log.len());
        let target = self
            .checkpoints
            .output_at(&self.project, self.log.view(), n);
        let changes = diff(&self.current, &target);
        self.current = target;
        self.at = n;
        self.encode(&changes)
    }

    /// The log index the DOM materialises right now.
    pub fn at(&self) -> u32 {
        self.at as u32
    }

    // ---- the timeline, for devtools ----

    pub fn len(&self) -> u32 {
        self.log.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.log.is_empty()
    }

    pub fn checkpoint_for(&self, n: u32) -> i32 {
        self.checkpoints
            .nearest(n as usize)
            .map_or(-1, |(u, _)| u as i32)
    }

    pub fn checkpoint_count(&self) -> u32 {
        self.checkpoints.len() as u32
    }

    /// The label of event `i` as a JS string handle, or `undefined`.
    #[cfg(feature = "bindgen")]
    pub fn kind(&self, i: u32) -> JsValue {
        self.log
            .view()
            .get(i as usize)
            .map_or(JsValue::UNDEFINED, |e| {
                self.labels[Kind::of(e) as usize].clone()
            })
    }

    /// The text input event `i` carried, if it is one and it did. A `text`
    /// slot's number is such an `i`.
    pub fn text_at(&self, i: u32) -> Option<&str> {
        match self.log.view().get(i as usize) {
            Some(Event::Input { input, .. }) => D::text(input),
            _ => None,
        }
    }

    /// `text_at` as a JS string handle, or `undefined`.
    #[cfg(feature = "bindgen")]
    pub fn text_js(&self, i: u32) -> JsValue {
        self.text_at(i)
            .map_or(JsValue::UNDEFINED, JsValue::from_str)
    }

    /// The tick time of event `i`, or NaN if it is not a tick.
    pub fn tick_ms(&self, i: u32) -> f64 {
        match self.log.view().get(i as usize) {
            Some(Event::Tick { ms }) => *ms as f64,
            _ => f64::NAN,
        }
    }

    // ---- internals ----

    /// The state at the head, resumed from the nearest checkpoint.
    #[inline(never)]
    fn now(&self) -> HostState<D, X> {
        self.checkpoints
            .output_at(&self.state, self.log.view(), self.log.len())
    }

    /// Record every effect the component wants that is not in flight.
    fn act(&mut self) {
        let (x, started) = self.now();
        let d = diff_effects(&(self.component.effects)(&x), &started);
        for fx in d.start {
            self.append(Event::started(self.component.key, fx));
        }
    }

    #[inline(never)]
    fn append(&mut self, ev: Event<D>) {
        self.log.append(ev);
        let n = self.log.len();
        if due(&self.checkpoints, n, CHECKPOINT_EVERY) {
            self.checkpoints.take(&self.state, self.log.view(), n);
        }
    }

    /// Encode changes as `[target, index, kind, name, value]`; `NaN` clears.
    /// Kind 2 is text: the value is the log index of the input event whose
    /// text to show, which the shim fetches with `text`.
    fn encode(&mut self, changes: &[Change]) -> Vec<f64> {
        let mut out = Vec::with_capacity(changes.len() * 5);
        for c in changes {
            let slot = c.slot();
            let (target, index) = match slot.target {
                Target::Root => ("root", -1.0),
                Target::Named(n) => (n, -1.0),
                Target::Indexed(n, i) => (n, f64::from(i)),
            };
            out.push(self.names.id(target));
            out.push(index);
            out.push(match slot.kind {
                SlotKind::Var => 0.0,
                SlotKind::Attr => 1.0,
                SlotKind::Text => 2.0,
            });
            out.push(self.names.id(slot.name));
            out.push(match c {
                Change::Set(_, v) => *v,
                Change::Clear(_) => f64::NAN,
            });
        }
        out
    }
}

/// Export a component as a wasm-bindgen class with the production boundary
/// API: inputs, ticks, frames, rendering. One line per app crate; the page
/// imports the class by name. Add `export_devtools!` for the timeline.
#[macro_export]
macro_rules! export_component {
    ($name:ident, $domain:ty, $state:ty, $component:expr) => {
        #[$crate::wasm_bindgen::prelude::wasm_bindgen]
        pub struct $name($crate::Host<$domain, $state>);

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        #[$crate::wasm_bindgen::prelude::wasm_bindgen]
        impl $name {
            #[wasm_bindgen(constructor)]
            pub fn new() -> Self {
                Self($crate::Host::new($component))
            }
            pub fn input_names(&self) -> Vec<$crate::wasm_bindgen::JsValue> {
                self.0.input_names()
            }
            pub fn name(&self, id: u32) -> $crate::wasm_bindgen::JsValue {
                self.0.name(id)
            }
            pub fn dispatch(&mut self, input: u32, index: i32, payload: &[u8]) -> Vec<f64> {
                self.0.dispatch(input, index, payload)
            }
            pub fn text(&self, i: u32) -> $crate::wasm_bindgen::JsValue {
                self.0.text_js(i)
            }
            pub fn tick(&mut self, ms: f64) -> Vec<f64> {
                self.0.tick(ms)
            }
            pub fn frame(&mut self, dt: f64) -> Vec<f64> {
                self.0.frame(dt)
            }
            pub fn render_at(&mut self, n: u32) -> Vec<f64> {
                self.0.render_at(n)
            }
            pub fn at(&self) -> u32 {
                self.0.at()
            }
            pub fn len(&self) -> u32 {
                self.0.len()
            }
            pub fn is_empty(&self) -> bool {
                self.0.is_empty()
            }
        }
    };
}

/// The devtools surface for an exported class: what `www/timeline.mjs`
/// needs and a production page does not. A second line in the app crate.
#[macro_export]
macro_rules! export_devtools {
    ($name:ident) => {
        #[$crate::wasm_bindgen::prelude::wasm_bindgen]
        impl $name {
            pub fn checkpoint_for(&self, n: u32) -> i32 {
                self.0.checkpoint_for(n)
            }
            pub fn checkpoint_count(&self) -> u32 {
                self.0.checkpoint_count()
            }
            pub fn kind(&self, i: u32) -> $crate::wasm_bindgen::JsValue {
                self.0.kind(i)
            }
            pub fn tick_ms(&self, i: u32) -> f64 {
                self.0.tick_ms(i)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use logfold_core::{Slot, apply};

    /// Decode a patch back into changes, using the host's own name table.
    pub(super) fn decode<D: Domain, X: Clone + 'static>(
        h: &Host<D, X>,
        patch: &[f64],
    ) -> Vec<Change> {
        patch
            .chunks(5)
            .map(|c| {
                let (tname, name) = (
                    h.names.name(c[0] as u32).unwrap(),
                    h.names.name(c[3] as u32).unwrap(),
                );
                let target = match (tname, c[1]) {
                    ("root", _) => Target::Root,
                    (n, i) if i >= 0.0 => Target::Indexed(n, i as u32),
                    (n, _) => Target::Named(n),
                };
                let slot = match c[2] as u8 {
                    0 => Slot::var(target, name),
                    1 => Slot::attr(target, name),
                    _ => Slot::text(target, name),
                };
                if c[4].is_nan() {
                    Change::Clear(slot)
                } else {
                    Change::Set(slot, c[4])
                }
            })
            .collect()
    }

    pub(super) fn input<D: Domain, X: Clone + 'static>(h: &Host<D, X>, name: &str) -> u32 {
        h.component
            .input_names()
            .position(|n| n == name)
            .expect("known input") as u32
    }

    fn follows<D: Domain, X: Clone + 'static>(mut h: Host<D, X>, inputs: usize) {
        let mut dom = Projection::new();
        for i in 0..40u32 {
            let patch = if i % 5 == 4 {
                h.tick(f64::from(i) * 16.0)
            } else {
                h.dispatch(i % inputs as u32, -1, b"")
            };
            apply(&mut dom, &decode(&h, &patch));
            assert_eq!(h.at(), h.len());
            assert_eq!(dom, h.project.run(h.log.view()), "after event {i}");
        }
        assert!(h.checkpoint_count() > 0);
        for n in [0u32, 40, 7, 8, 23, 1, 39, 16, 0, 40] {
            let patch = h.render_at(n);
            apply(&mut dom, &decode(&h, &patch));
            assert_eq!(h.at(), n);
            assert_eq!(dom, h.project.run(h.log.prefix(n as usize)), "at {n}");
        }
        let patch = h.render_at(3);
        apply(&mut dom, &decode(&h, &patch));
        let patch = h.dispatch(0, -1, b"");
        apply(&mut dom, &decode(&h, &patch));
        assert_eq!(h.at(), h.len(), "an input while scrubbed snaps to the head");
        assert_eq!(dom, h.project.run(h.log.view()));
    }

    #[test]
    fn the_dom_follows_at_for_every_component() {
        follows(Host::new(like_local::component()), 1);
        follows(Host::new(counter::component()), 3);
    }

    #[test]
    fn unchanged_state_costs_nothing() {
        let mut h = Host::new(like_local::component());
        let _ = h.dispatch(0, -1, b"");
        assert!(h.tick(16.0).is_empty(), "a tick changes no variable");
        assert!(h.render_at(h.len()).is_empty());
        assert!(
            h.dispatch(99, -1, b"").is_empty(),
            "an unknown input appends nothing"
        );
    }
}

/// A component whose inputs carry text and whose root shows one; shared
/// by the host and raw tests.
#[cfg(test)]
pub(crate) mod tests_support {
    pub fn notes() -> logfold_core::Component<notes::Notes, notes::View> {
        notes::component()
    }

    pub mod notes {
        use logfold_core::{Event, Projection};

        logfold_core::component! {
            pub mod ui;
            domain Notes;
            inputs { say: text => Say, clear => Clear, pick: index => Pick }
            root { text last; var said: int; var picked: int; }
            state View;
            step = step;
            project = project;
        }

        #[derive(Clone, Debug, Default, PartialEq, Eq)]
        pub struct View {
            /// The log index of the last non-empty thing said, if any.
            pub last: Option<u64>,
            pub said: u32,
            /// The last row picked, plus one; 0 for none.
            pub picked: u32,
        }

        pub fn step(v: View, at: u64, ev: &Ev) -> View {
            match ev {
                Event::Input {
                    input: Input::Say(s),
                    ..
                } if !s.is_empty() => View {
                    last: Some(at),
                    said: v.said + 1,
                    ..v
                },
                Event::Input {
                    input: Input::Clear,
                    ..
                } => View { last: None, ..v },
                Event::Input {
                    input: Input::Pick(row),
                    ..
                } => View {
                    picked: row + 1,
                    ..v
                },
                _ => v,
            }
        }

        pub fn project(v: &View) -> Projection {
            let p = Projection::new()
                .set(ui::said.slot(), v.said)
                .set(ui::picked.slot(), v.picked);
            match v.last {
                Some(at) => p.set(ui::last.slot(), at as f64),
                None => p,
            }
        }
    }
}

#[cfg(test)]
mod text_tests {
    use super::tests::{decode, input};
    use super::*;
    use logfold_core::{Slot, apply};

    #[test]
    fn text_crosses_as_an_index_into_the_log() {
        let mut h = Host::new(tests_support::notes());
        let (say, clear) = (input(&h, "say"), input(&h, "clear"));
        assert!(h.component.takes_text(say as usize));
        assert!(!h.component.takes_text(clear as usize));

        let patch = h.dispatch(say, -1, "hello".as_bytes());
        let changes = decode(&h, &patch);
        let text = Slot::text(Target::Root, "last");
        assert!(changes.contains(&Change::Set(text, 0.0)), "{changes:?}");
        assert!(patch.chunks(5).any(|c| c[2] == 2.0), "kind 2 on the wire");
        assert_eq!(h.text_at(0), Some("hello"));

        let _ = h.dispatch(say, -1, b"");
        assert_eq!(
            h.text_at(1),
            Some(""),
            "an empty payload is still the input's text"
        );
        let _ = h.dispatch(clear, -1, b"ignored");
        assert_eq!(h.text_at(2), None, "a unit input carries none");
        let _ = h.tick(16.0);
        assert_eq!(h.text_at(3), None, "a tick carries none");
        assert_eq!(h.text_at(99), None);

        let patch = h.dispatch(say, -1, &[0xff, b'o', b'k']);
        assert_eq!(
            h.text_at(4),
            Some("\u{fffd}ok"),
            "bad UTF-8 is replaced, never dropped"
        );
        assert!(decode(&h, &patch).contains(&Change::Set(text, 4.0)));

        // an index input carries the row it was fired from, or nothing
        let pick = input(&h, "pick");
        let picked = Slot::var(Target::Root, "--picked");
        let patch = h.dispatch(pick, 7, b"");
        assert!(decode(&h, &patch).contains(&Change::Set(picked, 8.0)));
        let len = h.len();
        assert!(
            h.dispatch(pick, -1, b"").is_empty(),
            "fired from no member: dropped"
        );
        assert_eq!(h.len(), len, "and nothing was appended");
        assert!(!h.component.takes_text(pick as usize));

        // scrubbing moves the text slot back to the index it had then
        let patch = h.render_at(1);
        assert!(decode(&h, &patch).contains(&Change::Set(text, 0.0)));
        let patch = h.render_at(3);
        assert!(decode(&h, &patch).contains(&Change::Clear(text)));
        assert_eq!(
            h.text_at(0),
            Some("hello"),
            "the text is in the log, not the DOM"
        );
    }

    /// The studio: her world simulated by the host, everything in one log.
    #[test]
    fn the_studio_cleans_the_room_and_the_dom_follows() {
        let mut h = Host::new(studio::component());
        let mut dom = Projection::new();
        let mut step = |h: &mut Host<studio::App, studio::State>, patch: Vec<f64>| {
            apply(&mut dom, &decode(h, &patch));
            assert_eq!(dom, h.project.run(h.log.view()));
        };
        let (start, run, north) = (input(&h, "start"), input(&h, "run"), input(&h, "north"));
        let p = h.dispatch(run, -1, b"");
        step(&mut h, p);
        let p = h.dispatch(start, -1, b"");
        step(&mut h, p);
        for i in 0..300 {
            let p = h.frame(250.0);
            step(&mut h, p);
            if i == 100 {
                // you walk away from your corner so she can finish
                for _ in 0..6 {
                    let p = h.dispatch(north, -1, b"");
                    step(&mut h, p);
                }
            }
        }
        let (state, started) = h.now();
        assert_eq!(state.attacks, 0, "careful policy: no attacks");
        assert!(
            state.brain.cleaned.len() >= 85,
            "cleaned {}",
            state.brain.cleaned.len()
        );
        assert!(started.is_empty(), "no e-stop was ever started");
        // the e-stop is an effect: dispatching it records a Started and latches
        let estop = input(&h, "estop");
        let p = h.dispatch(estop, -1, b"");
        step(&mut h, p);
        let (state, started) = h.now();
        assert!(state.latched);
        assert_eq!(started.len(), 1);
        // scrubbing the whole session in an awkward order keeps the DOM honest
        for n in [0u32, 500, 17, 999, 3, h.len()] {
            let p = h.render_at(n);
            apply(&mut dom, &decode(&h, &p));
            assert_eq!(
                dom,
                h.project.run(h.log.prefix(n.min(h.len()) as usize)),
                "at {n}"
            );
        }
    }
}
