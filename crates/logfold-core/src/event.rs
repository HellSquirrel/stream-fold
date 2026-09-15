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

use std::fmt::{self, Debug};

/// Scope key, e.g. `"post:42"`, `"brunhilda"`, `"conn:feed"`.
///
/// A plain `String` for now: a few bytes per event, and it never crosses
/// the WASM boundary. Swap for an interned id when a measurement asks.
pub type Key = String;

/// Position in the log. Assigned by [`crate::Log::append`].
pub type Index = u64;

/// Request id for an in-flight effect. Derived from the log index of the
/// event that caused it, so it is stable across replays.
pub type ReqId = u64;

/// What a domain can say. Implemented by a marker type per application.
pub trait Domain: 'static {
    /// User intent or commands. Pure: never depends on the world.
    type Input: Clone + Debug + PartialEq + Eq;
    /// Unsolicited world input.
    type Sense: Clone + Debug + PartialEq + Eq;
    /// Effects the host performs on the domain's behalf.
    type Effect: Clone + Debug + PartialEq + Eq + Ord + Action;
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
pub type Never = core::convert::Infallible;

impl Action for Never {
    fn req(&self) -> Option<ReqId> {
        match *self {}
    }
}

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
    /// The effect carries its own request id, if it expects an answer.
    Started { key: Key, effect: D::Effect },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IoResult {
    Done,
    Failed,
    Cancelled,
}

/// Where an event came from. Decides its fate under each replay mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Input,
    World,
    Host,
}

// Written by hand rather than derived: a derive would demand that the
// `Domain` marker itself be `Clone`, `Debug` and so on, when only the
// payload types are.
impl<D: Domain> Clone for Event<D> {
    fn clone(&self) -> Self {
        match self {
            Event::Tick { ms } => Event::Tick { ms: *ms },
            Event::Input { key, input } => Event::Input {
                key: key.clone(),
                input: input.clone(),
            },
            Event::Sense { key, sense } => Event::Sense {
                key: key.clone(),
                sense: sense.clone(),
            },
            Event::Io { key, req, res } => Event::Io {
                key: key.clone(),
                req: *req,
                res: res.clone(),
            },
            Event::Started { key, effect } => Event::Started {
                key: key.clone(),
                effect: effect.clone(),
            },
        }
    }
}

impl<D: Domain> PartialEq for Event<D> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Event::Tick { ms: a }, Event::Tick { ms: b }) => a == b,
            (Event::Input { key: k, input: a }, Event::Input { key: l, input: b }) => {
                k == l && a == b
            }
            (Event::Sense { key: k, sense: a }, Event::Sense { key: l, sense: b }) => {
                k == l && a == b
            }
            (
                Event::Io {
                    key: k,
                    req: r,
                    res: a,
                },
                Event::Io {
                    key: l,
                    req: q,
                    res: b,
                },
            ) => k == l && r == q && a == b,
            (Event::Started { key: k, effect: a }, Event::Started { key: l, effect: b }) => {
                k == l && a == b
            }
            _ => false,
        }
    }
}

impl<D: Domain> Eq for Event<D> {}

impl<D: Domain> Debug for Event<D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Event::Tick { ms } => f.debug_struct("Tick").field("ms", ms).finish(),
            Event::Input { key, input } => f
                .debug_struct("Input")
                .field("key", key)
                .field("input", input)
                .finish(),
            Event::Sense { key, sense } => f
                .debug_struct("Sense")
                .field("key", key)
                .field("sense", sense)
                .finish(),
            Event::Io { key, req, res } => f
                .debug_struct("Io")
                .field("key", key)
                .field("req", req)
                .field("res", res)
                .finish(),
            Event::Started { key, effect } => f
                .debug_struct("Started")
                .field("key", key)
                .field("effect", effect)
                .finish(),
        }
    }
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

    /// Host bookkeeping for an effect, to be appended *before* the host
    /// performs it. There is no separate request id: a result-bearing
    /// effect carries its own ([`Action::req`]), and a fire-and-forget one
    /// has nothing to be answered under.
    pub fn started(key: impl Into<Key>, effect: D::Effect) -> Self {
        Event::Started {
            key: key.into(),
            effect,
        }
    }
}
