//! LogFold core: the log is the only truth, folds are values, effects are
//! values, time is an input.
//!
//! Nothing in this crate touches the world. A host shim appends [`Event`]s,
//! runs [`Fold`]s, and interprets [`Effect`]s. See `docs/logfold-proposal.md`.

pub mod effect;
pub mod event;
pub mod expect;
pub mod fold;
pub mod log;

pub use effect::{Effect, EffectDiff, IdemKey, diff_effects};
pub use event::{Event, Index, IoResult, Key, Origin, Pure, ReqId, UiEvent};
pub use expect::{Breach, Expectation, Mode, check_all_prefixes, guard};
pub use fold::{Checkpoints, Fold, checkpoint_law, in_flight, now};
pub use log::{Log, LogView};
