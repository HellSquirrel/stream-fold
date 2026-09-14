//! The event model. Every event carries a monotonic [`Index`] once appended.
//!
//! Events are split by origin, because origin decides how each replay mode
//! treats them:
//!
//! | variant   | origin | deterministic replay | re-execution          |
//! |-----------|--------|----------------------|-----------------------|
//! | `Pure`    | input  | folded as recorded   | folded as recorded    |
//! | `Io`      | world  | folded as recorded   | dropped, regenerated  |
//! | `Started` | host   | inert bookkeeping    | dropped, regenerated  |
//!
//! View folds should only ever need `Pure` and `Io`. `Started` exists so
//! that the host's in-flight set is a fold over the log rather than host
//! memory (see [`crate::fold::InFlight`]), and it is written *before* the
//! effect is performed.
//!
//! Text payloads are deliberately absent for M0. Per proposal §3.7 strings
//! stay in the host and cross the boundary as opaque handles.

use crate::effect::Effect;

/// Scope key, e.g. `"post:42"`, `"conn:feed"`, `"io"`.
/// A `String` for now; swap for `SmolStr` when a benchmark asks for it.
pub type Key = String;

/// Position in the log. Assigned by [`crate::Log::append`].
pub type Index = u64;

/// Request id for an in-flight effect. Derived from the log index of the
/// event that caused it, so it is stable across replays.
pub type ReqId = u64;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Event {
    /// A deterministic input: user intent or virtual time. Never depends
    /// on the world, so it is kept verbatim in every replay mode.
    Pure(Pure),
    /// The world's answer to an effect the host performed.
    Io { key: Key, req: ReqId, res: IoResult },
    /// Host bookkeeping: an effect was started. Appended before performing.
    Started {
        key: Key,
        req: ReqId,
        effect: Effect,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Pure {
    /// Something the user did, committed by the host (or an escape hatch).
    Ui { key: Key, ev: UiEvent },
    /// Virtual time. The core never reads a wall clock.
    Tick { ms: u64 },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum UiEvent {
    Click,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum IoResult {
    Done,
    Failed,
    Cancelled,
}

/// Where an event came from. Decides its fate under each replay mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Origin {
    Input,
    World,
    Host,
}

impl Event {
    pub fn origin(&self) -> Origin {
        match self {
            Event::Pure(_) => Origin::Input,
            Event::Io { .. } => Origin::World,
            Event::Started { .. } => Origin::Host,
        }
    }

    pub fn is_pure(&self) -> bool {
        matches!(self, Event::Pure(_))
    }

    pub fn tick(ms: u64) -> Self {
        Event::Pure(Pure::Tick { ms })
    }

    pub fn click(key: impl Into<Key>) -> Self {
        Event::Pure(Pure::Ui {
            key: key.into(),
            ev: UiEvent::Click,
        })
    }
}
