//! The event model. Every event carries a monotonic [`Index`] once appended.
//!
//! Core knows about time, requests, and host bookkeeping. Everything
//! domain-specific — what a user can do, what the world can report, what
//! an effect is — comes from a [`Domain`] implementation, so the same
//! runtime serves a like button and a robot without either leaking into
//! the other.
//!
//! Events are split by origin, because origin decides how each replay mode
//! treats them:
//!
//! | variant   | origin | deterministic replay | re-execution          |
//! |-----------|--------|----------------------|-----------------------|
//! | `Tick`    | input  | folded as recorded   | folded as recorded    |
//! | `Input`   | input  | folded as recorded   | folded as recorded    |
//! | `Sense`   | world  | folded as recorded   | dropped, regenerated  |
//! | `Io`      | world  | folded as recorded   | dropped, regenerated  |
//! | `Started` | host   | inert bookkeeping    | dropped, regenerated  |
//!
//! `Sense` is the world speaking unprompted: a bump, a sighting, a message
//! on a socket. `Io` is the world answering a request the host started.
//! `Started` exists so the host's in-flight set is a fold over the log
//! rather than host memory (see [`crate::fold::in_flight`]); it is written
//! *before* the effect is performed.

use std::fmt::Debug;
use std::hash::Hash;

/// Scope key, e.g. `"post:42"`, `"brunhilda"`, `"conn:feed"`.
pub type Key = String;

/// Position in the log. Assigned by [`crate::Log::append`].
pub type Index = u64;

/// Request id for an in-flight effect. Derived from the log index of the
/// event that caused it, so it is stable across replays.
pub type ReqId = u64;

/// What a domain can say. Implemented by a marker type per application.
pub trait Domain: Clone + Debug + PartialEq + Eq + Hash + 'static {
    /// User intent or commands. Pure: never depends on the world.
    type Input: Clone + Debug + PartialEq + Eq + Hash;
    /// Unsolicited world input.
    type Sense: Clone + Debug + PartialEq + Eq + Hash;
    /// Actions the host performs on the domain's behalf.
    type Effect: Clone + Debug + PartialEq + Eq + Hash + Ord + Action;
}

/// An effect's relationship to the world.
pub trait Action {
    /// The request id the world will answer with, if it answers at all.
    fn req(&self) -> Option<ReqId>;

    /// Does the world answer this effect with an `Io` event? If not, the
    /// effect is fire-and-forget and is resolved the moment it is started.
    fn expects_result(&self) -> bool {
        self.req().is_some()
    }
}

/// The empty type, for domains with no senses or no effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Never {}

impl Action for Never {
    fn req(&self) -> Option<ReqId> {
        match *self {}
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Event<D: Domain> {
    /// Virtual time. The core never reads a wall clock.
    Tick { ms: u64 },
    /// A deterministic input: user intent or a command.
    Input { key: Key, input: D::Input },
    /// The world speaking unprompted.
    Sense { key: Key, sense: D::Sense },
    /// The world's answer to an effect the host performed.
    Io { key: Key, req: ReqId, res: IoResult },
    /// Host bookkeeping: an effect was started. Appended before performing.
    Started {
        key: Key,
        req: ReqId,
        effect: D::Effect,
    },
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

impl<D: Domain> Event<D> {
    pub fn origin(&self) -> Origin {
        match self {
            Event::Tick { .. } | Event::Input { .. } => Origin::Input,
            Event::Sense { .. } | Event::Io { .. } => Origin::World,
            Event::Started { .. } => Origin::Host,
        }
    }

    /// Pure events are the deterministic inputs: ticks and inputs.
    pub fn is_pure(&self) -> bool {
        self.origin() == Origin::Input
    }

    /// The scope key, if the event has one. Ticks are global.
    pub fn key(&self) -> Option<&str> {
        match self {
            Event::Tick { .. } => None,
            Event::Input { key, .. }
            | Event::Sense { key, .. }
            | Event::Io { key, .. }
            | Event::Started { key, .. } => Some(key),
        }
    }

    pub fn tick(ms: u64) -> Self {
        Event::Tick { ms }
    }

    pub fn input(key: impl Into<Key>, input: D::Input) -> Self {
        Event::Input {
            key: key.into(),
            input,
        }
    }

    pub fn sense(key: impl Into<Key>, sense: D::Sense) -> Self {
        Event::Sense {
            key: key.into(),
            sense,
        }
    }

    pub fn io(key: impl Into<Key>, req: ReqId, res: IoResult) -> Self {
        Event::Io {
            key: key.into(),
            req,
            res,
        }
    }

    /// Host bookkeeping for an effect. The request id comes from the
    /// effect; fire-and-forget effects get the index-derived id `req`.
    pub fn started(key: impl Into<Key>, req: ReqId, effect: D::Effect) -> Self {
        Event::Started {
            key: key.into(),
            req: effect.req().unwrap_or(req),
            effect,
        }
    }
}
