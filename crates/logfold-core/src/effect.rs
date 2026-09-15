//! Effects are values. The core never performs them.
//!
//! Design decisions:
//!
//! - Effects are a *diffed projection*, not an emitted stream. A projection
//!   computes the set of effects that *should* be in flight given the log;
//!   the host diffs that against the [`crate::fold::in_flight`] fold (see
//!   [`diff_effects`]). Nothing is ever "emitted" as a stateful act, so
//!   re-running a projection can never double-fire.
//! - The in-flight set is itself a fold over `Started` and `Io` events, so
//!   the host keeps no memory of its own and the durable outbox of
//!   proposal §4.2 is just the log.
//! - An effect declares whether it expects a result ([`Action`]). One that
//!   does not (an analytics ping, an e-stop) is resolved the moment it is
//!   started, and its `Started` event is the permanent record that it
//!   happened. That is what makes edge-triggered, fire-and-forget effects
//!   expressible in a level-triggered desired-set model.
//!
//! Not modelled here: *outputs* such as the rendered DOM or a motor
//! setpoint. Those are idempotent functions of state that the host diffs
//! against the world directly and never logs.

use std::collections::BTreeSet;

use crate::event::{Action, Index, Key};

/// Idempotency key: `(scope, log index of the causing event)`.
/// Stable across replays, unique per cause.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct IdemKey {
    pub scope: Key,
    pub index: Index,
}

impl IdemKey {
    pub fn new(scope: impl Into<Key>, index: Index) -> Self {
        Self {
            scope: scope.into(),
            index,
        }
    }
}

/// What the host must do to move from `in_flight` to `desired`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectDiff<E> {
    pub start: BTreeSet<E>,
    /// Effects to cancel. Only effects that expect a result can be
    /// cancelled; a fire-and-forget effect that is no longer desired is
    /// simply over.
    pub cancel: BTreeSet<E>,
}

impl<E> Default for EffectDiff<E> {
    fn default() -> Self {
        Self {
            start: BTreeSet::new(),
            cancel: BTreeSet::new(),
        }
    }
}

/// Diff desired effects against those already started.
pub fn diff_effects<E: Ord + Clone + Action>(
    desired: &BTreeSet<E>,
    in_flight: &BTreeSet<E>,
) -> EffectDiff<E> {
    EffectDiff {
        start: desired.difference(in_flight).cloned().collect(),
        cancel: in_flight
            .difference(desired)
            .filter(|e| e.expects_result())
            .cloned()
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::ReqId;

    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum Fx {
        Post(ReqId),
        Ping(Index),
    }
    impl Action for Fx {
        fn req(&self) -> Option<ReqId> {
            match self {
                Fx::Post(r) => Some(*r),
                Fx::Ping(_) => None,
            }
        }
    }

    #[test]
    fn diff_is_set_difference_both_ways() {
        let desired: BTreeSet<_> = [Fx::Post(1), Fx::Post(2)].into();
        let in_flight: BTreeSet<_> = [Fx::Post(2), Fx::Post(3)].into();
        let d = diff_effects(&desired, &in_flight);
        assert_eq!(d.start, [Fx::Post(1)].into());
        assert_eq!(d.cancel, [Fx::Post(3)].into());
    }

    #[test]
    fn diff_of_equal_sets_is_empty() {
        let s: BTreeSet<_> = [Fx::Post(1)].into();
        assert_eq!(diff_effects(&s, &s), EffectDiff::default());
    }

    #[test]
    fn fire_and_forget_is_never_cancelled_and_never_restarted() {
        let desired: BTreeSet<Fx> = BTreeSet::new();
        let started: BTreeSet<_> = [Fx::Ping(7)].into();
        let d = diff_effects(&desired, &started);
        assert_eq!(d, EffectDiff::default());
        let d = diff_effects(&started, &started);
        assert_eq!(d, EffectDiff::default(), "already started: nothing to do");
    }
}
