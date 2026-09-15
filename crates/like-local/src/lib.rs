//! The whole idea in one file: a like button.
//!
//! The UI is `render(fold(log))`. There is no state object anywhere.
//! The log is a list of events; the view is a fold over it; a click is
//! an event. That is all.

use logfold_core::{Component, Domain, Event, Fold, Never, Projection};

/// The domain: what this app can say. One input, no senses, no effects.
pub struct Like;

impl Domain for Like {
    type Input = Click;
    type Sense = Never;
    type Effect = Never;
}

#[derive(Clone, Debug, PartialEq, Eq)]
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
pub fn like() -> Fold<Ev, View> {
    Fold::new(View::default(), step)
}

/// The one event this button produces.
pub fn click() -> Ev {
    Ev::input("like", Click)
}

/// The inputs a host may dispatch, by name. The skeleton names them
/// (`data-on="click:toggle"`); the host looks the name up once.
pub const INPUTS: &[(&str, Click)] = &[("toggle", Click)];

/// The view as one number on the skeleton: the attribute `data-liked` on
/// the root, so `html[data-liked="1"] …` selects it in every browser. The
/// stylesheet decides what liked looks like; no string for it exists in
/// Rust.
pub fn project(v: &View) -> Projection {
    Projection::new().attr("root", "data-liked", u8::from(v.liked))
}

/// The projection as a fold: `like().map(project)`.
pub fn projection() -> Fold<Ev, View, Projection> {
    like().map(project)
}

/// The like button as a component: fold, projection, and one input.
/// A host runs this; the skeleton refers to `toggle` and `--liked`.
pub fn component() -> Component<Like, View> {
    Component::new("like", like())
        .project(project)
        .input("toggle", Click)
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
    fn projects_to_one_number() {
        let log: Log<Ev> = [click()].into_iter().collect();
        let p = projection().run(log.view());
        assert_eq!(
            p.get(logfold_core::Slot::attr("root", "data-liked")),
            Some(1.0)
        );
        assert_eq!(p.len(), 1);
    }

    #[test]
    fn ticks_do_not_flip() {
        let log: Log<Ev> = [click(), Ev::tick(16), Ev::tick(32)].into_iter().collect();
        assert!(like().run(log.view()).liked);
    }
}
