//! A todo list, built step by step.
//!
//! This is the first slice: typing something and pressing Enter adds an
//! item. Nothing can be toggled or removed yet; that is the next slice.
//! Grow it in this order, and `cargo test -p todo` will tell you when the
//! page's generated contract is stale:
//!
//! 1. `state`: what a todo list *is*.
//! 2. `inputs { … }`: what a person can do to it. `add: text => Add`
//!    carries what was typed; a plain `name => Variant` carries nothing.
//! 3. `step`: what each input does to the state. Pure.
//! 4. `root { … }` and `family … { … }`: what the page can show. Then
//!    `cargo xtask gen` to regenerate `www/gen/todo.*`.
//! 5. `project`: the state as numbers on those slots.
//! 6. `www/todo.html`: the skeleton and the stylesheet.
//!
//! Text never crosses the boundary as a string. The `Add` event carries
//! it into the log; an item remembers the log index of the event that
//! created it; the `title` text slot is set to that index, and the page
//! asks the host for the text behind it. So an item's id is free, and
//! scrubbing the timeline shows the right titles because they were
//! always in the log.

use logfold_core::{Event, Projection};

logfold_core::component! {
    pub mod ui;
    domain Todo;
    inputs { add: text => Add, toggle: index => Toggle }
    root { var count: int; }
    family item { attr present: bool; attr done: bool; text title; }
    state View;
    step = step;
    project = project;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// The log index of the `Add` that made it. Unique by construction.
    pub id: u64,
    pub text: String,
    pub done: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View {
    pub items: Vec<Item>,
}

/// One event in, one state out. An `Add` with something in it appends an
/// item named after its own log index; a `Toggle` flips the item in that
/// row, if there is one; blank input and every other event change nothing.
pub fn step(mut v: View, at: u64, ev: &Ev) -> View {
    match ev {
        Event::Input {
            input: Input::Add(text),
            ..
        } if !text.trim().is_empty() => {
            v.items.push(Item {
                id: at,
                text: text.trim().to_owned(),
                done: false,
            });
            v
        }
        Event::Input {
            input: Input::Toggle(row),
            ..
        } => {
            if let Some(item) = v.items.get_mut(*row as usize) {
                item.done = !item.done;
            }
            v
        }
    }
}

/// The count on the root; for each visible row, that it is present and
/// which log index holds its title.
pub fn project(v: &View) -> Projection {
    let mut p = Projection::new().set(ui::count.slot(), v.items.len() as u32);
    for (row, item) in v.items.iter().enumerate() {
        let row = row as u32;
        p = p
            .set(ui::item::present.at(row), 1u8)
            .set(ui::item::done.at(row), u8::from(item.done))
            .set(ui::item::title.at(row), item.id as f64);
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use logfold_core::{Log, checkpoint_law};

    fn add(text: &str) -> Ev {
        component().text_event(0, text).unwrap()
    }

    #[test]
    fn enter_adds_an_item_named_after_its_log_index() {
        let log: Log<Ev> = [Ev::tick(1), add("milk"), Ev::tick(2), add("  eggs ")]
            .into_iter()
            .collect();
        let v = component().fold.run(log.view());
        assert_eq!(v.items.len(), 2);
        assert_eq!(
            v.items[0],
            Item {
                id: 1,
                text: "milk".into(),
                done: false
            }
        );
        assert_eq!(
            v.items[1],
            Item {
                id: 3,
                text: "eggs".into(),
                done: false
            },
            "trimmed"
        );
        checkpoint_law(&component().fold, log.view()).unwrap();
    }

    #[test]
    fn blank_input_adds_nothing() {
        let log: Log<Ev> = [add(""), add("   "), Ev::tick(1)].into_iter().collect();
        assert_eq!(component().fold.run(log.view()), View::default());
    }

    #[test]
    fn the_projection_points_each_row_at_its_text() {
        let log: Log<Ev> = [add("milk"), add("eggs")].into_iter().collect();
        let p = component().projection().run(log.view());
        assert_eq!(p.get(ui::count.slot()), Some(2.0));
        assert_eq!(p.get(ui::item::present.at(0)), Some(1.0));
        assert_eq!(
            p.get(ui::item::title.at(0)),
            Some(0.0),
            "row 0 shows the text of event 0"
        );
        assert_eq!(p.get(ui::item::title.at(1)), Some(1.0));
        assert_eq!(p.get(ui::item::present.at(2)), None, "row 2 is empty");
        assert_eq!(p.get(ui::item::done.at(0)), Some(0.0));
        assert_eq!(p.len(), 7);
    }

    #[test]
    fn the_checkbox_flips_the_item_in_its_row() {
        let toggle = |row| component().index_event(1, row).unwrap();
        let log: Log<Ev> = [
            add("milk"),
            add("eggs"),
            toggle(1),
            toggle(1),
            toggle(0),
            toggle(9),
        ]
        .into_iter()
        .collect();
        let c = component();
        let v = c.fold.run(log.view());
        assert!(v.items[0].done, "toggled once");
        assert!(!v.items[1].done, "toggled twice");
        assert_eq!(v.items.len(), 2, "a row with no item is ignored");
        let p = c.projection().run(log.view());
        assert_eq!(p.get(ui::item::done.at(0)), Some(1.0));
        assert_eq!(p.get(ui::item::done.at(1)), Some(0.0));
        assert!(
            c.input_event(1).is_none(),
            "a toggle from no row is no event"
        );
        checkpoint_law(&c.fold, log.view()).unwrap();
    }

    #[test]
    fn rows_grow_with_the_list() {
        let log: Log<Ev> = (0..11).map(|i| add(&format!("item {i}"))).collect();
        let p = component().projection().run(log.view());
        assert_eq!(p.get(ui::item::present.at(10)), Some(1.0));
        assert_eq!(p.get(ui::item::title.at(10)), Some(10.0));
        assert_eq!(p.get(ui::count.slot()), Some(11.0));
        assert_eq!(ui::item::COUNT, None, "the page grows rows from a template");
    }

    #[test]
    fn the_text_is_in_the_log() {
        // what the host will answer when the page asks for a title's text
        let ev = add("milk");
        match &ev {
            Event::Input { input, .. } => {
                assert_eq!(<Todo as logfold_core::Domain>::text(input), Some("milk"))
            }
            _ => unreachable!(),
        }
    }

    /// The page's fragments are generated from the manifest by
    /// `cargo xtask gen`; this fails when they are stale.
    #[test]
    fn the_generated_fragments_are_current() {
        let www = concat!(env!("CARGO_MANIFEST_DIR"), "/../../www/gen/");
        let read = |f: &str| std::fs::read_to_string(format!("{www}{f}")).unwrap_or_default();
        assert_eq!(
            read("todo.css"),
            ui::MANIFEST.css(),
            "run `cargo xtask gen`"
        );
        assert_eq!(
            read("todo.manifest.mjs"),
            ui::MANIFEST.mjs(),
            "run `cargo xtask gen`"
        );
    }
}
