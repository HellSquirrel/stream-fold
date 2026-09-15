//! A counter. The second component, here to show what adding one costs:
//! a domain, a step, a projection, and a component value. The skeleton
//! renders the number as text with a CSS counter and lights a bar with
//! arithmetic on the same variable; see `www/counter.html`.

use logfold_core::{Component, Domain, Event, Fold, Never, Projection};

pub struct Counter;

impl Domain for Counter {
    type Input = Cmd;
    type Sense = Never;
    type Effect = Never;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    Inc,
    Dec,
    Reset,
}

pub type Ev = Event<Counter>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub n: i64,
}

pub fn step(v: View, _: u64, ev: &Ev) -> View {
    match ev {
        Event::Input {
            input: Cmd::Inc, ..
        } => View { n: v.n + 1 },
        Event::Input {
            input: Cmd::Dec, ..
        } => View { n: v.n - 1 },
        Event::Input {
            input: Cmd::Reset, ..
        } => View { n: 0 },
        _ => v,
    }
}

pub fn fold() -> Fold<Ev, View> {
    Fold::new(View::default(), step)
}

/// One number, the attribute `data-count` on the root. The stylesheet
/// bridges it into `--count` with typed `attr()`, selects on it directly
/// for the zero state, and everything visible follows from that.
pub fn project(v: &View) -> Projection {
    Projection::new().attr("root", "data-count", v.n as f64)
}

pub fn component() -> Component<Counter, View> {
    Component::new("counter", fold())
        .project(project)
        .input("inc", Cmd::Inc)
        .input("dec", Cmd::Dec)
        .input("reset", Cmd::Reset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use logfold_core::{Log, Slot, checkpoint_law};

    #[test]
    fn counts_and_projects() {
        let c = component();
        let log: Log<Ev> = [0usize, 0, 1, 0, 2, 1]
            .into_iter()
            .map(|id| c.input_event(id).unwrap())
            .collect();
        let want = [1, 2, 1, 2, 0, -1];
        for (n, v) in c.fold.scan(log.view()).skip(1) {
            assert_eq!(v.n, want[n - 1], "after event {}", n - 1);
        }
        let p = c.projection().run(log.view());
        assert_eq!(p.get(Slot::attr("root", "data-count")), Some(-1.0));
        assert_eq!(c.input_names().collect::<Vec<_>>(), ["inc", "dec", "reset"]);
        checkpoint_law(&c.fold, log.view()).unwrap();
    }
}
