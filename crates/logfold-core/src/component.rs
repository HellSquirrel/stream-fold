//! A component: a fold, its projection, its effects, optionally a simulated
//! world, and the inputs a host may dispatch, as one value. This is
//! everything a host needs to run a piece of UI (or a robot's panel) that
//! renders itself from numbers; see `project`.
//!
//! Values, not traits, like folds: build one with the builder and hand it
//! to a host. The host owns the log and the checkpoints; the component
//! owns meaning.

use std::collections::BTreeSet;
use std::rc::Rc;

use crate::event::{Domain, Event};
use crate::fold::Fold;
use crate::log::LogView;
use crate::project::{
    Change, Name, NameId, Projection, Slot, SlotKind, Target, apply, diff, intern,
};
use crate::slots::{Manifest, TargetDecl};
use crate::tracked::{Changes, TrackedVec};

/// What the simulated world does in one frame, given the state and the
/// frame's duration: the senses it reports. The tick itself is the host's.
pub type Simulate<D, X> = Rc<dyn Fn(&X, u64) -> Vec<Event<D>>>;

/// The effects that should be in flight, from state.
pub type Effects<D, X> = Rc<dyn Fn(&X) -> BTreeSet<<D as Domain>::Effect>>;

/// The projection's derivative: given the state before an event, the
/// state after it and the event, the changes that take `project(before)`
/// to `project(after)`. `None` means "no derivative for this event":
/// the host rebuilds the projection instead, which is always correct,
/// so a component gives derivatives only where they pay. Checked by
/// [`derivative_law`].
pub type Delta<D, X> = Rc<dyn Fn(&X, &X, &Event<D>) -> Option<Vec<Change>>>;

/// One family's members and how one member is drawn, behind a trait
/// object so that a component without families links none of this: the
/// algorithms live in `Family<X, M, C>`, which only exists where
/// [`Component::family`] is called.
pub trait FamilyPlan<X> {
    fn name(&self) -> Name;
    fn name_id(&self) -> NameId;
    /// Members addressed by key rather than position.
    fn keyed(&self) -> bool;
    /// Draw every member into the projection being built.
    fn render(&self, x: &X, into: &mut Projection);
    /// Take the members' change log.
    fn take_changes(&self, x: &mut X) -> Changes;
    /// Push what one event changed, from the log and the two states.
    /// `false` when the log says everything changed: rebuild instead.
    fn derive(&self, before: &X, after: &X, log: Changes, out: &mut Vec<Change>) -> bool;
}

type DrawFn<M, C> = Box<dyn Fn(u32, &M, &C, &mut Vec<Change>)>;
type AffectsFn<M, C> = Box<dyn Fn(&C, &M) -> bool>;

struct Family<X, M, C> {
    name: Name,
    name_id: NameId,
    get: fn(&X) -> &TrackedVec<M>,
    get_mut: fn(&mut X) -> &mut TrackedVec<M>,
    /// Members addressed by this key instead of their position; their
    /// position is then the `order` slot the framework writes.
    key: Option<fn(&M) -> u32>,
    draw: DrawFn<M, C>,
    context: Box<dyn Fn(&X) -> C>,
    affects: AffectsFn<M, C>,
}

impl<X, M, C: PartialEq> Family<X, M, C> {
    /// Slots and order for the member at position `p`, all as sets.
    fn draw_member(&self, p: usize, m: &M, ctx: &C, out: &mut Vec<Change>) -> u32 {
        let k = self.key.map_or(p as u32, |key| key(m));
        (self.draw)(k, m, ctx, out);
        if self.key.is_some() {
            out.push(Change::Set(
                Slot::order(Target::Indexed(self.name_id, k)),
                p as f64,
            ));
        }
        k
    }

    /// The derivative for a keyed family: members are matched by key, so
    /// a removal is one member's clears and a shift is order numbers.
    fn derive_keyed(
        &self,
        key: fn(&M) -> u32,
        before: &X,
        after: &X,
        log: Changes,
        out: &mut Vec<Change>,
    ) -> bool {
        if log.all {
            return false;
        }
        let (was, now) = ((self.get)(before), (self.get)(after));
        let (ctx_was, ctx_now) = ((self.context)(before), (self.context)(after));
        let shifted = log.from.is_some();
        // where each key was, needed when positions may have moved: a
        // shift, or a touched member whose key is not what was there
        let moved = shifted
            || log.touched.iter().any(|&i| {
                let i = i as usize;
                now.get(i)
                    .is_some_and(|m| was.get(i).is_none_or(|w| key(w) != key(m)))
            });
        let was_at: FxMap<u32, usize> = if moved {
            was.iter().enumerate().map(|(i, m)| (key(m), i)).collect()
        } else {
            FxMap::default()
        };
        let position_before = |k: u32, i: usize| -> Option<usize> {
            if let Some(m) = was.get(i)
                && key(m) == k
            {
                return Some(i);
            }
            if moved { was_at.get(&k).copied() } else { None }
        };
        let mut marked = vec![false; now.len()];
        for i in log.touched {
            if let Some(m) = marked.get_mut(i as usize) {
                *m = true;
            }
        }
        if let Some(from) = log.from {
            marked[from as usize..].fill(true);
        }
        if ctx_was != ctx_now {
            for (i, m) in now.iter().enumerate() {
                if (self.affects)(&ctx_was, m) || (self.affects)(&ctx_now, m) {
                    marked[i] = true;
                }
            }
        }
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for i in (0..now.len()).filter(|&i| marked[i]) {
            let (m, k) = (&now[i], key(&now[i]));
            a.clear();
            b.clear();
            let p0 = position_before(k, i);
            if let Some(p0) = p0 {
                (self.draw)(k, &was[p0], &ctx_was, &mut a);
            }
            (self.draw)(k, m, &ctx_now, &mut b);
            member_diff(&a, &b, out);
            if p0 != Some(i) {
                out.push(Change::Set(
                    Slot::order(Target::Indexed(self.name_id, k)),
                    i as f64,
                ));
            }
        }
        // keys that are gone: the order first, then whatever was set
        if shifted {
            let now_keys: FxSet<u32> = now.iter().map(key).collect();
            for m in was.iter().filter(|m| !now_keys.contains(&key(m))) {
                let k = key(m);
                out.push(Change::Clear(Slot::order(Target::Indexed(self.name_id, k))));
                a.clear();
                (self.draw)(k, m, &ctx_was, &mut a);
                out.extend(a.iter().filter_map(|c| match c {
                    Change::Set(slot, _) => Some(Change::Clear(*slot)),
                    Change::Clear(_) => None,
                }));
            }
        }
        true
    }
}

impl<X, M, C: PartialEq> FamilyPlan<X> for Family<X, M, C> {
    fn name(&self) -> Name {
        self.name
    }

    fn name_id(&self) -> NameId {
        self.name_id
    }

    fn keyed(&self) -> bool {
        self.key.is_some()
    }

    /// Every member's writes go in already sorted: the first member shows
    /// the order its slots sort in, and each member after it that writes
    /// the same slots in the same sequence is emitted through that
    /// permutation. Members ascend by position (or key), so the whole
    /// batch is sorted and the projection's sort becomes one linear check.
    fn render(&self, x: &X, into: &mut Projection) {
        let (members, ctx) = ((self.get)(x), (self.context)(x));
        let mut buf = Vec::new();
        let mut pattern: Vec<(SlotKind, NameId)> = Vec::new();
        let mut perm: Vec<usize> = Vec::new();
        for (i, m) in members.iter().enumerate() {
            buf.clear();
            self.draw_member(i, m, &ctx, &mut buf);
            let same = buf.len() == pattern.len()
                && buf.iter().zip(&pattern).all(|(c, (k, n))| {
                    let s = c.slot();
                    s.kind == *k && s.name == *n
                });
            if !same {
                pattern = buf.iter().map(|c| (c.slot().kind, c.slot().name)).collect();
                perm = (0..buf.len()).collect();
                // insertion sort of a handful of indices; nothing to link
                for a in 1..perm.len() {
                    let mut b = a;
                    while b > 0 && buf[perm[b]].slot() < buf[perm[b - 1]].slot() {
                        perm.swap(b, b - 1);
                        b -= 1;
                    }
                }
            }
            for &j in &perm {
                match buf[j] {
                    Change::Set(slot, v) => into.put(slot, v),
                    Change::Clear(slot) => into.put_absent(slot),
                }
            }
        }
    }

    fn take_changes(&self, x: &mut X) -> Changes {
        (self.get_mut)(x).take_changes()
    }

    fn derive(&self, before: &X, after: &X, log: Changes, out: &mut Vec<Change>) -> bool {
        if let Some(key) = self.key {
            return self.derive_keyed(key, before, after, log, out);
        }
        if log.all {
            return false;
        }
        let (was, now) = ((self.get)(before), (self.get)(after));
        // a shift over most of the family costs more to redraw member by
        // member than to rebuild and diff; measured at 100,000 rows,
        // removing row 3 was 186 ms derived against 113 rebuilt
        if log
            .from
            .is_some_and(|from| (now.len() as u32 - from) * 2 > now.len() as u32)
        {
            return false;
        }
        let (ctx_was, ctx_now) = ((self.context)(before), (self.context)(after));
        // which members to look at: touched, shifted, or affected by a
        // changed context; marked, so no sorting and no duplicates
        let mut marked = vec![false; now.len()];
        for i in log.touched {
            if let Some(m) = marked.get_mut(i as usize) {
                *m = true;
            }
        }
        if let Some(from) = log.from {
            marked[from as usize..].fill(true);
        }
        if ctx_was != ctx_now {
            for (i, m) in now.iter().enumerate() {
                if (self.affects)(&ctx_was, m) || (self.affects)(&ctx_now, m) {
                    marked[i] = true;
                }
            }
        }
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for i in (0..now.len()).filter(|&i| marked[i]) {
            a.clear();
            b.clear();
            if let Some(m) = was.get(i) {
                (self.draw)(i as u32, m, &ctx_was, &mut a);
            }
            (self.draw)(i as u32, &now[i], &ctx_now, &mut b);
            member_diff(&a, &b, out);
        }
        // members that vanished: whatever they had set, cleared
        for (i, m) in was.iter().enumerate().skip(now.len()) {
            a.clear();
            (self.draw)(i as u32, m, &ctx_was, &mut a);
            out.extend(a.iter().filter_map(|c| match c {
                Change::Set(slot, _) => Some(Change::Clear(*slot)),
                Change::Clear(_) => None,
            }));
        }
        true
    }
}

/// A hasher for integer keys: one multiply per word, and none of the
/// standard library's SipHash in the bundle.
#[derive(Default)]
struct Fx(u64);

impl std::hash::Hasher for Fx {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(*b)).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }
    fn write_u32(&mut self, i: u32) {
        self.0 = (self.0.rotate_left(5) ^ u64::from(i)).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

type FxMap<K, V> = std::collections::HashMap<K, V, std::hash::BuildHasherDefault<Fx>>;
type FxSet<K> = std::collections::HashSet<K, std::hash::BuildHasherDefault<Fx>>;

/// The changes from one member's slots as they were to as they are:
/// sets that differ, clears for sets that are gone. Small lists, linear.
fn member_diff(was: &[Change], now: &[Change], out: &mut Vec<Change>) {
    let value = |list: &[Change], slot: Slot| {
        list.iter().find_map(|c| match c {
            Change::Set(s, v) if *s == slot => Some(*v),
            _ => None,
        })
    };
    for c in now {
        if let Change::Set(slot, v) = *c
            && value(was, slot) != Some(v)
        {
            out.push(Change::Set(slot, v));
        }
    }
    for c in was {
        if let Change::Set(slot, _) = *c
            && value(now, slot).is_none()
        {
            out.push(Change::Clear(slot));
        }
    }
}

/// What a dispatched input becomes: a fixed value, a value built from the
/// bytes the page sent with it, or one built from the index of the family
/// member it was fired from. The builder owns the decoding, so a component
/// without text inputs links no UTF-8 handling at all.
#[derive(Clone, Debug)]
pub enum InputSpec<I> {
    Unit(I),
    Text(fn(&[u8]) -> I),
    Index(fn(u32) -> I),
}

pub struct Component<D: Domain, X> {
    /// Scope key every dispatched input and started effect is appended under.
    pub key: Name,
    /// State from the log.
    pub fold: Fold<Event<D>, X>,
    /// Numbers on named targets from state.
    pub project: Rc<dyn Fn(&X) -> Projection>,
    /// The projection's derivative, if the component wrote one by hand.
    /// Otherwise one is derived from `families`, when every declared
    /// family has a member projection.
    pub delta: Option<Delta<D, X>>,
    /// Families drawn member by member; see [`Component::family`].
    pub families: Vec<Rc<dyn FamilyPlan<X>>>,
    /// The effects that should be in flight, from state. Level-triggered:
    /// the host diffs this against what it has started.
    pub effects: Effects<D, X>,
    /// A simulated world, if this component brings its own. A host with a
    /// real world (a network, a robot) leaves this `None`.
    pub simulate: Option<Simulate<D, X>>,
    /// What the skeleton may ask for by name (`data-on="click:toggle"`).
    pub inputs: Vec<(Name, InputSpec<D::Input>)>,
    /// The declared contract, if the component has one. A host interns its
    /// names first, so ids match what the generated page expects.
    pub manifest: Option<&'static Manifest>,
}

impl<D: Domain, X> Clone for Component<D, X>
where
    X: Clone,
{
    fn clone(&self) -> Self {
        Self {
            key: self.key,
            fold: self.fold.clone(),
            project: self.project.clone(),
            delta: self.delta.clone(),
            families: self.families.clone(),
            effects: self.effects.clone(),
            simulate: self.simulate.clone(),
            inputs: self.inputs.clone(),
            manifest: self.manifest,
        }
    }
}

impl<D: Domain, X: Clone + 'static> Component<D, X> {
    /// A component with no projection, no effects, no world and no inputs yet.
    pub fn new(key: Name, fold: Fold<Event<D>, X>) -> Self {
        Self {
            key,
            fold,
            project: Rc::new(|_| Projection::new()),
            delta: None,
            families: Vec::new(),
            effects: Rc::new(|_| BTreeSet::new()),
            simulate: None,
            inputs: Vec::new(),
            manifest: None,
        }
    }

    /// Attach the declared contract. Its names are interned here, in its
    /// order, before anything else in the component can intern one, so
    /// that in a fresh module the ids are the manifest's indices.
    pub fn manifest(mut self, m: &'static Manifest) -> Self {
        for n in m.names() {
            intern(n);
        }
        self.manifest = Some(m);
        self
    }

    /// The projection, finished on the way out so a host only ever reads
    /// settled trees.
    pub fn project(mut self, f: impl Fn(&X) -> Projection + 'static) -> Self {
        self.project = Rc::new(move |x| f(x).finish());
        self
    }

    /// The projection's derivative; see [`Delta`].
    pub fn delta(mut self, f: impl Fn(&X, &X, &Event<D>) -> Option<Vec<Change>> + 'static) -> Self {
        self.delta = Some(Rc::new(f));
        self
    }

    pub fn effects(mut self, f: impl Fn(&X) -> BTreeSet<D::Effect> + 'static) -> Self {
        self.effects = Rc::new(f);
        self
    }

    pub fn simulate(mut self, f: impl Fn(&X, u64) -> Vec<Event<D>> + 'static) -> Self {
        self.simulate = Some(Rc::new(f));
        self
    }

    /// An input that is the same event every time.
    pub fn input(mut self, name: Name, input: D::Input) -> Self {
        self.inputs.push((name, InputSpec::Unit(input)));
        self
    }

    /// An input that carries the text the page sent with it, as UTF-8
    /// bytes `make` decodes however it likes.
    pub fn text_input(mut self, name: Name, make: fn(&[u8]) -> D::Input) -> Self {
        self.inputs.push((name, InputSpec::Text(make)));
        self
    }

    /// An input that carries the index of the family member it was fired
    /// from (`data-fold="item-3"` gives 3). Fired outside any member, it
    /// is dropped.
    pub fn index_input(mut self, name: Name, make: fn(u32) -> D::Input) -> Self {
        self.inputs.push((name, InputSpec::Index(make)));
        self
    }

    /// A family drawn member by member: `get` reaches its members in the
    /// state, `draw` gives one member's slots, `context` is whatever else
    /// a member's slots depend on, and `affects` says whether a member
    /// is drawn differently under a given context. With no context, pass
    /// `|_| ()` and `|_, _| false`.
    #[allow(clippy::too_many_arguments)]
    pub fn family<M: 'static, C: PartialEq + 'static>(
        mut self,
        name: Name,
        get: fn(&X) -> &TrackedVec<M>,
        get_mut: fn(&mut X) -> &mut TrackedVec<M>,
        key: Option<fn(&M) -> u32>,
        draw: impl Fn(u32, &M, &C, &mut Vec<Change>) + 'static,
        context: impl Fn(&X) -> C + 'static,
        affects: impl Fn(&C, &M) -> bool + 'static,
    ) -> Self
    where
        X: 'static,
    {
        self.families.push(Rc::new(Family {
            name,
            name_id: intern(name),
            get,
            get_mut,
            key,
            draw: Box::new(draw),
            context: Box::new(context),
            affects: Box::new(affects),
        }));
        self
    }

    /// The whole page from a state: the root projection plus every
    /// family, member by member. Finished.
    pub fn render(&self, x: &X) -> Projection {
        let mut p = (self.project)(x);
        for f in &self.families {
            f.render(x, &mut p);
        }
        p.finish()
    }

    /// The projection as a fold over the log.
    pub fn projection(&self) -> Fold<Event<D>, X, Projection> {
        let c = self.clone();
        self.fold.clone().map(move |x| c.render(x))
    }

    /// The changes one event made to the page, without rebuilding it:
    /// the hand-written `delta` if there is one, else derived from the
    /// families' change logs and a diff of the root. `None` means the
    /// host should rebuild. Takes the change logs either way.
    pub fn derive(&self, before: &X, after: &mut X, ev: &Event<D>) -> Option<Vec<Change>> {
        let logs: Vec<Changes> = self
            .families
            .iter()
            .map(|f| f.take_changes(after))
            .collect();
        if let Some(delta) = &self.delta {
            return delta(before, after, ev);
        }
        if self.families.is_empty() || !self.every_family_is_drawn() {
            return None;
        }
        let mut out = diff(&(self.project)(before), &(self.project)(after));
        for (f, log) in self.families.iter().zip(logs) {
            if !f.derive(before, after, log, &mut out) {
                return None;
            }
        }
        Some(out)
    }

    fn every_family_is_drawn(&self) -> bool {
        let Some(m) = self.manifest else {
            return true;
        };
        m.slots().all(|s| match s.target {
            TargetDecl::Family { name, .. } => self.families.iter().any(|f| f.name() == name),
            _ => true,
        })
    }

    /// The event a dispatched input becomes, if the id is known. A text
    /// input gets the empty string; an index input gets nothing and yields
    /// `None`. See `text_event` and `index_event`.
    pub fn input_event(&self, id: usize) -> Option<Event<D>> {
        self.event_for(id, None, b"")
    }

    /// The event a dispatched input becomes when the page sent `text`
    /// with it. Inputs that carry no text ignore it.
    pub fn text_event(&self, id: usize, text: &str) -> Option<Event<D>> {
        self.event_for(id, None, text.as_bytes())
    }

    /// The event a dispatched input becomes when fired from family member
    /// `index`. Inputs that carry no index ignore it.
    pub fn index_event(&self, id: usize, index: u32) -> Option<Event<D>> {
        self.event_for(id, Some(index), b"")
    }

    /// What a host received: the input id, the family member it was fired
    /// from if any, and the bytes sent with it. A text input decodes the
    /// bytes itself (the generated ones lossily, as UTF-8); an index input
    /// without an index is dropped.
    pub fn event_for(&self, id: usize, index: Option<u32>, payload: &[u8]) -> Option<Event<D>> {
        let input = match &self.inputs.get(id)?.1 {
            InputSpec::Unit(input) => input.clone(),
            InputSpec::Text(make) => make(payload),
            InputSpec::Index(make) => make(index?),
        };
        Some(Event::input(self.key, input))
    }

    /// Does input `id` carry text?
    pub fn takes_text(&self, id: usize) -> bool {
        matches!(self.inputs.get(id), Some((_, InputSpec::Text(_))))
    }

    pub fn input_names(&self) -> impl Iterator<Item = Name> + '_ {
        self.inputs.iter().map(|(n, _)| *n)
    }
}

/// The derivative law: for every event in `log` where the component gives
/// a derivative, hand-written or derived, applying it to the page before
/// must equal the page after. The incremental path and the rebuild path are then
/// the same function, and a forgotten write fails here, not on a page.
pub fn derivative_law<D: Domain, X: Clone + 'static>(
    c: &Component<D, X>,
    log: LogView<'_, Event<D>>,
) -> Result<(), String> {
    let mut before = c.fold.state(log.prefix(log.base()));
    for (i, ev) in log.iter() {
        let mut after = c.fold.step_one(before.clone(), i, ev);
        if let Some(changes) = c.derive(&before, &mut after, ev) {
            let mut incremental = c.render(&before);
            apply(&mut incremental, &changes);
            let full = c.render(&after);
            if incremental != full {
                let wrong = diff(&incremental, &full);
                return Err(format!(
                    "derivative at {i} ({ev:?}) is off by {} slots, e.g. {:?}",
                    wrong.len(),
                    wrong.iter().take(3).collect::<Vec<_>>()
                ));
            }
        }
        before = after;
    }
    Ok(())
}

/// Declare a whole component: the `slots!` contract plus the domain marker,
/// the inputs as name-to-variant pairs, the state, and the step and project
/// functions by name. Everything a host needs comes out: the marker and its
/// `Domain` impl, an `Input` enum, `Ev`, `INPUTS`, and `component()` with
/// the manifest attached. What you write yourself is the state type, the
/// step, and the projection: the three things that are actually yours.
///
/// ```ignore
/// logfold_core::component! {
///     pub mod ui;
///     domain Like;
///     inputs { toggle => Toggle }
///     root { attr liked: bool; }
///     state View;                  // `state View = expr;` for a non-Default init
///     step = step;
///     project = project;
/// }
/// ```
///
/// An input that carries what a person typed is `add: text => Add`; its
/// variant is `Add(String)`, and a `text title;` slot shows a text by the
/// log index of the input that carried it. An input fired from inside a
/// family member is `toggle: index => Toggle`; its variant is
/// `Toggle(u32)` with the member's index.
///
/// A large family is drawn member by member: `project row from rows =
/// project_row;` names the state field holding the members, a
/// [`TrackedVec`], and a function from `(index, &member)` to that
/// member's slots. Add `, context = f, affects = g` when a member's
/// slots also depend on something outside it: `f(&State) -> C` and
/// `g(&C, &member) -> bool`. From the members' change log the
/// framework derives what each event changed, so the host never
/// rebuilds the page for a small change; `derivative_law` checks it.
///
/// `delta = f;` gives a hand-written derivative instead, `f(before,
/// after, ev) -> Option<Vec<Change>>`, `None` meaning "rebuild". See
/// [`Delta`] and [`derivative_law`].
///
/// Optional clauses, for components with a world: `sense SenseType;`,
/// `effect EffectType;`, `effects = f;`, `simulate = f;`, `key = "scope";`.
#[macro_export]
macro_rules! component {
    (
        $vis:vis mod $ui:ident;
        domain $dom:ident;
        $( key = $key:literal; )?
        $( sense $sense:ty; )?
        $( effect $effect:ty; )?
        inputs { $( $iname:ident $( : $ikind:ident )? => $ivar:ident ),* $(,)? }
        $( root { $($root:tt)* } )?
        $( family $fam:ident $( ($count:expr) )? $( $fkeyed:ident )? { $($famslots:tt)* } )*
        $( consts { $($cname:ident : $cval:expr),* $(,)? } )?
        state $state:ty $( = $init:expr )?;
        step = $step:expr;
        project = $project:expr;
        $( project $pfam:ident from $pfield:ident = $pdraw:expr $( , key = $pkey:expr )? $( , context = $pctx:expr , affects = $paffects:expr )? ; )*
        $( delta = $delta:expr; )?
        $( effects = $effects:expr; )?
        $( simulate = $simulate:expr; )?
    ) => {
        $crate::slots! {
            $vis mod $ui;
            $( root { $($root)* } )?
            $( family $fam $( ($count) )? $( $fkeyed )? { $($famslots)* } )*
            inputs { $( $iname ),* }
            $( consts { $( $cname : $cval ),* } )?
        }

        $crate::component!(@variants [] $( $iname $( : $ikind )? => $ivar ),*);

        /// The domain marker.
        pub struct $dom;

        impl $crate::Domain for $dom {
            type Input = Input;
            type Sense = $crate::component!(@ty $crate::Never $(; $sense)?);
            type Effect = $crate::component!(@ty $crate::Never $(; $effect)?);
            fn text(input: &Input) -> Option<&str> {
                $crate::component!(@text input [] $( $iname $( : $ikind )? => $ivar ),*)
            }
        }

        pub type Ev = $crate::Event<$dom>;

        /// Input names in dispatch order, paired with what each becomes.
        pub const INPUTS: &[(&str, $crate::component::InputSpec<Input>)] =
            &[ $( (stringify!($iname), $crate::component!(@spec $ivar $($ikind)?)) ),* ];

        pub fn component() -> $crate::Component<$dom, $state> {
            let key: &'static str = $crate::component!(@key stringify!($dom) $(; $key)?);
            let init: $state = $crate::component!(@init $($init)?);
            #[allow(unused_mut)]
            let mut c = $crate::Component::new(key, $crate::Fold::new(init, $step))
                .manifest(&$ui::MANIFEST)
                .project($project);
            $( c = $crate::component!(@family c, $state, $pfam, $pfield, $pdraw; [$($pkey)?]; [$($pctx, $paffects)?]); )*
            $( c = c.delta($delta); )?
            $( c = c.effects($effects); )?
            $( c = c.simulate($simulate); )?
            for (name, spec) in INPUTS {
                c = match spec {
                    $crate::component::InputSpec::Unit(input) => c.input(name, input.clone()),
                    $crate::component::InputSpec::Text(make) => c.text_input(name, *make),
                    $crate::component::InputSpec::Index(make) => c.index_input(name, *make),
                };
            }
            c
        }
    };

    // ---- the Input enum: a unit variant, or `Variant(String)` for `name: text` ----
    (@variants [$($acc:tt)*] $iname:ident : text => $ivar:ident $(, $($rest:tt)*)?) => {
        $crate::component!(@variants [$($acc)* $ivar(String),] $($($rest)*)?);
    };
    (@variants [$($acc:tt)*] $iname:ident : index => $ivar:ident $(, $($rest:tt)*)?) => {
        $crate::component!(@variants [$($acc)* $ivar(u32),] $($($rest)*)?);
    };
    (@variants [$($acc:tt)*] $iname:ident => $ivar:ident $(, $($rest:tt)*)?) => {
        $crate::component!(@variants [$($acc)* $ivar,] $($($rest)*)?);
    };
    (@variants [$($acc:tt)*]) => {
        /// The inputs the skeleton can name, one variant per `data-on` name.
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum Input {
            $($acc)*
        }
    };
    // ---- Domain::text: one arm per text input ----
    (@text $input:ident [$($acc:tt)*] $iname:ident : text => $ivar:ident $(, $($rest:tt)*)?) => {
        $crate::component!(@text $input [$($acc)* Input::$ivar(s) => Some(s.as_str()),] $($($rest)*)?)
    };
    (@text $input:ident [$($acc:tt)*] $iname:ident : index => $ivar:ident $(, $($rest:tt)*)?) => {
        $crate::component!(@text $input [$($acc)*] $($($rest)*)?)
    };
    (@text $input:ident [$($acc:tt)*] $iname:ident => $ivar:ident $(, $($rest:tt)*)?) => {
        $crate::component!(@text $input [$($acc)*] $($($rest)*)?)
    };
    (@text $input:ident [$($acc:tt)*]) => {
        #[allow(unreachable_patterns, clippy::match_single_binding)]
        match $input { $($acc)* _ => None }
    };
    (@spec $ivar:ident text) => {
        $crate::component::InputSpec::Text(|bytes| Input::$ivar(String::from_utf8_lossy(bytes).into_owned()))
    };
    (@spec $ivar:ident index) => { $crate::component::InputSpec::Index(Input::$ivar) };
    // ---- a family drawn member by member, keyed or not, with or without a context ----
    (@family $c:ident, $state:ty, $fam:ident, $field:ident, $draw:expr; [$($key:expr)?]; []) => {
        $c.family(
            stringify!($fam),
            |s: &$state| &s.$field,
            |s: &mut $state| &mut s.$field,
            $crate::component!(@member_key $($key)?),
            |i, m, _: &(), out: &mut Vec<$crate::Change>| out.extend($draw(i, m)),
            |_| (),
            |_, _| false,
        )
    };
    (@family $c:ident, $state:ty, $fam:ident, $field:ident, $draw:expr; [$($key:expr)?]; [$ctx:expr, $affects:expr]) => {
        $c.family(
            stringify!($fam),
            |s: &$state| &s.$field,
            |s: &mut $state| &mut s.$field,
            $crate::component!(@member_key $($key)?),
            |i, m, ctx, out: &mut Vec<$crate::Change>| out.extend($draw(i, m, ctx)),
            $ctx,
            $affects,
        )
    };
    (@member_key) => { None };
    (@member_key $key:expr) => { Some($key) };
    (@spec $ivar:ident) => { $crate::component::InputSpec::Unit(Input::$ivar) };
    (@ty $default:ty) => { $default };
    (@ty $default:ty; $given:ty) => { $given };
    (@key $default:expr) => { $default };
    (@key $default:expr; $given:literal) => { $given };
    (@init) => { Default::default() };
    (@init $e:expr) => { $e };
}
