//! LogFold core: the log is the only truth, folds are values, effects are
//! values, time is an input.
//!
//! Nothing in this crate touches the world. A host shim appends [`Event`]s,
//! runs [`Fold`]s, and interprets a domain's effects. What a domain can
//! say is declared by a [`Domain`] implementation. See
//! `docs/logfold-proposal.md`.

pub mod component;
pub mod effect;
pub mod event;
pub mod expect;
pub mod fold;
pub mod log;
pub mod project;
pub mod slots;
pub mod tracked;

pub use component::{Component, Delta, FamilyPlan, InputSpec, derivative_law};
pub use effect::{EffectDiff, IdemKey, diff_effects};
pub use event::{Action, Domain, Event, Index, IoResult, Key, Never, Origin, ReqId};
pub use expect::{Breach, Expectation, Mode, check_all_prefixes, guard};
pub use fold::{Checkpoints, Fold, checkpoint_law, in_flight, now};
pub use log::{Log, LogView};
pub use project::{Change, Name, Projection, Slot, SlotKind, Target, apply, diff};
pub use slots::{Declared, Manifest, SlotDecl, SlotType, TargetDecl};
pub use tracked::{Changes, TrackedVec};
