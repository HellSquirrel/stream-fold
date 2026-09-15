//! Expectations: executable properties over the log.
//!
//! An expectation is built from a fold and a predicate on its output
//! ([`Expectation::on`]), so checking it on every prefix is one
//! [`Fold::scan`] pass. [`Expectation::raw`] wraps an arbitrary function
//! of the view for the few properties that are not folds; it costs a full
//! recompute per prefix and says so.
//!
//! Two execution modes exist today:
//! - **guard**: check the current log once (dev-mode, per frame);
//! - **fuzz**: check every prefix, so a property that holds at the end but
//!   broke in the middle is still caught.
//!
//! The fluent DSL of proposal §4.8 and the static (typestate) mode are
//! deferred until there are enough expectations to generalise from.

use std::rc::Rc;

use crate::event::Index;
use crate::fold::Fold;
use crate::log::LogView;

/// A failed expectation, pointing at the log index where it first broke.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Breach {
    pub name: &'static str,
    /// Index of the last event in the shortest failing prefix.
    /// `None` means the empty log already fails.
    pub at: Option<Index>,
    pub msg: String,
}

impl std::fmt::Display for Breach {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.at {
            Some(i) => write!(
                f,
                "expectation `{}` breached at index {}: {}",
                self.name, i, self.msg
            ),
            None => write!(
                f,
                "expectation `{}` breached on empty log: {}",
                self.name, self.msg
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Guard,
    AllPrefixes,
}

type Check<E> = Rc<dyn for<'l> Fn(LogView<'l, E>, Mode) -> Result<(), Breach>>;

pub struct Expectation<E> {
    pub name: &'static str,
    check: Check<E>,
}

impl<E> Clone for Expectation<E> {
    fn clone(&self) -> Self {
        Self {
            name: self.name,
            check: self.check.clone(),
        }
    }
}

fn breach_at(name: &'static str, prefix_end: usize, msg: String) -> Breach {
    Breach {
        name,
        at: prefix_end.checked_sub(1).map(|i| i as Index),
        msg,
    }
}

impl<E: 'static> Expectation<E> {
    /// A predicate on a fold's output. All-prefix mode is a single scan.
    pub fn on<X: Clone + 'static, S: 'static>(
        name: &'static str,
        fold: Fold<E, X, S>,
        pred: impl Fn(&S) -> Result<(), String> + 'static,
    ) -> Self {
        let check = move |log: LogView<'_, E>, mode: Mode| match mode {
            Mode::Guard => pred(&fold.run(log)).map_err(|m| breach_at(name, log.end(), m)),
            Mode::AllPrefixes => fold
                .scan(log)
                .find_map(|(n, out)| pred(&out).err().map(|m| breach_at(name, n, m)))
                .map_or(Ok(()), Err),
        };
        Self {
            name,
            check: Rc::new(check),
        }
    }

    /// An arbitrary function of the view. All-prefix mode recomputes it for
    /// every prefix, so this is quadratic; prefer [`Expectation::on`].
    pub fn raw(
        name: &'static str,
        f: impl for<'l> Fn(LogView<'l, E>) -> Result<(), String> + 'static,
    ) -> Self {
        let check = move |log: LogView<'_, E>, mode: Mode| match mode {
            Mode::Guard => f(log).map_err(|m| breach_at(name, log.end(), m)),
            Mode::AllPrefixes => (log.base()..=log.end())
                .find_map(|n| f(log.prefix(n)).err().map(|m| breach_at(name, n, m)))
                .map_or(Ok(()), Err),
        };
        Self {
            name,
            check: Rc::new(check),
        }
    }
}

impl<E> Expectation<E> {
    pub fn check(&self, log: LogView<'_, E>, mode: Mode) -> Result<(), Breach> {
        (self.check)(log, mode)
    }
}

/// Guard mode: evaluate every expectation against the full view once.
pub fn guard<E>(log: LogView<'_, E>, exps: &[Expectation<E>]) -> Result<(), Breach> {
    exps.iter().try_for_each(|e| e.check(log, Mode::Guard))
}

/// Fuzz mode: evaluate every expectation against every prefix and report
/// the earliest breach per expectation, first expectation wins.
pub fn check_all_prefixes<E>(log: LogView<'_, E>, exps: &[Expectation<E>]) -> Result<(), Breach> {
    exps.iter()
        .try_for_each(|e| e.check(log, Mode::AllPrefixes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::Log;

    fn at_most_one_event() -> Expectation<u8> {
        Expectation::on("at_most_one", Fold::new(0usize, |n, _, _| n + 1), |n| {
            if *n <= 1 {
                Ok(())
            } else {
                Err(format!("len = {n}"))
            }
        })
    }

    #[test]
    fn prefix_check_finds_earliest_breach() {
        let log: Log<u8> = (0..3).collect();
        let b = check_all_prefixes(log.view(), &[at_most_one_event()]).unwrap_err();
        assert_eq!(b.at, Some(1));
        assert_eq!(b.name, "at_most_one");
    }

    #[test]
    fn guard_only_checks_the_end() {
        let log: Log<u8> = (0..1).collect();
        assert!(guard(log.view(), &[at_most_one_event()]).is_ok());
    }

    #[test]
    fn raw_matches_on_for_the_same_property() {
        let log: Log<u8> = (0..4).collect();
        let raw = Expectation::raw("at_most_one", |v: LogView<'_, u8>| {
            if v.len() <= 1 {
                Ok(())
            } else {
                Err(format!("len = {}", v.len()))
            }
        });
        assert_eq!(
            check_all_prefixes(log.view(), &[raw]),
            check_all_prefixes(log.view(), &[at_most_one_event()])
        );
    }
}
