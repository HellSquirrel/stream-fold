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
use crate::slots::Manifest;

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
    /// The declared contract, if the component has one. A host interns its
    /// names first, so ids match what the generated page expects.
    pub manifest: Option<&'static Manifest>,
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
            manifest: self.manifest,
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
            manifest: None,
        }
    }

    pub fn manifest(mut self, m: &'static Manifest) -> Self {
        self.manifest = Some(m);
        self
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

/// Declare a whole component: the `slots!` contract plus the domain marker,
/// the inputs as name-to-variant pairs, the state, and the step and project
/// functions by name. Everything a host needs comes out: the marker and its
/// `Domain` impl, an `Input` enum, `Ev`, `INPUTS`, and `component()` with
/// the manifest attached. What you write yourself is the state type, the
/// step, and the projection: the three things that are actually yours.
///
/// ```ignore
/// logfold_core::component! {
///     pub mod ui;
///     domain Like;
///     inputs { toggle => Toggle }
///     root { attr liked: bool; }
///     state View;                  // `state View = expr;` for a non-Default init
///     step = step;
///     project = project;
/// }
/// ```
///
/// Optional clauses, for components with a world: `sense SenseType;`,
/// `effect EffectType;`, `effects = f;`, `simulate = f;`, `key = "scope";`.
#[macro_export]
macro_rules! component {
    (
        $vis:vis mod $ui:ident;
        domain $dom:ident;
        $( key = $key:literal; )?
        $( sense $sense:ty; )?
        $( effect $effect:ty; )?
        inputs { $( $iname:ident => $ivar:ident ),* $(,)? }
        $( root { $($root:tt)* } )?
        $( family $fam:ident ($count:expr) { $($famslots:tt)* } )*
        $( consts { $($cname:ident : $cval:expr),* $(,)? } )?
        state $state:ty $( = $init:expr )?;
        step = $step:expr;
        project = $project:expr;
        $( effects = $effects:expr; )?
        $( simulate = $simulate:expr; )?
    ) => {
        $crate::slots! {
            $vis mod $ui;
            $( root { $($root)* } )?
            $( family $fam($count) { $($famslots)* } )*
            inputs { $( $iname ),* }
            $( consts { $( $cname : $cval ),* } )?
        }

        /// The inputs the skeleton can name, one variant per `data-on` name.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Input {
            $( $ivar ),*
        }

        /// The domain marker.
        pub struct $dom;

        impl $crate::Domain for $dom {
            type Input = Input;
            type Sense = $crate::component!(@ty $crate::Never $(; $sense)?);
            type Effect = $crate::component!(@ty $crate::Never $(; $effect)?);
        }

        pub type Ev = $crate::Event<$dom>;

        /// Input names in dispatch order, paired with their variants.
        pub const INPUTS: &[(&str, Input)] = &[ $( (stringify!($iname), Input::$ivar) ),* ];

        pub fn component() -> $crate::Component<$dom, $state> {
            let key: &'static str = $crate::component!(@key stringify!($dom) $(; $key)?);
            let init: $state = $crate::component!(@init $($init)?);
            #[allow(unused_mut)]
            let mut c = $crate::Component::new(key, $crate::Fold::new(init, $step))
                .manifest(&$ui::MANIFEST)
                .project($project);
            $( c = c.effects($effects); )?
            $( c = c.simulate($simulate); )?
            for (name, input) in INPUTS {
                c = c.input(name, *input);
            }
            c
        }
    };
    (@ty $default:ty) => { $default };
    (@ty $default:ty; $given:ty) => { $given };
    (@key $default:expr) => { $default };
    (@key $default:expr; $given:literal) => { $given };
    (@init) => { Default::default() };
    (@init $e:expr) => { $e };
}
