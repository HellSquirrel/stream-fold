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

use std::collections::BTreeMap;

/// A target or variable name. Static: names are part of the skeleton.
pub type Name = &'static str;

/// How a number lands on a target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SlotKind {
    /// A CSS custom property, `--name`. Inherits; feeds `calc()` and style queries.
    Var,
    /// An attribute, `data-name`. Visible in markup; feeds selectors and typed `attr()`.
    Attr,
}

/// One variable or attribute on one target, e.g. `("root", Var, "--liked")`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Slot {
    pub target: Name,
    pub kind: SlotKind,
    pub name: Name,
}

impl Slot {
    pub const fn var(target: Name, name: Name) -> Self {
        Self {
            target,
            kind: SlotKind::Var,
            name,
        }
    }

    pub const fn attr(target: Name, name: Name) -> Self {
        Self {
            target,
            kind: SlotKind::Attr,
            name,
        }
    }
}

/// A flat map from slots to numbers. `NaN` is never stored: it is the
/// wire encoding of "cleared", so a projection cannot contain it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Projection(BTreeMap<Slot, f64>);

impl Projection {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder: a custom property on a target.
    pub fn var(mut self, target: Name, name: Name, value: impl Into<f64>) -> Self {
        self.set(Slot::var(target, name), value.into());
        self
    }

    /// Builder: an attribute on a target.
    pub fn attr(mut self, target: Name, name: Name, value: impl Into<f64>) -> Self {
        self.set(Slot::attr(target, name), value.into());
        self
    }

    pub fn set(&mut self, slot: Slot, value: f64) {
        assert!(
            !value.is_nan(),
            "NaN is the wire encoding of a cleared slot"
        );
        self.0.insert(slot, value);
    }

    pub fn clear(&mut self, slot: Slot) {
        self.0.remove(&slot);
    }

    pub fn get(&self, slot: Slot) -> Option<f64> {
        self.0.get(&slot).copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = (Slot, f64)> + '_ {
        self.0.iter().map(|(s, v)| (*s, *v))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
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
/// both are not mentioned, so an unchanged world costs nothing.
pub fn diff(from: &Projection, to: &Projection) -> Vec<Change> {
    let mut out = Vec::new();
    for (slot, value) in to.iter() {
        if from.get(slot) != Some(value) {
            out.push(Change::Set(slot, value));
        }
    }
    for (slot, _) in from.iter() {
        if to.get(slot).is_none() {
            out.push(Change::Clear(slot));
        }
    }
    out
}

/// A model host: apply changes to a projection. This is what a real host
/// does to its skeleton, and what the tests use to prove `diff`.
pub fn apply(p: &mut Projection, changes: &[Change]) {
    for c in changes {
        match *c {
            Change::Set(slot, value) => p.set(slot, value),
            Change::Clear(slot) => p.clear(slot),
        }
    }
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
        assert_eq!(
            d,
            vec![
                Change::Set(Slot::var("root", "--new"), 4.0),
                Change::Set(Slot::var("root", "--y"), 5.0),
                Change::Clear(Slot::var("root", "--gone")),
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
        assert_eq!(p.get(Slot::var("root", "x")), Some(1.0));
        assert_eq!(p.get(Slot::attr("root", "x")), Some(2.0));
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
