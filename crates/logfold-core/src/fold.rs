//! Folds as values, in the style of Haskell's `foldl` library.
//!
//! A [`Fold`] is an initial state, a step over one event, and a `done`
//! function that turns the internal state into the output. Folds compose:
//! [`Fold::map`] derives a projection, [`Fold::zip`] runs two folds in one
//! pass, [`Fold::scoped`] restricts a fold to the events it declares
//! (proposal §3.2), and [`Fold::from`] is a checkpoint: the same fold
//! starting from a saved state at a saved index.
//!
//! Folds are generic over the event type `E`; the runtime folds in this
//! module ([`now`], [`in_flight`]) are for [`Event<D>`].
//!
//! The left-fold law makes checkpoints honest: for any fold `f`, any log
//! and any split `k`, `f.from(f.state(prefix k), k).run(log) == f.run(log)`.
//! [`checkpoint_law`] checks it for a given fold and log.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::event::{Action, Domain, Event, Index};
use crate::log::LogView;

type Step<'a, E, X> = Rc<dyn Fn(X, Index, &E) -> X + 'a>;
type Done<'a, X, S> = Rc<dyn Fn(&X) -> S + 'a>;

/// A left fold over events `E` with internal state `X` and output `S`.
///
/// `S` defaults to `X`: a plain fold's output is its state.
pub struct Fold<'a, E, X, S = X> {
    init: X,
    step: Step<'a, E, X>,
    done: Done<'a, X, S>,
    /// Absolute index this fold starts at. Zero unless resumed via [`Fold::from`].
    skip: usize,
}

impl<E, X: Clone, S> Clone for Fold<'_, E, X, S> {
    fn clone(&self) -> Self {
        Fold {
            init: self.init.clone(),
            step: self.step.clone(),
            done: self.done.clone(),
            skip: self.skip,
        }
    }
}

impl<'a, E: 'a, X: Clone + 'a> Fold<'a, E, X> {
    /// A fold whose output is its state.
    pub fn new(init: X, step: impl Fn(X, Index, &E) -> X + 'a) -> Self {
        Fold {
            init,
            step: Rc::new(step),
            done: Rc::new(|x: &X| x.clone()),
            skip: 0,
        }
    }
}

impl<'a, E: 'a, X: Clone + 'a, S: 'a> Fold<'a, E, X, S> {
    /// One step of the underlying state machine.
    pub fn step(&self, x: X, index: Index, ev: &E) -> X {
        (self.step)(x, index, ev)
    }

    /// The output for an internal state.
    pub fn done(&self, x: &X) -> S {
        (self.done)(x)
    }

    /// The initial internal state.
    pub fn init(&self) -> X {
        self.init.clone()
    }

    /// Absolute index this fold starts at.
    pub fn skip(&self) -> usize {
        self.skip
    }

    /// Fold the view (from `skip` onward) and return the final internal state.
    pub fn state(&self, log: LogView<'_, E>) -> X {
        log.suffix(self.skip)
            .iter()
            .fold(self.init.clone(), |x, (i, e)| (self.step)(x, i, e))
    }

    /// Fold the view and return the output.
    pub fn run(&self, log: LogView<'_, E>) -> S {
        (self.done)(&self.state(log))
    }

    /// The output after every prefix, in one pass. Yields `(n, output)`
    /// where `n` is the absolute prefix end: first `(skip, done(init))`,
    /// then one item per event.
    pub fn scan<'s, 'l: 's>(
        &'s self,
        log: LogView<'l, E>,
    ) -> impl Iterator<Item = (usize, S)> + 's {
        let tail = log.suffix(self.skip);
        let first = (tail.base(), (self.done)(&self.init));
        let mut x = Some(self.init.clone());
        std::iter::once(first).chain(tail.iter().map(move |(i, e)| {
            let next = (self.step)(x.take().expect("scan state"), i, e);
            let out = (self.done)(&next);
            x = Some(next);
            (i as usize + 1, out)
        }))
    }

    /// The same fold, starting from `state` as if `upto` events were
    /// already folded. This *is* a checkpoint.
    pub fn from(&self, state: X, upto: usize) -> Fold<'a, E, X, S> {
        Fold {
            init: state,
            step: self.step.clone(),
            done: self.done.clone(),
            skip: upto,
        }
    }

    /// Derive a projection from this fold's output.
    pub fn map<T: 'a>(self, f: impl Fn(&S) -> T + 'a) -> Fold<'a, E, X, T> {
        let done = self.done;
        Fold {
            init: self.init,
            step: self.step,
            done: Rc::new(move |x| f(&done(x))),
            skip: self.skip,
        }
    }

    /// Run two folds over the same events in one pass.
    ///
    /// Both must start at the same index. Zipping folds resumed from
    /// different checkpoints is a construction-time programmer error and
    /// panics in every build: the alternative is a pair whose halves
    /// silently describe different prefixes of the log.
    pub fn zip<Y: Clone + 'a, T: 'a>(
        self,
        other: Fold<'a, E, Y, T>,
    ) -> Fold<'a, E, (X, Y), (S, T)> {
        assert_eq!(
            self.skip, other.skip,
            "zip of folds resumed at different indices"
        );
        let (sx, sy) = (self.step, other.step);
        let (dx, dy) = (self.done, other.done);
        Fold {
            init: (self.init, other.init),
            step: Rc::new(move |(x, y), i, e| (sx(x, i, e), sy(y, i, e))),
            done: Rc::new(move |(x, y)| (dx(x), dy(y))),
            skip: self.skip,
        }
    }

    /// Restrict the fold to the events it declares. Everything else is
    /// stepped over untouched. This is proposal §3.2's scope.
    pub fn scoped(self, keep: impl Fn(&E) -> bool + 'a) -> Fold<'a, E, X, S> {
        let step = self.step;
        Fold {
            init: self.init,
            step: Rc::new(move |x, i, e| if keep(e) { step(x, i, e) } else { x }),
            done: self.done,
            skip: self.skip,
        }
    }
}

/// The left-fold law: resuming from the state at any split equals folding
/// the whole. Returns the first violating split.
pub fn checkpoint_law<E, X: Clone, S: PartialEq + std::fmt::Debug>(
    f: &Fold<'_, E, X, S>,
    log: LogView<'_, E>,
) -> Result<(), String> {
    let full = f.run(log);
    for k in log.base()..=log.end() {
        let resumed = f.from(f.state(log.prefix(k)), k).run(log);
        if resumed != full {
            return Err(format!(
                "split at {k}: resumed {resumed:?} != full {full:?}"
            ));
        }
    }
    Ok(())
}

/// An ordered store of saved states for one fold, keyed by the absolute
/// index they cover. Answers "nearest state at or before index n" in
/// logarithmic time, which is what scrubbing, replay and export need.
///
/// Retention is the caller's policy: insert whenever you like, prune
/// whenever you like. The store never folds on its own.
#[derive(Clone, Debug)]
pub struct Checkpoints<X> {
    by_upto: BTreeMap<usize, X>,
}

impl<X> Default for Checkpoints<X> {
    fn default() -> Self {
        Self {
            by_upto: BTreeMap::new(),
        }
    }
}

impl<X: Clone> Checkpoints<X> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Save `state` as the fold's state after the first `upto` events.
    pub fn insert(&mut self, upto: usize, state: X) {
        self.by_upto.insert(upto, state);
    }

    /// Fold `log` up to `n` and save the result.
    pub fn take<E, S>(&mut self, fold: &Fold<'_, E, X, S>, log: LogView<'_, E>, n: usize) {
        let n = n.min(log.end());
        self.insert(n, fold.state(log.prefix(n)));
    }

    pub fn len(&self) -> usize {
        self.by_upto.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_upto.is_empty()
    }

    /// The saved state with the largest `upto <= n`, if any.
    pub fn nearest(&self, n: usize) -> Option<(usize, &X)> {
        self.by_upto.range(..=n).next_back().map(|(u, x)| (*u, x))
    }

    /// `fold` resumed from the nearest saved state at or before `n`, or
    /// from its own start if none is saved.
    pub fn resume<'a, E: 'a, S: 'a>(&self, fold: &Fold<'a, E, X, S>, n: usize) -> Fold<'a, E, X, S>
    where
        X: 'a,
    {
        match self.nearest(n) {
            Some((upto, x)) if upto >= fold.skip() => fold.from(x.clone(), upto),
            _ => fold.from(fold.init(), fold.skip()),
        }
    }

    /// Output after the first `n` events of `log`, resuming from the
    /// nearest saved state. `n` is clamped to the log length.
    pub fn output_at<'a, E: 'a, S: 'a>(
        &self,
        fold: &Fold<'a, E, X, S>,
        log: LogView<'_, E>,
        n: usize,
    ) -> S
    where
        X: 'a,
    {
        let n = n.min(log.end());
        self.resume(fold, n).run(log.prefix(n))
    }

    /// Drop every saved state with `upto > n`. Use after the log is
    /// rewound or truncated, or when an out-of-order merge invalidates the tail.
    pub fn truncate_after(&mut self, n: usize) {
        self.by_upto.split_off(&(n + 1));
    }
}

/// Virtual time: the most recent `Tick`, 0 before any.
pub fn now<D: Domain>() -> Fold<'static, Event<D>, u64> {
    Fold::new(0, |t, _, ev| match ev {
        Event::Tick { ms } => *ms,
        _ => t,
    })
}

/// Effects the host has started and that are not over.
///
/// `Started` inserts. `Io` removes the effect with the matching request
/// id. A fire-and-forget effect (no request id) is never removed: its
/// `Started` event is the permanent record that it happened, which is what
/// stops the host re-firing it. Because this is a fold, the host's
/// "outbox" survives a restart for free: fold the log, perform whatever
/// is still here and expects a result.
pub fn in_flight<D: Domain>() -> Fold<'static, Event<D>, BTreeSet<D::Effect>> {
    Fold::new(
        BTreeSet::new(),
        |mut s: BTreeSet<D::Effect>, _, ev: &Event<D>| {
            match ev {
                Event::Started { effect, .. } => {
                    s.insert(effect.clone());
                }
                Event::Io { req, .. } => {
                    s.retain(|e| e.req() != Some(*req));
                }
                _ => {}
            }
            s
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{IoResult, Never, ReqId};
    use crate::log::Log;

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    struct T;
    #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
    impl Domain for T {
        type Input = ();
        type Sense = Never;
        type Effect = Fx;
    }
    type Ev = Event<T>;

    /// Count of ticks and the last tick value.
    fn ticks() -> Fold<'static, Ev, (u32, u64)> {
        Fold::new((0, 0), |(n, last), _, ev| match ev {
            Event::Tick { ms } => (n + 1, *ms),
            _ => (n, last),
        })
    }

    #[test]
    fn checkpoint_law_holds_for_ticks() {
        let log: Log<Ev> = (1..=5).map(Ev::tick).collect();
        checkpoint_law(&ticks(), log.view()).unwrap();
    }

    #[test]
    fn scan_yields_every_prefix() {
        let log: Log<Ev> = (1..=3).map(Ev::tick).collect();
        let outs: Vec<_> = ticks().scan(log.view()).collect();
        assert_eq!(
            outs,
            vec![(0, (0, 0)), (1, (1, 1)), (2, (2, 2)), (3, (3, 3))]
        );
    }

    #[test]
    fn from_skips_and_scan_starts_at_skip() {
        let log: Log<Ev> = (1..=4).map(Ev::tick).collect();
        let f = ticks();
        let ck = f.from(f.state(log.prefix(2)), 2);
        assert_eq!(ck.run(log.view()), (4, 4));
        assert_eq!(
            ck.scan(log.view()).map(|(n, _)| n).collect::<Vec<_>>(),
            vec![2, 3, 4]
        );
        assert_eq!(ck.run(log.prefix(1)), (2, 2));
    }

    #[test]
    fn zip_of_aligned_resumed_folds_keeps_the_skip() {
        let log: Log<Ev> = (1..=4).map(Ev::tick).collect();
        // A plain fold and a mapped one, both resumed at the same index.
        let (a, b) = (ticks(), ticks().map(|(n, _)| *n));
        let ra = a.from(a.state(log.prefix(2)), 2);
        let rb = b.from(b.state(log.prefix(2)), 2);
        let z = ra.zip(rb);
        assert_eq!(z.skip(), 2);
        assert_eq!(z.run(log.view()), ((4, 4), 4));
    }

    #[test]
    #[should_panic(expected = "zip of folds resumed at different indices")]
    fn zip_of_misaligned_folds_panics() {
        let log: Log<Ev> = (1..=4).map(Ev::tick).collect();
        let f = ticks();
        let resumed = f.from(f.state(log.prefix(2)), 2);
        let _ = resumed.zip(ticks());
    }

    #[test]
    fn map_zip_and_scoped_compose() {
        let log: Log<Ev> = [Ev::tick(5), Ev::input("k", ()), Ev::tick(7)]
            .into_iter()
            .collect();
        let count = Fold::new(0u32, |n, _, _| n + 1);
        let only_pure = Fold::new(0u32, |n, _, _| n + 1).scoped(Ev::is_pure);
        let inputs_only =
            Fold::new(0u32, |n, _, _| n + 1).scoped(|e| matches!(e, Event::Input { .. }));
        let both = count
            .zip(only_pure)
            .zip(inputs_only)
            .map(|((a, b), c)| (*a, *b, *c));
        assert_eq!(both.run(log.view()), (3, 3, 1));
        assert_eq!(now::<T>().map(|t| t * 2).run(log.view()), 14);
    }

    #[test]
    fn store_resumes_from_nearest_checkpoint() {
        let log: Log<Ev> = (1..=10).map(Ev::tick).collect();
        let f = ticks();
        let mut store = Checkpoints::new();
        store.take(&f, log.view(), 4);
        store.take(&f, log.view(), 8);
        assert_eq!(store.nearest(3), None);
        assert_eq!(store.nearest(4).map(|(u, _)| u), Some(4));
        assert_eq!(store.nearest(7).map(|(u, _)| u), Some(4));
        assert_eq!(store.nearest(99).map(|(u, _)| u), Some(8));
        for n in 0..=12 {
            assert_eq!(
                store.output_at(&f, log.view(), n),
                f.run(log.prefix(n)),
                "n = {n}"
            );
        }
        store.truncate_after(5);
        assert_eq!(store.len(), 1);
        assert_eq!(store.nearest(99).map(|(u, _)| u), Some(4));
    }

    #[test]
    fn in_flight_tracks_started_minus_answered_and_keeps_fire_and_forget() {
        let log: Log<Ev> = [
            Ev::input("k", ()),
            Ev::started("k", 1, Fx::Post(1)),
            Ev::started("k", 2, Fx::Ping(2)),
            Ev::io("k", 1, IoResult::Done),
        ]
        .into_iter()
        .collect();
        let f = in_flight::<T>();
        assert_eq!(f.run(log.prefix(1)), BTreeSet::new());
        assert_eq!(f.run(log.prefix(2)), [Fx::Post(1)].into());
        assert_eq!(f.run(log.prefix(3)), [Fx::Post(1), Fx::Ping(2)].into());
        assert_eq!(
            f.run(log.prefix(4)),
            [Fx::Ping(2)].into(),
            "ping stays as the record"
        );
        checkpoint_law(&f, log.view()).unwrap();
    }
}
