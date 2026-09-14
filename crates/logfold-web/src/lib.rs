//! Browser host for `like-local`.
//!
//! The core owns the log and the folds. The JavaScript shim does three
//! things and nothing else: append an event when the user acts, read the
//! view and write it into the DOM, and ask for the view at an earlier
//! index when the user scrubs history.
//!
//! The DOM is an *output*, not an effect: an idempotent function of the
//! view that the shim diffs against what is on screen. Nothing about
//! rendering is logged.
//!
//! Text stays in the host (proposal §3.7). No `String` exists in this
//! crate. Labels are JS strings created once at init and handed back as
//! externref handles; numbers cross as numbers and JS formats them.
//!
//! Checkpoint policy lives here, in the host: one saved state every
//! `CHECKPOINT_EVERY` events. The core never decides when.

use like_local::{View, click, like};
use logfold_core::{Checkpoints, Event, Fold, Log, Pure, UiEvent};
use wasm_bindgen::prelude::*;

const CHECKPOINT_EVERY: usize = 8;

/// Event kinds, as indices into a table of JS string handles.
#[derive(Clone, Copy)]
#[repr(u8)]
enum Kind {
    Click = 0,
    Tick = 1,
    Io = 2,
    Started = 3,
}

impl Kind {
    fn of(ev: &Event) -> Kind {
        match ev {
            Event::Pure(Pure::Ui {
                ev: UiEvent::Click, ..
            }) => Kind::Click,
            Event::Pure(Pure::Tick { .. }) => Kind::Tick,
            Event::Io { .. } => Kind::Io,
            Event::Started { .. } => Kind::Started,
        }
    }
}

/// A JS string handle. Outside wasm there is no JS, so native tests get
/// `undefined`; nothing in the core ever looks inside a label.
fn label(s: &'static str) -> JsValue {
    #[cfg(target_arch = "wasm32")]
    {
        JsValue::from_str(s)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = s;
        JsValue::UNDEFINED
    }
}

#[wasm_bindgen]
pub struct LikeApp {
    log: Log,
    like: Fold<'static, View>,
    checkpoints: Checkpoints<View>,
    /// JS strings, created once. Indexed by `Kind`. Handles, not bytes.
    labels: [JsValue; 4],
}

impl Default for LikeApp {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl LikeApp {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let labels = [label("click"), label("tick"), label("io"), label("started")];
        Self {
            log: Log::new(),
            like: like(),
            checkpoints: Checkpoints::new(),
            labels,
        }
    }

    /// The user pressed the button. Returns the new event's index.
    pub fn click(&mut self) -> u32 {
        self.append(click())
    }

    /// Virtual time advanced. `ms` is whatever clock the host chooses.
    pub fn tick(&mut self, ms: f64) -> u32 {
        self.append(Event::tick(ms as u64))
    }

    /// Number of events in the log.
    pub fn len(&self) -> u32 {
        self.log.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.log.is_empty()
    }

    /// The current view: `fold(log)`.
    pub fn liked(&self) -> bool {
        self.liked_at(self.len())
    }

    /// The view after the first `n` events, resumed from the nearest
    /// checkpoint. This is the scrubber.
    pub fn liked_at(&self, n: u32) -> bool {
        self.checkpoints
            .output_at(&self.like, self.log.view(), n as usize)
            .liked
    }

    /// Index of the checkpoint the scrubber would resume from for `n`,
    /// or -1 if it would fold from zero. For the demo's provenance line.
    pub fn checkpoint_for(&self, n: u32) -> i32 {
        self.checkpoints
            .nearest(n as usize)
            .map_or(-1, |(u, _)| u as i32)
    }

    pub fn checkpoint_count(&self) -> u32 {
        self.checkpoints.len() as u32
    }

    /// The label of event `i` as a JS string handle, or `undefined`.
    /// No bytes are copied: the handle was made once in `new`.
    pub fn kind(&self, i: u32) -> JsValue {
        self.event(i).map_or(JsValue::UNDEFINED, |e| {
            self.labels[Kind::of(e) as usize].clone()
        })
    }

    /// The tick time of event `i`, or NaN if it is not a tick.
    pub fn tick_ms(&self, i: u32) -> f64 {
        match self.event(i) {
            Some(Event::Pure(Pure::Tick { ms })) => *ms as f64,
            _ => f64::NAN,
        }
    }

    fn event(&self, i: u32) -> Option<&Event> {
        self.log.view().iter().nth(i as usize).map(|(_, e)| e)
    }

    fn append(&mut self, ev: Event) -> u32 {
        let i = self.log.append(ev);
        let n = self.log.len();
        if n.is_multiple_of(CHECKPOINT_EVERY) {
            self.checkpoints.take(&self.like, self.log.view(), n);
        }
        i as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubbing_agrees_with_folding_from_zero() {
        let mut app = LikeApp::new();
        for i in 0..40 {
            if i % 3 == 0 {
                app.tick(i as f64 * 16.0);
            } else {
                app.click();
            }
        }
        assert_eq!(app.checkpoint_count(), 5);
        for n in 0..=app.len() {
            let from_zero = app.like.run(app.log.prefix(n as usize)).liked;
            assert_eq!(app.liked_at(n), from_zero, "n = {n}");
        }
        assert_eq!(app.checkpoint_for(7), -1);
        assert_eq!(app.checkpoint_for(8), 8);
        assert_eq!(app.checkpoint_for(23), 16);
        assert!(app.tick_ms(0) == 0.0 && app.tick_ms(1).is_nan());
    }
}
