//! The whole idea in one file: a like button.
//!
//! The UI is `render(fold(log))`. There is no state object anywhere.
//! The log is a list of events; the view is a fold over it; a click is
//! an event. That is all.

use logfold_core::{Domain, Event, Fold, Never};

/// The domain: what this app can say. One input, no senses, no effects.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Like;

impl Domain for Like {
    type Input = Click;
    type Sense = Never;
    type Effect = Never;
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Click;

pub type Ev = Event<Like>;

/// What the UI renders.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub liked: bool,
}

/// One event in, one view out. A click flips the flag; anything else
/// leaves it alone.
pub fn step(v: View, _: u64, ev: &Ev) -> View {
    match ev {
        Event::Input { input: Click, .. } => View { liked: !v.liked },
        _ => v,
    }
}

/// The like button as a fold: start not liked, apply `step` per event.
pub fn like() -> Fold<'static, Ev, View> {
    Fold::new(View::default(), step)
}

/// The one event this button produces.
pub fn click() -> Ev {
    Ev::input("like", Click)
}

#[cfg(test)]
mod tests {
    use super::*;
    use logfold_core::Log;

    #[test]
    fn empty_log_is_not_liked() {
        assert!(!like().run(Log::<Ev>::new().view()).liked);
    }

    #[test]
    fn each_click_flips() {
        let log: Log<Ev> = [click(), click(), click()].into_iter().collect();
        assert!(like().run(log.prefix(1)).liked);
        assert!(!like().run(log.prefix(2)).liked);
        assert!(like().run(log.prefix(3)).liked);
    }

    #[test]
    fn ticks_do_not_flip() {
        let log: Log<Ev> = [click(), Ev::tick(16), Ev::tick(32)].into_iter().collect();
        assert!(like().run(log.view()).liked);
    }
}
