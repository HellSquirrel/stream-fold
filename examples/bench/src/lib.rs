//! The js-framework-benchmark table: create N rows, update every tenth,
//! select one, swap two, remove one, append, clear. The same operations
//! every framework publishes numbers for, so ours can be compared.
//!
//! What is different here, on purpose: a row's label is three numbers
//! (an adjective, a colour, a noun from a fixed vocabulary), rendered by
//! the stylesheet from enum classes, so no text ever crosses the
//! boundary; rows are addressed by position, so removing row 3 rewrites
//! every row after it; and rows are never removed from the DOM, only
//! hidden. The benchmark is meant to show what those choices cost.
//!
//! The rows live in a `TrackedVec`, and the family is drawn one row at a
//! time by `project_row`. From that the framework derives what each
//! event changes on the page, so selecting a row costs one write, not a
//! rebuild of 700,000 slots. Rows are keyed by id: removing one is one
//! member gone and an order number on the rows after it; the page drops
//! the node and moves nothing.

use logfold_core::{Change, Event, Projection, TrackedVec};

logfold_core::component! {
    pub mod ui;
    domain Bench;
    inputs {
        create: text => Create,
        append: text => Append,
        update => Update,
        select: index => Select,
        swap => Swap,
        remove: index => Remove,
        clear => Clear,
    }
    root { var count: int; }
    family row keyed {
        class present;
        class selected;
        attr id: int;
        attr bangs: int;
        class adj: enum { pretty, large, big, small, tall, short, long, handsome, plain, quaint, clean, elegant, easy, angry, crazy, helpful, mushy, odd, unsightly, adorable, important, inexpensive, cheap, expensive, fancy };
        class colour: enum { red, yellow, blue, green, pink, brown, purple, tan, white, black, orange };
        class noun: enum { table, chair, house, bbq, desk, car, pony, cookie, sandwich, burger, pizza, mouse, keyboard };
    }
    state State;
    step = step;
    project = project;
    project row from rows = project_row, key = row_key, context = selection, affects = affects;
}

pub const ADJECTIVES: u32 = 25;
pub const COLOURS: u32 = 11;
pub const NOUNS: u32 = 13;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: u32,
    pub adj: u8,
    pub colour: u8,
    pub noun: u8,
    /// How many times "update" has hit this row; the label shows it.
    pub bangs: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub rows: TrackedVec<Row>,
    /// The selected row's id, so selection survives a remove above it.
    pub selected: Option<u32>,
    pub next_id: u32,
}

/// A count typed into the harness; anything unparsable is the reference
/// benchmark's 1,000.
fn count(text: &str) -> usize {
    text.trim().parse().unwrap_or(1000)
}

/// Deterministic "random" labels: the log index and the row id seed a
/// xorshift, so the same log always builds the same table.
fn label(at: u64, id: u32) -> (u8, u8, u8) {
    let mut x = (at as u32).wrapping_mul(0x9E37_79B9) ^ id.wrapping_mul(0x85EB_CA6B) | 1;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x
    };
    (
        (next() % ADJECTIVES) as u8,
        (next() % COLOURS) as u8,
        (next() % NOUNS) as u8,
    )
}

impl State {
    fn extend(&mut self, n: usize, at: u64) {
        self.rows.reserve(n);
        for _ in 0..n {
            let id = self.next_id;
            self.next_id += 1;
            let (adj, colour, noun) = label(at, id);
            self.rows.push(Row {
                id,
                adj,
                colour,
                noun,
                bangs: 0,
            });
        }
    }
}

pub fn step(mut s: State, at: u64, ev: &Ev) -> State {
    let Event::Input { input, .. } = ev else {
        return s;
    };
    match input {
        Input::Create(n) => {
            s.rows.clear();
            s.selected = None;
            s.extend(count(n), at);
        }
        Input::Append(n) => s.extend(count(n), at),
        Input::Update => {
            for r in s.rows.iter_mut().step_by(10) {
                r.bangs += 1;
            }
        }
        Input::Select(p) => s.selected = s.rows.get(*p as usize).map(|r| r.id),
        Input::Swap => {
            // the reference swaps rows 1 and 998; on a shorter table, 1 and the second to last
            let len = s.rows.len();
            if len > 2 {
                s.rows.swap(1, (len - 2).min(998));
            }
        }
        Input::Remove(p) => {
            if (*p as usize) < s.rows.len() {
                s.rows.remove(*p as usize);
            }
        }
        Input::Clear => {
            s.rows.clear();
            s.selected = None;
        }
    }
    s
}

/// The root: one number.
pub fn project(s: &State) -> Projection {
    Projection::new().set(ui::count.slot(), s.rows.len() as u32)
}

/// A row is addressed by its id, so removing one is one member gone and
/// the rows after it only change their order number.
pub fn row_key(r: &Row) -> u32 {
    r.id
}

/// One row: seven slots, drawn from the row and the selection. `i` is
/// the row's key, its id.
pub fn project_row(i: u32, r: &Row, selected: &Option<u32>) -> [Change; 7] {
    [
        ui::row::present.at(i).set(1u8),
        ui::row::id.at(i).set(r.id),
        ui::row::bangs.at(i).set(r.bangs),
        ui::row::adj.at(i).set(r.adj),
        ui::row::colour.at(i).set(r.colour),
        ui::row::noun.at(i).set(r.noun),
        if *selected == Some(r.id) {
            ui::row::selected.at(i).set(1u8)
        } else {
            ui::row::selected.at(i).clear()
        },
    ]
}

/// What a row's slots depend on besides the row: the selection.
pub fn selection(s: &State) -> Option<u32> {
    s.selected
}

/// Which rows a selection touches: the selected one.
pub fn affects(selected: &Option<u32>, r: &Row) -> bool {
    *selected == Some(r.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use logfold_core::{Log, checkpoint_law, derivative_law};

    fn text(name: &str, t: &str) -> Ev {
        let c = component();
        let id = c.input_names().position(|n| n == name).unwrap();
        c.text_event(id, t).unwrap()
    }
    fn at(name: &str, i: u32) -> Ev {
        let c = component();
        let id = c.input_names().position(|n| n == name).unwrap();
        c.index_event(id, i).unwrap()
    }
    fn plain(name: &str) -> Ev {
        let c = component();
        let id = c.input_names().position(|n| n == name).unwrap();
        c.input_event(id).unwrap()
    }
    fn run(events: Vec<Ev>) -> State {
        let log: Log<Ev> = events.into_iter().collect();
        component().fold.run(log.view())
    }

    #[test]
    fn create_makes_n_rows_with_deterministic_labels() {
        let a = run(vec![text("create", "1000")]);
        let b = run(vec![text("create", "1000")]);
        assert_eq!(a.rows.len(), 1000);
        assert_eq!(a, b, "same log, same table");
        assert_eq!(a.rows[7].id, 7);
        assert!(a.rows.iter().all(|r| (r.adj as u32) < ADJECTIVES
            && (r.colour as u32) < COLOURS
            && (r.noun as u32) < NOUNS));
        let labels: std::collections::HashSet<_> =
            a.rows.iter().map(|r| (r.adj, r.colour, r.noun)).collect();
        assert!(labels.len() > 500, "labels vary: {} distinct", labels.len());
        assert_eq!(
            run(vec![text("create", "nonsense")]).rows.len(),
            1000,
            "the reference default"
        );
    }

    #[test]
    fn the_operations_do_what_the_reference_does() {
        let s = run(vec![text("create", "1000"), plain("update")]);
        assert!(
            s.rows
                .iter()
                .enumerate()
                .all(|(i, r)| r.bangs == u32::from(i % 10 == 0))
        );

        let s = run(vec![text("create", "1000"), plain("swap")]);
        assert_eq!((s.rows[1].id, s.rows[998].id), (998, 1));
        let s = run(vec![text("create", "10"), plain("swap")]);
        assert_eq!(
            (s.rows[1].id, s.rows[8].id),
            (8, 1),
            "short table: 1 and second to last"
        );

        let s = run(vec![
            text("create", "1000"),
            at("select", 5),
            at("remove", 3),
        ]);
        assert_eq!(s.rows.len(), 999);
        assert_eq!(s.rows[3].id, 4, "rows shift up");
        assert_eq!(
            s.selected,
            Some(5),
            "selection is by id and survives the shift"
        );
        let p = component().render(&s);
        assert_eq!(
            p.get(ui::row::selected.at(5)),
            Some(1.0),
            "and is drawn on its row, by id"
        );
        assert_eq!(p.get(ui::row::present.at(3)), None, "row 3 is gone");
        assert_eq!(
            p.get(logfold_core::Slot::order(logfold_core::Target::Indexed(
                "row", 5
            ))),
            Some(4.0),
            "id 5 now sits at position 4"
        );

        let s = run(vec![text("create", "1000"), text("append", "1000")]);
        assert_eq!(s.rows.len(), 2000);
        assert_eq!(s.rows[1999].id, 1999);

        let s = run(vec![
            text("create", "1000"),
            at("select", 1),
            plain("clear"),
        ]);
        assert_eq!(
            s,
            State {
                next_id: 1000,
                ..State::default()
            }
        );
        assert!(
            run(vec![
                at("remove", 0),
                at("select", 0),
                plain("swap"),
                plain("update")
            ])
            .rows
            .is_empty(),
            "nothing on an empty table"
        );
    }

    #[test]
    fn seven_numbers_per_row_and_a_count() {
        let s = run(vec![text("create", "3"), at("select", 1)]);
        let p = component().render(&s);
        assert_eq!(p.get(ui::count.slot()), Some(3.0));
        assert_eq!(
            p.len(),
            1 + 3 * 6 + 1 + 3,
            "count, six per row, one selected, three orders"
        );
        assert_eq!(p.get(ui::row::id.at(2)), Some(2.0));
        assert_eq!(p.get(ui::row::selected.at(1)), Some(1.0));
        assert_eq!(p.get(ui::row::selected.at(0)), None);
    }

    /// The framework derives the derivative: small operations give a few
    /// changes, bulk ones give `None`, and every answer agrees with a
    /// full render.
    #[test]
    fn the_derived_derivative_is_small_and_right() {
        let c = component();
        let mut s = run(vec![text("create", "100")]);
        s.rows.take_changes(); // a host takes the log after every event; `run` did not
        let step = |s: &mut State, ev: Ev| -> Option<usize> {
            let before = s.clone();
            *s = c.fold.step_one(s.clone(), 1, &ev);
            c.derive(&before, s, &ev).map(|d| d.len())
        };
        assert_eq!(step(&mut s, at("select", 5)), Some(1), "one class set");
        assert_eq!(
            step(&mut s, at("select", 7)),
            Some(2),
            "one cleared, one set"
        );
        assert_eq!(
            step(&mut s, plain("update")),
            Some(10),
            "every tenth row's counter"
        );
        assert_eq!(
            step(&mut s, plain("swap")),
            Some(2),
            "two order numbers; nothing else moves"
        );
        assert_eq!(
            step(&mut s, at("remove", 90)),
            Some(1 + 6 + 9 + 1),
            "one gone (order, six slots), nine shifted, the count"
        );
        assert_eq!(step(&mut s, Ev::tick(1)), Some(0), "a tick changes nothing");
        assert_eq!(
            step(&mut s, text("append", "5")),
            Some(5 * 7 + 1),
            "five rows, six sets and an order each, and the count"
        );
        assert_eq!(step(&mut s, text("create", "50")), None, "bulk: rebuild");
        assert_eq!(step(&mut s, plain("clear")), None);
    }

    #[test]
    fn checkpoints_resume_correctly_through_a_session() {
        let mut events = vec![text("create", "100")];
        for i in 0..40u32 {
            events.push(match i % 6 {
                0 => plain("update"),
                1 => at("select", i),
                2 => plain("swap"),
                3 => at("remove", i % 7),
                4 => text("append", "5"),
                _ => Ev::tick(u64::from(i)),
            });
        }
        let log: Log<Ev> = events.into_iter().collect();
        checkpoint_law(&component().fold, log.view()).unwrap();
    }

    /// Every derived derivative agrees with the full projection, event by
    /// event, on a session that removes, swaps and selects across the table.
    #[test]
    fn the_derivative_agrees_with_the_projection() {
        let mut events = vec![text("create", "60")];
        for i in 0..60u32 {
            events.push(match i % 8 {
                0 => plain("update"),
                1 => at("select", i % 50),
                2 => plain("swap"),
                3 => at("remove", i % 9),
                4 => at("select", 1),
                5 => text("append", "3"),
                6 => plain("clear"),
                _ => Ev::tick(u64::from(i)),
            });
            if i % 8 == 6 {
                events.push(text("create", "40"));
            }
        }
        let log: Log<Ev> = events.into_iter().collect();
        derivative_law(&component(), log.view()).unwrap();
        let log: Log<Ev> = [
            plain("update"),
            at("select", 0),
            plain("swap"),
            at("remove", 0),
        ]
        .into_iter()
        .collect();
        derivative_law(&component(), log.view()).unwrap();
    }
}
