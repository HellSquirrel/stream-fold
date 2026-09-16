# Crafting UI with LogFold

A guide for building a piece of UI on this runtime, from a blank crate to
a page. It assumes you have read the one-paragraph pitch in
`docs/logfold-proposal.md`: state is never mutated, the UI is a fold over
an append-only log, effects are values, time is an event.

## The model in four sentences

1. Everything that happens is an **event** appended to a **log**: a user
   input, a tick of virtual time, a sensor reading, the world answering a
   request, the host recording that it started an effect.
2. Your state is a **fold** over the log: an initial value and a step
   function. There is no other state. Any prefix of the log gives a state;
   a **checkpoint** is the fold resumed from a saved state.
3. The page is a **projection** of the state: a flat map of numbers onto
   named targets, written into CSS custom properties and attributes. The
   stylesheet renders. Moving the page to another log index is a map diff,
   so rendering and scrubbing history are the same operation.
4. What you write is the state, the step, and the projection. A
   declaration generates everything else, including the page's side of the
   contract.

## What you write

A component is a crate with one declaration and three definitions:

| you write | what it is |
|---|---|
| `component! { … }` | the contract: targets, slots with types, inputs, constants |
| `struct View` | the state; `Default` gives the initial value |
| `fn step(View, Index, &Ev) -> View` | one event in, one state out; pure |
| `fn project(&View) -> Projection` | numbers onto the declared slots |

The declaration expands to the domain marker, an `Input` enum with one
variant per named input, `Ev`, `INPUTS`, and `component()` with the
manifest attached. Then one line in an app crate exports it as a wasm
class, and a page mounts it.

An input is a name the page can fire: `toggle => Toggle` becomes
`Input::Toggle`, and `data-on="click:toggle"` on any element fires it. An
input that carries what a person typed is `add: text => Add`; its variant
is `Add(String)`, and `data-on="enter:add"` on a text field fires it with
the field's value on Enter and empties the field. An input fired from
inside a family member is `toggle: index => Toggle`; its variant is
`Toggle(u32)` with the member's index, so a checkbox inside `item-3`
says "row 3" and the fold decides what row 3 means. See example 4.

## Example 1: a toggle

The like button, `examples/like-local/src/lib.rs`, without its tests:

```rust
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
    root { attr liked: bool; }
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

/// One number on the skeleton: `data-liked`, present or absent. The
/// stylesheet decides what liked looks like; no string for it exists here.
pub fn project(v: &View) -> Projection {
    Projection::new().set(ui::liked.slot(), u8::from(v.liked))
}
```

Three things to notice. The step matches on `Input::Toggle`, a variant the
declaration made from `inputs { toggle => Toggle }`. The projection puts
one number on the root through the typed slot `ui::liked`, so a typo is a
compile error. And there is no string "♥" anywhere in Rust; the stylesheet
owns it.

The page, `www/index.html`. The skeleton names the target and the input:

```html
<h1>Like</h1>
<p><button data-fold="like" data-on="click:toggle">like</button> <button id="tick">tick</button></p>
```

The stylesheet does everything visible from the attribute the host writes:

```css
  [data-fold=like] { font-size: 2rem; padding: .5rem 1.5rem; transition: scale .15s, color .15s, border-color .15s; }
  [data-fold=like]::before { content: "♡ "; }
  html[data-liked="1"] [data-fold=like] { scale: 1.1; color: #d32f2f; border-color: #d32f2f; }
  html[data-liked="1"] [data-fold=like]::before { content: "♥ "; }
```

And the script mounts the class through the generic shim; nothing here is
specific to a like button:

```js
  import init, { LikeApp } from "./pkg/like-app/like_app.js";
  import { mount } from "./logfold.mjs";
  import { timeline } from "./timeline.mjs";
  await init();
  const host = mount(new LikeApp());
  timeline(host, document.getElementById("history"));
  document.getElementById("tick").onclick = () => host.tick();
```

Export it as a bundle with one line in `examples/apps/like-app/src/lib.rs`:

```rust
logfold_web::export_component!(LikeApp, like_local::Like, like_local::View, like_local::component());
logfold_web::export_devtools!(LikeApp);   // the timeline's methods; drop for a page without one
```

and build it with `./scripts/build-www.sh like-app`.

## Example 2: a number, a bar and a disabled button

The counter, `examples/counter/src/lib.rs`:

```rust
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
```

Three inputs, one attribute. The page shows what CSS does with one number.
The digit is a CSS counter; the bar is ten cells lit by arithmetic with
`sibling-index()`, no per-cell markup; the reset button dims at zero by an
attribute selector:

```html
<h1>Counter</h1>
<p>
  <button data-on="click:dec">−</button>
  <span data-fold="count"></span>
  <button data-on="click:inc">+</button>
  <button data-on="click:reset">reset</button>
</p>
<div class="bar"><i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i></div>
```

```css
  /* the number as text: a CSS counter set from the variable */
  [data-fold=count] { display: inline-block; min-width: 3ch; text-align: center; font-size: 2rem; font-variant-numeric: tabular-nums; }
  [data-fold=count]::before { counter-reset: n var(--count); content: counter(n); }

  /* a bar of ten cells lit by arithmetic, no queries and no per-cell markup:
     cell i is lit when count >= i, and i is the cell's own sibling-index() */
  .bar { display: flex; gap: 4px; margin: 1rem 0; }
  .bar i { width: 2rem; height: 1rem; border-radius: 3px; background: #4a90e2; transition: opacity .2s;
           opacity: calc(0.15 + 0.85 * clamp(0, var(--count) - sibling-index() + 1, 1)); }

  /* reset looks idle at zero: the attribute is selectable directly */
  html[data-count="0"] [data-on="click:reset"] { opacity: .4; }
```

The typed `attr()` bridge, `html { --count: attr(data-count type(<integer>), 0); }`,
turns the attribute into an inherited variable, so both the selector world
and the `calc()` world see the same number from one write.

## Example 3: a component with a world

Brunhilda's studio, `examples/studio/src/lib.rs`, is the real app: a
control panel, a robot's brain and its simulated world as one fold on one
log. It declares with `slots!` and builds the component explicitly, because
it has senses, effects and a simulated world:

```rust
logfold_core::slots! {
    pub mod ui;
    root {
        attr running: bool;
        attr dropouts: bool;
        attr policy: enum { naive, careful };
        attr mode: enum { idle, cleaning, docking, stopped };
        attr latched: bool;
        attr attacking: bool;
        attr heading: enum { none, north, east, south, west };
        attr seen: bool;
        attr fresh: bool;
        var fps: int = 4;
        var her_x: int;
        var her_y: int;
        var human_x: int = 6;
        var human_y: int = 6;
        var seen_x: int = -1;
        var seen_y: int = -1;
        var seen_age: int = -1;
        var attacks: int;
        var bumps: int;
        var cleaned: int;
        var ticks: int;
    }
    family cell((brunhilda::W * brunhilda::H)) {
        attr cleaned: bool;
        attr furniture: bool;
        attr dock: bool;
    }
    inputs { run, pause, naive, careful, dropouts, faster, slower, start, dock, estop, north, east, south, west }
    consts { room_w: brunhilda::W, room_h: brunhilda::H, cell_px: 40 }
}
```

Booleans become attributes that are present or absent, enums become
attributes spelled by name, so the stylesheet reads
`html[data-mode="cleaning"]` and `html[data-running]`. The `cell` family is
96 targets named `cell-0` … `cell-95`, each with three boolean attributes.
Constants come from the same numbers the fold uses.

```rust
pub fn component() -> Component<App, State> {
    let room = Room::default();
    let (r1, r2, r3) = (room.clone(), room.clone(), room.clone());
    Component::new(
        KEY,
        Fold::new(State::new(&room), move |s, i, ev| step(&r1, s, i, ev)),
    )
    .manifest(&ui::MANIFEST)
    .project(move |s| project(&r2, s))
    .effects(|s| s.brain.desired_effects())
    .simulate(move |s, dt| simulate(&r3, s, dt))
    .input("run", Input::Panel(Panel::Run))
    .input("pause", Input::Panel(Panel::Pause))
    .input("naive", Input::Panel(Panel::Naive))
    .input("careful", Input::Panel(Panel::Careful))
    .input("dropouts", Input::Panel(Panel::ToggleDropouts))
    .input("faster", Input::Panel(Panel::Faster))
    .input("slower", Input::Panel(Panel::Slower))
    .input("start", Input::Robot(Cmd::Start))
    .input("dock", Input::Robot(Cmd::Dock))
    .input("estop", Input::Robot(Cmd::EStop))
    .input("north", Input::Human(Dir::N))
    .input("east", Input::Human(Dir::E))
    .input("south", Input::Human(Dir::S))
    .input("west", Input::Human(Dir::W))
}
```

`effects` is the set of effects that should be in flight, from state; the
host diffs it against what it has started and records a `Started` per new
one. `simulate` is what the world reports in one frame; the host's
`frame(dt)` appends those senses, then a tick, then acts on effects. A
component with a real world, a network or a robot, leaves `simulate` out
and its page appends senses itself.

Run `cargo xtask gen` and the page's side of the contract is written to
`www/gen/studio.css` (typed `@property` registrations and the constants)
and `www/gen/studio.manifest.mjs` (names, inputs, value tables). The page
links the CSS and passes the manifest to `mount`. A test fails when either
is stale.

## Example 4: text, without a string crossing

The todo list, `examples/todo/src/lib.rs`. Typing and pressing Enter adds
an item; the page shows the items' titles. Yet the boundary still moves
only numbers. The trick is where the text lives: in the log.

```rust
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

pub struct Item { pub id: u64, pub text: String, pub done: bool }   // id: the log index of the Add

pub fn step(mut v: View, at: u64, ev: &Ev) -> View {
    match ev {
        Event::Input { input: Input::Add(text), .. } if !text.trim().is_empty() => {
            v.items.push(Item { id: at, text: text.trim().to_owned(), done: false });
            v
        }
        Event::Input { input: Input::Toggle(row), .. } => {
            if let Some(item) = v.items.get_mut(*row as usize) { item.done = !item.done; }
            v
        }
        _ => v,
    }
}

pub fn project(v: &View) -> Projection {
    let mut p = Projection::new().set(ui::count.slot(), v.items.len() as u32);
    for (row, item) in v.items.iter().enumerate() {
        p = p.set(ui::item::present.at(row as u32), 1u8)
             .set(ui::item::done.at(row as u32), u8::from(item.done))
             .set(ui::item::title.at(row as u32), item.id as f64);
    }
    p
}
```

Three things to notice. The `Add` event carries the string into the log,
so the log is still the whole story and the timeline replays titles
correctly. The step gets the event's own index as its second argument,
which makes a free, unique item id. And a `text` slot is a number like
any other: the log index of the input whose text to show. The shim asks
the app for that text and writes it into the row's `[data-text="title"]`
child.

The page: a field that adds on Enter, and one template the rows grow from:

```html
<p><input data-on="enter:add" placeholder="what needs doing? press enter"></p>
<ul>
  <template data-fold="item"><li><input type="checkbox" data-on="click:toggle"><span data-text="title"></span></li></template>
</ul>
```

```css
  li[data-fold^="item-"] { display: none; }
  li[data-fold^="item-"][data-present] { display: flex; }
  li[data-fold^="item-"] input[type="checkbox"] { appearance: none; /* drawn from the row */ }
  li[data-fold^="item-"][data-done] input[type="checkbox"] { background: green; }
  li[data-fold^="item-"][data-done] [data-text="title"] { text-decoration: line-through; }
```

The checkbox is an input, not state. The shim cancels its native toggle
(every `data-on` handler calls `preventDefault`), the click reaches the
fold as `Toggle(row)`, and the row's `data-done` attribute, written by the
host, is what draws the tick. So scrubbing the timeline unticks it, the
same as everything else on the page.

`family item { … }` with no count is unbounded. The first patch that
names `item-N` makes the shim clone the template for every member up to
N, in order. Members are never removed, so a row that is no longer
present is hidden by CSS, and which item sits in which row is the
projection's decision. A family with a count, like the studio's
`cell(96)`, is pre-rendered instead.

## Testing without a browser

- **Unit**: fold a hand-written log and assert on the state; project it and
  assert on slots. `component().input_event(i)` gives you the event for
  input `i`; `component().text_event(i, "milk")` gives a text input its
  text.
- **Property**: write expectations as a fold plus a predicate with
  `Expectation::on`, and check them on every prefix of a generated log with
  `check_all_prefixes`. `checkpoint_law` checks that resuming from any
  split equals folding from zero. `examples/like-button/tests/fuzz.rs` is
  the worked example, with an adversarial world and a server model.
- **Host**: `logfold_web::Host` runs natively. The tests in
  `crates/logfold-web/src/host.rs` drive a component through inputs,
  frames and scrubs and check a model DOM against the projection after
  every patch.

## Rules that keep it simple

- Numbers cross the boundary; strings do not. Labels and glyphs live in
  the stylesheet. Text a person typed goes into the log as an input's
  payload and comes back to the page by log index through a `text` slot;
  it is never state the DOM owns.
- Appearance is never generated. The generator writes registrations,
  constants, names and value tables.
- Bounded structure is pre-rendered and toggled by numbers. Unbounded lists
  grow from a template, one flat member per index, never removed. Nothing
  diffs a tree.
- Time is the host's: the page owns the clock and calls `tick` or `frame`;
  the fold only ever sees ticks.
- An effect is a value the fold desires; the host records it before
  performing it. Nothing about rendering is ever logged.

## Where things are

| | |
|---|---|
| `crates/logfold-core` | the runtime: log, events, folds, checkpoints, projection, expectations, `slots!`, `component!` |
| `crates/logfold-web` | the generic browser host, `export_component!`, `export_devtools!`, `export_raw!` |
| `crates/xtask` | `cargo xtask gen` |
| `www/logfold.mjs`, `www/timeline.mjs`, `www/logfold-raw.mjs` | the shim, the devtools, the raw loader |
| `examples/*` | the components; `examples/apps/*` their bundles; `www/*.html` their pages |
| `docs/boundary.md` | why the boundary is shaped this way, with measurements |
