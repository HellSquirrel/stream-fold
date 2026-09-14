//! Append-only log and read-only views, generic over the event type.
//!
//! A plain `Vec` on purpose. Persistent vectors and shards (proposal §4.9)
//! arrive when a measurement says they are needed.

use crate::event::{Domain, Event, Index};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Log<E> {
    events: Vec<E>,
}

impl<E> Default for Log<E> {
    fn default() -> Self {
        Self { events: Vec::new() }
    }
}

impl<E> Log<E> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one event and return its index.
    pub fn append(&mut self, ev: E) -> Index {
        self.events.push(ev);
        (self.events.len() - 1) as Index
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The whole log as a view.
    pub fn view(&self) -> LogView<'_, E> {
        LogView {
            events: &self.events,
            base: 0,
        }
    }

    /// Events with absolute index `< n`. `n` is clamped to the log length.
    pub fn prefix(&self, n: usize) -> LogView<'_, E> {
        self.view().prefix(n)
    }
}

impl<D: Domain> Log<Event<D>> {
    /// Only the pure events, re-indexed from zero. This is the seed for
    /// re-execution: feed it to a live host and the world answers afresh.
    /// Request ids in the result will differ from the original, because
    /// they derive from indices.
    pub fn inputs(&self) -> Log<Event<D>> {
        self.events
            .iter()
            .filter(|e| e.is_pure())
            .cloned()
            .collect()
    }
}

impl<E> FromIterator<E> for Log<E> {
    fn from_iter<I: IntoIterator<Item = E>>(iter: I) -> Self {
        Self {
            events: iter.into_iter().collect(),
        }
    }
}

/// An immutable window onto the log covering absolute indices
/// `[base, base + len)`. Events keep their absolute indices, so a view of
/// the tail folds exactly like the same events inside the whole log.
#[derive(Debug)]
pub struct LogView<'a, E> {
    events: &'a [E],
    base: usize,
}

impl<E> Clone for LogView<'_, E> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<E> Copy for LogView<'_, E> {}

impl<'a, E> LogView<'a, E> {
    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Absolute index of the first event in the view (even if empty).
    pub fn base(&self) -> usize {
        self.base
    }

    /// One past the absolute index of the last event in the view.
    pub fn end(&self) -> usize {
        self.base + self.events.len()
    }

    /// Events with absolute index in `[from, to)`, clamped to this view.
    pub fn slice(&self, from: usize, to: usize) -> LogView<'a, E> {
        let from = from.clamp(self.base, self.end());
        let to = to.clamp(from, self.end());
        LogView {
            events: &self.events[from - self.base..to - self.base],
            base: from,
        }
    }

    /// Events with absolute index `< n`.
    pub fn prefix(&self, n: usize) -> LogView<'a, E> {
        self.slice(self.base, n)
    }

    /// Events with absolute index `>= from`.
    pub fn suffix(&self, from: usize) -> LogView<'a, E> {
        self.slice(from, self.end())
    }

    /// Events with their absolute indices, oldest first.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (Index, &'a E)> + use<'a, E> {
        let base = self.base;
        self.events
            .iter()
            .enumerate()
            .map(move |(i, e)| ((base + i) as Index, e))
    }

    /// The event at absolute index `i`, if it is inside this view.
    pub fn get(&self, i: usize) -> Option<&'a E> {
        i.checked_sub(self.base).and_then(|k| self.events.get(k))
    }

    /// Absolute index of the most recent event, if any.
    pub fn last_index(&self) -> Option<Index> {
        (!self.events.is_empty()).then(|| (self.end() - 1) as Index)
    }

    /// The most recent event, if any.
    pub fn last(&self) -> Option<(Index, &'a E)> {
        self.iter().next_back()
    }
}

impl<D: Domain> LogView<'_, Event<D>> {
    /// Virtual "now": the most recent `Tick`, or 0 before any tick.
    pub fn now(&self) -> u64 {
        crate::fold::now::<D>().run(*self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{IoResult, Never};

    #[derive(Clone, Debug, PartialEq, Eq, Hash)]
    struct T;
    impl Domain for T {
        type Input = ();
        type Sense = ();
        type Effect = Never;
    }
    type Ev = Event<T>;

    #[test]
    fn prefix_clamps_and_indexes_from_zero() {
        let mut log = Log::new();
        assert_eq!(log.append(Ev::tick(1)), 0);
        assert_eq!(log.append(Ev::tick(2)), 1);
        assert_eq!(log.prefix(99).len(), 2);
        assert_eq!(log.prefix(0).now(), 0);
        assert_eq!(log.prefix(1).now(), 1);
        assert_eq!(log.view().now(), 2);
        assert_eq!(log.view().last_index(), Some(1));
        assert_eq!(log.prefix(0).last_index(), None);
    }

    #[test]
    fn slices_keep_absolute_indices() {
        let log: Log<Ev> = (0..5).map(Ev::tick).collect();
        let tail = log.view().suffix(3);
        assert_eq!(tail.base(), 3);
        assert_eq!(tail.end(), 5);
        assert_eq!(tail.iter().map(|(i, _)| i).collect::<Vec<_>>(), vec![3, 4]);
        assert_eq!(tail.last_index(), Some(4));
        assert_eq!(tail.get(3), Some(&Ev::tick(3)));
        assert_eq!(tail.get(2), None);
        assert_eq!(
            tail.prefix(4).iter().map(|(i, _)| i).collect::<Vec<_>>(),
            vec![3]
        );
        assert_eq!(tail.prefix(1).len(), 0);
        assert_eq!(tail.prefix(1).base(), 3);
        assert_eq!(log.view().suffix(99).len(), 0);
        assert_eq!(log.view().suffix(99).base(), 5);
    }

    #[test]
    fn inputs_keeps_only_pure_events() {
        let log: Log<Ev> = [
            Ev::input("k", ()),
            Ev::sense("k", ()),
            Ev::tick(16),
            Ev::io("k", 0, IoResult::Done),
        ]
        .into_iter()
        .collect();
        let inputs = log.inputs();
        assert_eq!(inputs.len(), 2);
        assert!(inputs.view().iter().all(|(_, e)| e.is_pure()));
    }
}
