//! Projection: a fold's state as a flat map of numbers on named targets.
//!
//! This is the output channel to a world that renders itself. A host owns
//! a static skeleton (HTML and CSS, a rig, a panel) with named targets, and
//! the projection says, for each `(target, variable)`, one number. The host
//! writes numbers; the skeleton's own rules (the cascade, a shader, a
//! motor controller) turn them into appearance and motion. Nothing here
//! knows what a target is.
//!
//! The diff between two projections is a set difference, so moving the
//! world from the state at one log index to another, forwards or
//! backwards, is the same operation. There is no tree, no identity, and
//! the correctness proof is [`apply`]: applying `diff(a, b)` to `a` gives
//! `b`, checked below.
//!
//! A slot is either a CSS custom property (`--count`) or an attribute
//! (`data-count`) on the target. Both carry one number. Variables inherit
//! down the tree and feed `calc()`; attributes are visible in the markup,
//! selectable everywhere (`[data-liked="1"]`), and typed `attr()` bridges
//! them into variables where that is supported. The projection chooses
//! per slot; the host writes whichever it is told.
//!
//! Numbers only, by design. Text and form values are the admitted
//! exceptions and get their own slot kind when an example needs one.

use std::cmp::Ordering;

/// A target or variable name. Static: names are part of the skeleton.
pub type Name = &'static str;

/// How a number lands on a target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SlotKind {
    /// A keyed family member's position among its siblings; cleared, the
    /// member is gone. Written by the framework, never declared. First,
    /// so that a member's removal precedes its other clears in a diff.
    Order,
    /// A CSS custom property, `--name`. Inherits; feeds `calc()` and style queries.
    Var,
    /// An attribute, `data-name`, carrying a number. Feeds typed `attr()` and
    /// value selectors like `[data-count="0"]`.
    Attr,
    /// A class, `name` for a boolean, `name-value` for an enum. The fastest
    /// selector the platform has; state that rules key on goes here.
    Class,
    /// The text content of the target's `[data-text="name"]` child, or of
    /// the target itself. The number is the log index of the input event
    /// that carried the text; the host hands the page the text behind it.
    Text,
}

/// Where a slot lives: the document root, an element the skeleton named
/// with `data-fold="name"`, or member `i` of a family named `name-i`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Root,
    Named(Name),
    Indexed(Name, u32),
}

/// Names are `&'static str`, almost always the very same string from the
/// declaration, so equal pointers settle a comparison before any byte is
/// read. Same order as a byte comparison, since equal pointers mean equal
/// bytes.
#[inline]
fn name_cmp(a: Name, b: Name) -> Ordering {
    if std::ptr::eq(a, b) {
        Ordering::Equal
    } else {
        a.cmp(b)
    }
}

impl Ord for Target {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Target::Root, Target::Root) => Ordering::Equal,
            (Target::Root, _) => Ordering::Less,
            (_, Target::Root) => Ordering::Greater,
            (Target::Named(a), Target::Named(b)) => name_cmp(a, b),
            (Target::Named(_), Target::Indexed(..)) => Ordering::Less,
            (Target::Indexed(..), Target::Named(_)) => Ordering::Greater,
            (Target::Indexed(a, i), Target::Indexed(b, j)) => name_cmp(a, b).then(i.cmp(j)),
        }
    }
}

impl PartialOrd for Target {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl From<&'static str> for Target {
    fn from(name: &'static str) -> Self {
        if name == "root" {
            Target::Root
        } else {
            Target::Named(name)
        }
    }
}

/// One variable or attribute on one target, e.g. `(Root, Var, "--liked")`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub target: Target,
    pub kind: SlotKind,
    pub name: Name,
}

impl Ord for Slot {
    fn cmp(&self, other: &Self) -> Ordering {
        self.target
            .cmp(&other.target)
            .then(self.kind.cmp(&other.kind))
            .then_with(|| name_cmp(self.name, other.name))
    }
}

impl PartialOrd for Slot {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Slot {
    pub const fn var(target: Target, name: Name) -> Self {
        Self {
            target,
            kind: SlotKind::Var,
            name,
        }
    }

    pub const fn attr(target: Target, name: Name) -> Self {
        Self {
            target,
            kind: SlotKind::Attr,
            name,
        }
    }

    pub const fn text(target: Target, name: Name) -> Self {
        Self {
            target,
            kind: SlotKind::Text,
            name,
        }
    }

    pub const fn class(target: Target, name: Name) -> Self {
        Self {
            target,
            kind: SlotKind::Class,
            name,
        }
    }

    /// The position of a keyed family member; see [`SlotKind::Order`].
    pub const fn order(target: Target) -> Self {
        Self {
            target,
            kind: SlotKind::Order,
            name: "order",
        }
    }

    /// This slot set to `value`, as a change. What a member projection
    /// returns: `ui::row::id.at(i).set(r.id)`.
    pub fn set(self, value: impl Into<f64>) -> Change {
        Change::Set(self, value.into())
    }

    /// This slot cleared, as a change.
    pub const fn clear(self) -> Change {
        Change::Clear(self)
    }
}

/// A flat map from slots to numbers. `NaN` is never stored: it is the
/// wire encoding of "cleared", so a projection cannot contain it.
///
/// A projection is built once, by a `project` function writing every slot,
/// and then read in order, by a diff. That is a sorted `Vec`'s job, not a
/// tree's: writes go to a batch in the order made, and the batch is sorted
/// (stable, so the last write to a slot wins) and merged into the sorted
/// store the first time the projection is read. Measured at 600,000 slots
/// per event, a B-tree cost 180 ms to build by insertion and, bulk-built,
/// linked 17 KB of the standard library's sort into every bundle. The
/// B-tree stays where it earns its keep: the checkpoint store, whose job
/// is random insertion and nearest-key lookup.
#[derive(Clone, Debug, Default)]
pub struct Projection {
    /// Sorted by slot, no duplicates. A cleared slot stays in place with
    /// `NaN` for a value, a tombstone: clearing and re-setting a slot in a
    /// large store is then a binary search, not a memmove of the store.
    /// Tombstones are compacted away whenever the store is rebuilt or
    /// merged, and no read ever sees one.
    sorted: Vec<(Slot, f64)>,
    /// Writes since the last settle, in the order they were made.
    pending: Vec<(Slot, f64)>,
}

impl PartialEq for Projection {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

/// Stable bottom-up merge sort. Small, and nothing to link: the standard
/// library's sort is a large piece of code for a bundle that sorts one
/// kind of thing. Already sorted input, the usual case, is one pass.
fn sort_stable(v: &mut Vec<(Slot, f64)>) {
    let n = v.len();
    if n < 2 || v.windows(2).all(|w| w[0].0 <= w[1].0) {
        return;
    }
    let mut src = std::mem::take(v);
    let mut dst: Vec<(Slot, f64)> = Vec::with_capacity(n);
    let mut width = 1;
    while width < n {
        dst.clear();
        let mut lo = 0;
        while lo < n {
            let mid = (lo + width).min(n);
            let hi = (lo + 2 * width).min(n);
            let (mut i, mut j) = (lo, mid);
            while i < mid && j < hi {
                if src[j].0 < src[i].0 {
                    dst.push(src[j]);
                    j += 1;
                } else {
                    dst.push(src[i]);
                    i += 1;
                }
            }
            dst.extend_from_slice(&src[i..mid]);
            dst.extend_from_slice(&src[j..hi]);
            lo = hi;
        }
        std::mem::swap(&mut src, &mut dst);
        width *= 2;
    }
    *v = src;
}

impl Projection {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold pending writes into the sorted store. A host calls it after
    /// `project` returns; any read that finds writes pending does the same
    /// on a copy.
    pub fn finish(mut self) -> Self {
        self.settle();
        self
    }

    fn settle(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let mut batch = std::mem::take(&mut self.pending);
        sort_stable(&mut batch);
        // equal slots: the later write wins, in the earlier position
        batch.dedup_by(|later, earlier| {
            if later.0 == earlier.0 {
                *earlier = *later;
                true
            } else {
                false
            }
        });
        if self.sorted.is_empty() {
            self.sorted = batch;
            return;
        }
        // a few writes into a large store: in place, no full merge
        if batch.len() * 32 < self.sorted.len() {
            for (slot, value) in batch {
                match self.sorted.binary_search_by(|(s, _)| s.cmp(&slot)) {
                    Ok(i) => self.sorted[i].1 = value,
                    Err(i) => self.sorted.insert(i, (slot, value)),
                }
            }
            return;
        }
        let old = std::mem::take(&mut self.sorted);
        let mut out = Vec::with_capacity(old.len() + batch.len());
        let (mut i, mut j) = (0, 0);
        while i < old.len() && j < batch.len() {
            match old[i].0.cmp(&batch[j].0) {
                Ordering::Less => {
                    if !old[i].1.is_nan() {
                        out.push(old[i]);
                    }
                    i += 1;
                }
                Ordering::Greater => {
                    out.push(batch[j]);
                    j += 1;
                }
                Ordering::Equal => {
                    out.push(batch[j]);
                    i += 1;
                    j += 1;
                }
            }
        }
        out.extend(old[i..].iter().filter(|(_, v)| !v.is_nan()));
        out.extend_from_slice(&batch[j..]);
        self.sorted = out;
    }

    /// The store with any pending writes folded in; a copy when needed.
    fn settled(&self) -> std::borrow::Cow<'_, [(Slot, f64)]> {
        if self.pending.is_empty() {
            std::borrow::Cow::Borrowed(&self.sorted)
        } else {
            std::borrow::Cow::Owned(self.clone().finish().sorted)
        }
    }

    /// Builder: a custom property on a target. `"root"` is the root.
    pub fn var(self, target: impl Into<Target>, name: Name, value: impl Into<f64>) -> Self {
        self.set(Slot::var(target.into(), name), value)
    }

    /// Builder: an attribute on a target. `"root"` is the root.
    pub fn attr(self, target: impl Into<Target>, name: Name, value: impl Into<f64>) -> Self {
        self.set(Slot::attr(target.into(), name), value)
    }

    /// Builder: any slot, typically one declared with `slots!`.
    pub fn set(mut self, slot: Slot, value: impl Into<f64>) -> Self {
        self.put(slot, value.into());
        self
    }

    /// Write one slot in place.
    pub fn put(&mut self, slot: Slot, value: f64) {
        assert!(
            !value.is_nan(),
            "NaN is the wire encoding of a cleared slot"
        );
        self.pending.push((slot, value));
    }

    /// Reserve a slot as cleared: a tombstone in place, so that a later
    /// `set` of it is a replacement, not an insertion into a large store.
    /// A family's render does this for every slot a member draws as
    /// cleared. Reads never see it.
    pub fn put_absent(&mut self, slot: Slot) {
        self.pending.push((slot, f64::NAN));
    }

    pub fn clear(&mut self, slot: Slot) {
        self.settle();
        if let Ok(i) = self.sorted.binary_search_by(|(s, _)| s.cmp(&slot)) {
            self.sorted[i].1 = f64::NAN;
        }
    }

    pub fn get(&self, slot: Slot) -> Option<f64> {
        if let Some((_, v)) = self.pending.iter().rev().find(|(s, _)| *s == slot) {
            return (!v.is_nan()).then_some(*v);
        }
        None.or_else(|| {
            self.sorted
                .binary_search_by(|(s, _)| s.cmp(&slot))
                .ok()
                .map(|i| self.sorted[i].1)
                .filter(|v| !v.is_nan())
        })
    }

    /// Slots in order. Pending writes cost a settled copy; a host's
    /// projections are finished, so they never pay it.
    pub fn iter(&self) -> Box<dyn Iterator<Item = (Slot, f64)> + '_> {
        match self.settled() {
            std::borrow::Cow::Borrowed(v) => {
                Box::new(v.iter().copied().filter(|(_, v)| !v.is_nan()))
            }
            std::borrow::Cow::Owned(v) => Box::new(v.into_iter().filter(|(_, v)| !v.is_nan())),
        }
    }

    pub fn len(&self) -> usize {
        self.iter().count()
    }

    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }
}

/// One write the host must perform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    Set(Slot, f64),
    Clear(Slot),
}

impl Change {
    pub fn slot(&self) -> Slot {
        match self {
            Change::Set(s, _) | Change::Clear(s) => *s,
        }
    }
}

/// What the host must write to move from `from` to `to`. Slots equal in
/// both are not mentioned, so an unchanged world costs nothing. One merge
/// walk over the two sorted stores: linear, no lookups.
pub fn diff(from: &Projection, to: &Projection) -> Vec<Change> {
    let (from, to) = (from.settled(), to.settled());
    let live = |v: &&(Slot, f64)| !v.1.is_nan();
    let mut a = from.iter().filter(live).peekable();
    let mut b = to.iter().filter(live).peekable();
    let mut out = Vec::new();
    loop {
        match (a.peek(), b.peek()) {
            (None, None) => break,
            (Some((sa, _)), None) => {
                out.push(Change::Clear(*sa));
                a.next();
            }
            (None, Some((sb, vb))) => {
                out.push(Change::Set(*sb, *vb));
                b.next();
            }
            (Some((sa, va)), Some((sb, vb))) => match sa.cmp(sb) {
                Ordering::Less => {
                    out.push(Change::Clear(*sa));
                    a.next();
                }
                Ordering::Greater => {
                    out.push(Change::Set(*sb, *vb));
                    b.next();
                }
                Ordering::Equal => {
                    if va != vb {
                        out.push(Change::Set(*sb, *vb));
                    }
                    a.next();
                    b.next();
                }
            },
        }
    }
    out
}

/// Apply changes to a projection, in order: what a host does to its
/// skeleton, what the tests use to prove `diff`, and how a derivative's
/// changes land on the projection the DOM holds. Runs of sets settle as
/// one batch.
pub fn apply(p: &mut Projection, changes: &[Change]) {
    for c in changes {
        match *c {
            Change::Set(slot, value) => p.put(slot, value),
            Change::Clear(slot) => p.clear(slot),
        }
    }
    p.settle();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(pairs: &[(&'static str, f64)]) -> Projection {
        pairs
            .iter()
            .fold(Projection::new(), |p, (v, x)| p.var("root", v, *x))
    }

    #[test]
    fn diff_mentions_only_what_changed() {
        let a = p(&[("--x", 1.0), ("--y", 2.0), ("--gone", 3.0)]);
        let b = p(&[("--x", 1.0), ("--y", 5.0), ("--new", 4.0)]);
        let d = diff(&a, &b);
        // one merge walk: changes come out in slot order
        assert_eq!(
            d,
            vec![
                Change::Clear(Slot::var(Target::Root, "--gone")),
                Change::Set(Slot::var(Target::Root, "--new"), 4.0),
                Change::Set(Slot::var(Target::Root, "--y"), 5.0),
            ]
        );
        assert!(diff(&a, &a).is_empty());
    }

    #[test]
    fn a_variable_and_an_attribute_of_the_same_name_are_different_slots() {
        let p = Projection::new()
            .var("root", "x", 1.0)
            .attr("root", "x", 2.0);
        assert_eq!(p.len(), 2);
        assert_eq!(p.get(Slot::var(Target::Root, "x")), Some(1.0));
        assert_eq!(p.get(Slot::attr(Target::Root, "x")), Some(2.0));
    }

    #[test]
    fn applying_the_diff_reaches_the_target_both_ways() {
        let states = [
            p(&[]),
            p(&[("--a", 1.0)]),
            p(&[("--a", 1.0), ("--b", 0.5)]),
            p(&[("--b", 0.5)]),
            p(&[("--a", 2.0), ("--b", 0.5), ("--c", -1.0)]),
        ];
        for a in &states {
            for b in &states {
                let mut world = a.clone();
                apply(&mut world, &diff(a, b));
                assert_eq!(&world, b, "from {a:?} to {b:?}");
            }
        }
    }

    #[test]
    #[should_panic(expected = "NaN is the wire encoding")]
    fn nan_is_rejected() {
        let _ = Projection::new().var("root", "--x", f64::NAN);
    }
}
