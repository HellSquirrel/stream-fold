//! A counter. The second component, here to show what adding one costs:
//! a declaration, a state, a step, a projection. The skeleton renders the
//! number as text with a CSS counter and lights a bar with arithmetic on
//! the same attribute; see `www/counter.html`.

use logfold_core::{Event, Projection};

logfold_core::component! {
    pub mod ui;
    domain Counter;
    inputs { inc => Inc, dec => Dec, reset => Reset }
    root { attr count: int; }
    state View;
    step = step;
    project = project;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub n: i64,
}

pub fn step(v: View, _: u64, ev: &Ev) -> View {
    match ev {
        Event::Input {
            input: Input::Inc, ..
        } => View { n: v.n + 1 },
        Event::Input {
            input: Input::Dec, ..
        } => View { n: v.n - 1 },
        Event::Input {
            input: Input::Reset,
            ..
        } => View { n: 0 },
        _ => v,
    }
}

/// One number, the attribute `data-count` on the root. The stylesheet
/// bridges it into `--count` with typed `attr()`, selects on it directly
/// for the zero state, and everything visible follows from that.
pub fn project(v: &View) -> Projection {
    Projection::new().set(ui::count.slot(), v.n as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use logfold_core::{Log, checkpoint_law};

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
        assert_eq!(p.get(ui::count.slot()), Some(-1.0));
        assert_eq!(c.input_names().collect::<Vec<_>>(), ["inc", "dec", "reset"]);
        assert!(c.manifest.is_some());
        checkpoint_law(&c.fold, log.view()).unwrap();
    }
}
