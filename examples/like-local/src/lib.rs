//! The whole idea in one file: a like button.
//!
//! The UI is `render(fold(log))`. There is no state object anywhere.
//! The log is a list of events; the view is a fold over it; a click is
//! an event; the page renders one number the fold projects. That is all.
//!
//! Three things are yours: the state, the step, and the projection. The
//! declaration generates the rest.

use logfold_core::{Event, Projection};

logfold_core::component! {
    pub mod ui;
    domain Like;
    inputs { toggle => Toggle }
    root { class liked; }
    state View;
    step = step;
    project = project;
}

/// What the UI renders.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub liked: bool,
}

/// One event in, one view out. A click flips the flag; anything else
/// leaves it alone.
pub fn step(v: View, _: u64, ev: &Ev) -> View {
    match ev {
        Event::Input {
            input: Input::Toggle,
            ..
        } => View { liked: !v.liked },
        _ => v,
    }
}

/// One number on the skeleton: the class `liked` on the root, present or
/// absent. The stylesheet decides what liked looks like; no string for it
/// exists here.
pub fn project(v: &View) -> Projection {
    Projection::new().set(ui::liked.slot(), u8::from(v.liked))
}

#[cfg(test)]
mod tests {
    use super::*;
    use logfold_core::Log;

    fn click() -> Ev {
        component().input_event(0).unwrap()
    }

    #[test]
    fn empty_log_is_not_liked() {
        assert!(!component().fold.run(Log::<Ev>::new().view()).liked);
    }

    #[test]
    fn each_click_flips() {
        let log: Log<Ev> = [click(), click(), click()].into_iter().collect();
        let like = component().fold;
        assert!(like.run(log.prefix(1)).liked);
        assert!(!like.run(log.prefix(2)).liked);
        assert!(like.run(log.prefix(3)).liked);
    }

    #[test]
    fn ticks_do_not_flip() {
        let log: Log<Ev> = [click(), Ev::tick(16), Ev::tick(32)].into_iter().collect();
        assert!(component().fold.run(log.view()).liked);
    }

    #[test]
    fn projects_to_one_number() {
        let log: Log<Ev> = [click()].into_iter().collect();
        let p = component().projection().run(log.view());
        assert_eq!(p.get(ui::liked.slot()), Some(1.0));
        assert_eq!(p.len(), 1);
        assert_eq!(INPUTS.len(), 1);
        assert_eq!(INPUTS[0].0, "toggle");
        assert!(matches!(
            INPUTS[0].1,
            logfold_core::InputSpec::Unit(Input::Toggle)
        ));
    }
}
