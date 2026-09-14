//! Append-only log and read-only views.
//!
//! A plain `Vec` on purpose. Persistent vectors and shards (proposal §4.9)
//! arrive when a measurement says they are needed.

use crate::event::{Event, Index};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Log {
    events: Vec<Event>,
}

impl Log {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one event and return its index.
    pub fn append(&mut self, ev: Event) -> Index {
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
    pub fn view(&self) -> LogView<'_> {
        LogView {
            events: &self.events,
            base: 0,
        }
    }

    /// Events with absolute index `< n`. `n` is clamped to the log length.
    pub fn prefix(&self, n: usize) -> LogView<'_> {
        self.view().prefix(n)
    }

    /// Only the pure events, re-indexed from zero. This is the seed for
    /// re-execution: feed it to a live host and the world answers afresh.
    /// Request ids in the result will differ from the original, because
    /// they derive from indices.
    pub fn inputs(&self) -> Log {
        self.events
            .iter()
            .filter(|e| e.is_pure())
            .cloned()
            .collect()
    }
}

impl FromIterator<Event> for Log {
    fn from_iter<I: IntoIterator<Item = Event>>(iter: I) -> Self {
        Self {
            events: iter.into_iter().collect(),
        }
    }
}

/// An immutable window onto the log covering absolute indices
/// `[base, base + len)`. Events keep their absolute indices, so a view of
/// the tail folds exactly like the same events inside the whole log.
#[derive(Clone, Copy, Debug)]
pub struct LogView<'a> {
    events: &'a [Event],
    base: usize,
}

impl<'a> LogView<'a> {
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
    pub fn slice(&self, from: usize, to: usize) -> LogView<'a> {
        let from = from.clamp(self.base, self.end());
        let to = to.clamp(from, self.end());
        LogView {
            events: &self.events[from - self.base..to - self.base],
            base: from,
        }
    }

    /// Events with absolute index `< n`.
    pub fn prefix(&self, n: usize) -> LogView<'a> {
        self.slice(self.base, n)
    }

    /// Events with absolute index `>= from`.
    pub fn suffix(&self, from: usize) -> LogView<'a> {
        self.slice(from, self.end())
    }

    /// Events with their absolute indices, oldest first.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (Index, &'a Event)> + use<'a> {
        let base = self.base;
        self.events
            .iter()
            .enumerate()
            .map(move |(i, e)| ((base + i) as Index, e))
    }

    /// Absolute index of the most recent event, if any.
    pub fn last_index(&self) -> Option<Index> {
        (!self.events.is_empty()).then(|| (self.end() - 1) as Index)
    }

    /// The most recent event, if any.
    pub fn last(&self) -> Option<(Index, &'a Event)> {
        self.iter().next_back()
    }

    /// Virtual "now": the most recent `Tick`, or 0 before any tick.
    pub fn now(&self) -> u64 {
        crate::fold::now().run(*self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::{Effect, IdemKey};
    use crate::event::IoResult;

    #[test]
    fn prefix_clamps_and_indexes_from_zero() {
        let mut log = Log::new();
        assert_eq!(log.append(Event::tick(1)), 0);
        assert_eq!(log.append(Event::tick(2)), 1);
        assert_eq!(log.prefix(99).len(), 2);
        assert_eq!(log.prefix(0).now(), 0);
        assert_eq!(log.prefix(1).now(), 1);
        assert_eq!(log.view().now(), 2);
        assert_eq!(log.view().last_index(), Some(1));
        assert_eq!(log.prefix(0).last_index(), None);
    }

    #[test]
    fn slices_keep_absolute_indices() {
        let log: Log = (0..5).map(Event::tick).collect();
        let tail = log.view().suffix(3);
        assert_eq!(tail.base(), 3);
        assert_eq!(tail.end(), 5);
        assert_eq!(tail.iter().map(|(i, _)| i).collect::<Vec<_>>(), vec![3, 4]);
        assert_eq!(tail.last_index(), Some(4));
        // prefix of a suffix, and clamping in both directions
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
        let key = || "k".to_string();
        let log: Log = [
            Event::click(key()),
            Event::Started {
                key: key(),
                req: 0,
                effect: Effect::Ping {
                    key: key(),
                    idem: IdemKey {
                        scope: key(),
                        index: 0,
                    },
                },
            },
            Event::tick(16),
            Event::Io {
                key: key(),
                req: 0,
                res: IoResult::Done,
            },
        ]
        .into_iter()
        .collect();
        let inputs = log.inputs();
        assert_eq!(inputs.len(), 2);
        assert!(inputs.view().iter().all(|(_, e)| e.is_pure()));
    }
}
