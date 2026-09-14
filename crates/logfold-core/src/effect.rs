//! Effects are values. The core never performs them.
//!
//! Design decisions for M0:
//!
//! - Effects are a *diffed projection*, not an emitted stream. A projection
//!   computes the set of effects that *should* be in flight given the log;
//!   the host diffs that against the [`crate::fold::InFlight`] fold (see
//!   [`diff_effects`]). Nothing is ever "emitted" as a stateful act, so
//!   re-running a projection can never double-fire.
//! - The in-flight set is itself a fold over `Started` and `Io` events, so
//!   the host keeps no memory of its own and the durable outbox of
//!   proposal §4.2 is just the log.
//! - An effect declares whether it expects a result. One that does not
//!   (an analytics ping) is resolved the moment it is started. That is
//!   what makes edge-triggered, fire-and-forget actions expressible in a
//!   level-triggered desired-set model.
//!
//! Not modelled here: *outputs* such as the rendered DOM or a motor
//! setpoint. Those are idempotent functions of state that the host diffs
//! against the world directly and never logs.

use std::collections::BTreeSet;

use crate::event::{Index, Key, ReqId};

/// Idempotency key: `(scope, log index of the causing event)`.
/// Stable across replays, unique per cause.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IdemKey {
    pub scope: Key,
    pub index: Index,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Effect {
    /// Send a request. Payload is domain-specific and lives in `intent`
    /// until text handles exist; for the like button it is "liked = true/false".
    Post {
        req: ReqId,
        key: Key,
        intent: bool,
        idem: IdemKey,
    },
    /// Ask the host to fire a `Tick` (or a domain event) no earlier than `until_ms`.
    /// Carries only a request id: the host stays dumb and answers with an
    /// ordinary `Io` event rather than re-injecting an embedded event.
    Delay { req: ReqId, until_ms: u64 },
    /// Fire-and-forget: the canonical result-less action. Resolved on start.
    Ping { key: Key, idem: IdemKey },
}

impl Effect {
    /// The request id the world will answer with, if this effect expects an answer.
    pub fn req(&self) -> Option<ReqId> {
        match self {
            Effect::Post { req, .. } | Effect::Delay { req, .. } => Some(*req),
            Effect::Ping { .. } => None,
        }
    }

    /// Does the world answer this effect with an `Io` event?
    pub fn expects_result(&self) -> bool {
        self.req().is_some()
    }
}

/// What the host must do to move from `in_flight` to `desired`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EffectDiff {
    pub start: BTreeSet<Effect>,
    pub cancel: BTreeSet<Effect>,
}

/// Diff desired effects against those already in flight.
pub fn diff_effects(desired: &BTreeSet<Effect>, in_flight: &BTreeSet<Effect>) -> EffectDiff {
    EffectDiff {
        start: desired.difference(in_flight).cloned().collect(),
        cancel: in_flight.difference(desired).cloned().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(req: ReqId) -> Effect {
        Effect::Post {
            req,
            key: "post:1".into(),
            intent: true,
            idem: IdemKey {
                scope: "post:1".into(),
                index: req,
            },
        }
    }

    #[test]
    fn diff_is_set_difference_both_ways() {
        let desired: BTreeSet<_> = [post(1), post(2)].into();
        let in_flight: BTreeSet<_> = [post(2), post(3)].into();
        let d = diff_effects(&desired, &in_flight);
        assert_eq!(d.start, [post(1)].into());
        assert_eq!(d.cancel, [post(3)].into());
    }

    #[test]
    fn diff_of_equal_sets_is_empty() {
        let s: BTreeSet<_> = [post(1)].into();
        assert_eq!(diff_effects(&s, &s), EffectDiff::default());
    }
}
