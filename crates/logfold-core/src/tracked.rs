//! A `Vec` that remembers what changed. The state of a component with a
//! large family keeps its members in one of these; the step mutates it
//! exactly as it would a `Vec`, and the framework reads the log afterwards
//! to project only the members that moved. That is the ΔState of the
//! projection's derivative, produced for free by the mutation itself.
//!
//! Reads go through `Deref<Target = [T]>`: `len`, `iter`, `get`, `rows[i]`.
//! Every mutating method records what it touched; there is no unlogged
//! way to mutate. The log is not part of a value's identity: two tracked
//! vectors with the same items are equal whatever their logs say.

use std::ops::{Deref, Index, IndexMut};

/// What changed since the log was last taken.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    /// Everything may have changed: cleared, sorted, retained, drained.
    pub all: bool,
    /// Every member from this index on may have moved (an insert or a
    /// remove shifted them), or is gone (a truncate).
    pub from: Option<u32>,
    /// Members touched in place, by index, in touch order, possibly repeated.
    pub touched: Vec<u32>,
    /// The largest length the vector had during the batch, so a member
    /// that has since vanished can be cleared.
    pub high: u32,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        !self.all && self.from.is_none() && self.touched.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct TrackedVec<T> {
    items: Vec<T>,
    log: Changes,
}

impl<T> Default for TrackedVec<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: PartialEq> PartialEq for TrackedVec<T> {
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items
    }
}

impl<T: Eq> Eq for TrackedVec<T> {}

impl<T> Deref for TrackedVec<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.items
    }
}

impl<T> Index<usize> for TrackedVec<T> {
    type Output = T;
    fn index(&self, i: usize) -> &T {
        &self.items[i]
    }
}

impl<T> IndexMut<usize> for TrackedVec<T> {
    fn index_mut(&mut self, i: usize) -> &mut T {
        self.touch(i);
        &mut self.items[i]
    }
}

impl<T> From<Vec<T>> for TrackedVec<T> {
    fn from(items: Vec<T>) -> Self {
        let mut t = Self {
            items,
            log: Changes::default(),
        };
        t.log.all = true;
        t.note_len();
        t
    }
}

impl<T> FromIterator<T> for TrackedVec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Vec::from_iter(iter).into()
    }
}

impl<T> TrackedVec<T> {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            log: Changes::default(),
        }
    }

    fn note_len(&mut self) {
        self.log.high = self.log.high.max(self.items.len() as u32);
    }

    fn touch(&mut self, i: usize) {
        self.log.touched.push(i as u32);
    }

    fn shift_from(&mut self, i: usize) {
        self.note_len();
        let i = i as u32;
        self.log.from = Some(self.log.from.map_or(i, |f| f.min(i)));
    }

    /// Take the log and start a fresh one. `touched` is in the order the
    /// touches happened and may repeat; a reader marks, it does not sort.
    pub fn take_changes(&mut self) -> Changes {
        let mut log = std::mem::take(&mut self.log);
        log.high = log.high.max(self.items.len() as u32);
        log
    }

    pub fn get_mut(&mut self, i: usize) -> Option<&mut T> {
        if i < self.items.len() {
            self.touch(i);
        }
        self.items.get_mut(i)
    }

    /// Mutable iteration, recording each member as it is yielded, so
    /// `iter_mut().step_by(10)` records every tenth, not all.
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut {
            inner: self.items.iter_mut(),
            next: 0,
            log: &mut self.log,
        }
    }

    pub fn push(&mut self, x: T) {
        self.touch(self.items.len());
        self.items.push(x);
        self.note_len();
    }

    pub fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for x in iter {
            self.push(x);
        }
    }

    pub fn reserve(&mut self, n: usize) {
        self.items.reserve(n);
    }

    pub fn swap(&mut self, a: usize, b: usize) {
        self.touch(a);
        self.touch(b);
        self.items.swap(a, b);
    }

    pub fn insert(&mut self, i: usize, x: T) {
        self.shift_from(i);
        self.items.insert(i, x);
        self.note_len();
    }

    pub fn remove(&mut self, i: usize) -> T {
        self.shift_from(i);
        self.items.remove(i)
    }

    pub fn pop(&mut self) -> Option<T> {
        if !self.items.is_empty() {
            self.shift_from(self.items.len() - 1);
        }
        self.items.pop()
    }

    pub fn truncate(&mut self, n: usize) {
        if n < self.items.len() {
            self.shift_from(n);
        }
        self.items.truncate(n);
    }

    pub fn clear(&mut self) {
        self.note_len();
        self.log.all = true;
        self.items.clear();
    }

    pub fn retain(&mut self, f: impl FnMut(&T) -> bool) {
        self.note_len();
        self.log.all = true;
        self.items.retain(f);
    }

    pub fn sort_by(&mut self, f: impl FnMut(&T, &T) -> std::cmp::Ordering) {
        self.log.all = true;
        self.items.sort_by(f);
    }

    /// Replace everything.
    pub fn set(&mut self, items: Vec<T>) {
        self.note_len();
        self.log.all = true;
        self.items = items;
        self.note_len();
    }
}

pub struct IterMut<'a, T> {
    inner: std::slice::IterMut<'a, T>,
    next: usize,
    log: &'a mut Changes,
}

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = &'a mut T;
    fn next(&mut self) -> Option<&'a mut T> {
        let x = self.inner.next()?;
        self.log.touched.push(self.next as u32);
        self.next += 1;
        Some(x)
    }
    fn nth(&mut self, n: usize) -> Option<&'a mut T> {
        let x = self.inner.nth(n)?;
        self.next += n;
        self.log.touched.push(self.next as u32);
        self.next += 1;
        Some(x)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mutation_is_logged_and_reads_are_free() {
        let mut v: TrackedVec<u32> = (0..10).collect();
        assert!(v.take_changes().all, "built from a vec: everything");
        assert_eq!(v.len(), 10);
        assert_eq!(v[3], 3);
        assert_eq!(v.iter().sum::<u32>(), 45);
        assert!(v.take_changes().is_empty(), "reads log nothing");

        v[2] += 1;
        *v.get_mut(7).unwrap() += 1;
        v.swap(1, 8);
        for x in v.iter_mut().step_by(4) {
            *x += 100;
        }
        let mut ch = v.take_changes();
        ch.touched.sort();
        ch.touched.dedup();
        assert_eq!(ch.touched, vec![0, 1, 2, 4, 7, 8]);
        assert!(ch.from.is_none() && !ch.all);

        v.push(99);
        v.remove(3);
        let ch = v.take_changes();
        assert_eq!(ch.touched, vec![10], "the push, at its index");
        assert_eq!(ch.from, Some(3), "the remove shifted 3..");
        assert_eq!(ch.high, 11, "there were eleven before the remove");
        assert_eq!(v.len(), 10);

        v.truncate(4);
        let ch = v.take_changes();
        assert_eq!((ch.from, ch.high), (Some(4), 10));
        v.clear();
        assert!(v.take_changes().all);

        let a: TrackedVec<u32> = vec![1, 2].into();
        let mut b = TrackedVec::new();
        b.push(1);
        b.push(2);
        assert_eq!(a, b, "equal by items, whatever the logs");
    }
}
