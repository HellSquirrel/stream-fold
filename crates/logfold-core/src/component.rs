//! A component: a fold, its projection, its effects, optionally a simulated
//! world, and the inputs a host may dispatch, as one value. This is
//! everything a host needs to run a piece of UI (or a robot's panel) that
//! renders itself from numbers; see `project`.
//!
//! Values, not traits, like folds: build one with the builder and hand it
//! to a host. The host owns the log and the checkpoints; the component
//! owns meaning.

use std::collections::BTreeSet;
use std::rc::Rc;

use crate::event::{Domain, Event};
use crate::fold::Fold;
use crate::project::{Name, Projection};

/// What the simulated world does in one frame, given the state and the
/// frame's duration: the senses it reports. The tick itself is the host's.
pub type Simulate<D, X> = Rc<dyn Fn(&X, u64) -> Vec<Event<D>>>;

/// The effects that should be in flight, from state.
pub type Effects<D, X> = Rc<dyn Fn(&X) -> BTreeSet<<D as Domain>::Effect>>;

pub struct Component<D: Domain, X> {
    /// Scope key every dispatched input and started effect is appended under.
    pub key: Name,
    /// State from the log.
    pub fold: Fold<Event<D>, X>,
    /// Numbers on named targets from state.
    pub project: Rc<dyn Fn(&X) -> Projection>,
    /// The effects that should be in flight, from state. Level-triggered:
    /// the host diffs this against what it has started.
    pub effects: Effects<D, X>,
    /// A simulated world, if this component brings its own. A host with a
    /// real world (a network, a robot) leaves this `None`.
    pub simulate: Option<Simulate<D, X>>,
    /// What the skeleton may ask for by name (`data-on="click:toggle"`).
    pub inputs: Vec<(Name, D::Input)>,
}

impl<D: Domain, X> Clone for Component<D, X>
where
    X: Clone,
{
    fn clone(&self) -> Self {
        Self {
            key: self.key,
            fold: self.fold.clone(),
            project: self.project.clone(),
            effects: self.effects.clone(),
            simulate: self.simulate.clone(),
            inputs: self.inputs.clone(),
        }
    }
}

impl<D: Domain, X: Clone + 'static> Component<D, X> {
    /// A component with no projection, no effects, no world and no inputs yet.
    pub fn new(key: Name, fold: Fold<Event<D>, X>) -> Self {
        Self {
            key,
            fold,
            project: Rc::new(|_| Projection::new()),
            effects: Rc::new(|_| BTreeSet::new()),
            simulate: None,
            inputs: Vec::new(),
        }
    }

    pub fn project(mut self, f: impl Fn(&X) -> Projection + 'static) -> Self {
        self.project = Rc::new(f);
        self
    }

    pub fn effects(mut self, f: impl Fn(&X) -> BTreeSet<D::Effect> + 'static) -> Self {
        self.effects = Rc::new(f);
        self
    }

    pub fn simulate(mut self, f: impl Fn(&X, u64) -> Vec<Event<D>> + 'static) -> Self {
        self.simulate = Some(Rc::new(f));
        self
    }

    pub fn input(mut self, name: Name, input: D::Input) -> Self {
        self.inputs.push((name, input));
        self
    }

    /// The projection as a fold: `fold.map(project)`.
    pub fn projection(&self) -> Fold<Event<D>, X, Projection> {
        let project = self.project.clone();
        self.fold.clone().map(move |x| project(x))
    }

    /// The event a dispatched input becomes, if the id is known.
    pub fn input_event(&self, id: usize) -> Option<Event<D>> {
        self.inputs
            .get(id)
            .map(|(_, input)| Event::input(self.key, input.clone()))
    }

    pub fn input_names(&self) -> impl Iterator<Item = Name> + '_ {
        self.inputs.iter().map(|(n, _)| *n)
    }
}
