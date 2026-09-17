# The JS ↔ Rust boundary

*Design note, Sept 2026. Implemented for the like button in `crates/logfold-web`
(`LikeApp`, `www/index.html`), then for the counter and for Brunhilda's studio,
which replaced the hand-written canvas host.*

## The problem this replaces

The first host exposed the fold field by field (`liked()`, `liked_at(n)`)
and the page's JavaScript decided how each field became DOM. It worked
because the view was one boolean, and it had no shape: no statement of what
the DOM *is* in terms of the log, domain knowledge in JavaScript, a rewrite
per new element. The obvious repair, a view tree in Rust and a patch stream
of DOM operations, is a tree diff with better hygiene, which is the MV* move
again. We went a step further out.

## The one invariant

> **The DOM is the materialisation of `project(fold(log[..at]))`.**

`project` is a pure function from fold state to a *projection*: a flat map
from `(target, variable)` to a number. The host owns a static skeleton, HTML
with named targets and a stylesheet, and writes those numbers into CSS custom
properties. The stylesheet renders. The host's only state beyond the DOM is
`at`, a log index, and the projection at `at`.

Every operation the boundary offers is one of two things: **append** an event,
or **move `at`**. Moving `at` is a set difference between two projections, so
rendering after a click and scrubbing the timeline are the same operation
in different directions, and the devtools slider drives the real button.

This is proposal §3 principle 3 (effects are values) plus the v0.2 decision
that the DOM is an *output*: an idempotent function of state, diffed by the
host against the world. Here the diff is between two maps of numbers.

## Who does what

```
   DOM ──events──► shim ──dispatch(input id)──► core: log ─► fold ─► projection
    ▲               │ ▲                                                  │
    │               │ └──── names, once, as handles ◄────────────────────┤
    └── setProperty ┘ ◄──── patch(at → n): [target, var, value]… ◄───────┘
```

**The skeleton (HTML + CSS) owns:** structure, every appearance decision,
every motion, the names of targets (`data-fold="like"`) and of inputs
(`data-on="click:toggle"`). The string "♥" exists only in the stylesheet.

**Rust owns:** the log, the folds, checkpoints, the projection function, the
diff, and the tables that turn names into small integers. It never touches
the DOM and never sees a glyph.

**The shim owns:** a delegated click listener that turns `data-on` names into
`dispatch` calls, and a loop that writes `[target, var, value]` triples with
`setProperty`, or `removeProperty` when the value is `NaN`. It is generic. The
like page contains no like-specific JavaScript; the rest of its script is the
timeline, which is a second host for the same log.

## What crosses, in which direction

| direction | what | encoding |
|---|---|---|
| JS → Rust | `dispatch(input, index, bytes)` | integer id from `input_names()`; the family member the input was fired from, or -1; the UTF-8 of a form field's value, empty for a click. An input ignores what it does not carry |
| JS → Rust | `tick(ms)` | number |
| JS → Rust | `render_at(n)` | integer |
| Rust → JS | patch | `Float64Array` of `[target, index, kind, name, value]`; kind 0 = custom property, 1 = attribute, 2 = text, 3 = class; `NaN` = clear |
| Rust → JS | `name(id)` | a JS string handle, fetched once per id and cached by the shim |
| Rust → JS | `text(i)` | the text input event `i` carried, as a string handle; a kind-2 value is such an `i` |
| Rust → JS | timeline metadata | numbers and handles, as before |

No string crosses per frame. Names cross once, as handles, the first time an
id appears. `dispatch` appends the event and returns the patch to the new
head, so a click is one round trip. An unchanged projection returns an empty
patch; a tick costs nothing. Text crosses when a person types it, once,
into the log; a text slot points back at it by index, so re-rendering or
scrubbing never re-sends it.

## The like button, end to end

```html
<button data-fold="like" data-on="click:toggle">like</button>
```
```css
[data-fold=like] { transition: scale .15s, color .15s; }
[data-fold=like]::before { content: "♡ "; }
html[data-liked="1"] [data-fold=like] { scale: 1.1; color: #d32f2f; }
html[data-liked="1"] [data-fold=like]::before { content: "♥ "; }
```
```rust
pub fn project(v: &View) -> Projection {
    Projection::new().set(ui::liked.slot(), u8::from(v.liked))   // root { class liked; }
}
pub const INPUTS: &[(&str, Click)] = &[("toggle", Click)];
```

The class lives on the root so that `html.liked …` can select anything on
the page. The transition on `scale` means the button animates
on every click and on every scrub. Nothing in Rust knows there is a heart.

## Three ways a number can land: a variable, an attribute or a class

A slot is a CSS custom property on the target (`--count`), an attribute
(`data-count`), or a class (`liked`, `mode-cleaning`). Same number, same
diff, same host loop; the declaration chooses per slot and the shim calls
`setProperty`, `setAttribute` or `classList`. They differ in what the
stylesheet can do with them:

| | custom property | attribute | class |
|---|---|---|---|
| inherits down the tree | yes | no | no |
| usable in `calc()` and transitions | yes | through typed `attr()` (Chrome 133+) | no |
| conditional rules | `@container style()` (2023+) | value selectors, every browser | the cheapest selector there is |
| carries | any number | any number | a boolean or an enum |

Classes are for state because selector matching and style invalidation are
built around them. Measured in Chrome 152, 5,000 rows restyled per toggle:
a boolean on the root costs 2.6 ms as a class against 4.5 ms as an
attribute; an enum switch 2.5 ms against 9.2 ms; a per-row boolean on
every row 4.3 ms against 5.6 ms. Attributes carry numbers the stylesheet
reads by value: `html { --count: attr(data-count type(<integer>), 0); }`
turns `data-count` into an inherited variable, and `[data-count="0"]`
selects the zero state. The like button is one class on the root; the
counter writes `data-count`, bridges it, and lights its bar with
`sibling-index()` so the cells carry no per-cell markup.

There is no reverse bridge: nothing on the platform sets a class or an
attribute from a style. State flows Rust → class, attribute or variable →
stylesheet, never back.

## What CSS renders from a number alone

Booleans and enums through `@container style(--v: n)`. Integers as visible
text through `counter-set` from a registered `@property` and
`content: counter()`. Positions through `calc(var(--x) * 1px)`. Visibility
through the same queries. Motion through `transition` on registered
properties, so scrubbing between log indices animates for free. And the state
is inspectable in the browser's own Styles panel, a devtools pane nobody had
to write.

## The two admitted exceptions

Numbers cover appearance. Two things are not appearance and get their own
slot kind *when an example needs them*, not before:

1. **Text.** A message body cannot come from `content:`; generated content is
   neither selectable nor fully accessible. The todo list needed it, so it
   exists: a `text` slot is written into `textContent`, and its number is
   the log index of the input event that carried the text. The text
   itself entered the log as that input's payload (`add: text => Add`),
   so the log stays the whole story and no string is ever DOM-owned. The
   shim fetches it by index and writes the target's `[data-text="name"]`
   child, or the target itself. Labels and glyphs still do not need it.
2. **Form values.** An input's `value` is a property, not a style. Same
   second-class slot, still without an example. The todo field is not
   one: the host never writes it, the shim empties it after Enter. Nor is
   the todo checkbox: its native toggle is cancelled and the tick is drawn
   from the row's `done` class.

Both are still flat writes to a named target. There is still no tree.

## Structure that changes

Three cases, in order of how often they happen:

- **Bounded variation** is pre-rendered and toggled by variables. Brunhilda's
  room is 96 cells that always exist; she is one element whose `--x` and
  `--y` change, and a CSS transition makes her glide between cells.
- **Unbounded lists** are a family without a count. The page holds one
  `<template data-fold="item">`; the first patch that names `item-N` makes
  the shim clone members up to N, in order. Members are flat, addressed by
  index, and never removed; a row that is gone is hidden. The todo list
  is the example. A long chat may still want a window on top of this.
- **Arbitrary nesting** would need islands cloned from a `<template>` by
  key. No example needs it yet, and the chat client will say whether it is
  real.

## Correctness

`logfold_core::project::diff` is proved by `apply`: applying `diff(a, b)` to
`a` yields `b`, for every pair in a set of projections, in both directions.
The host test in `logfold-web` drives forty events, scrubs through the
indices in an awkward order, clicks while scrubbed, and checks after every
step that a model DOM equals the projection at `at`. Neither test needs a
browser. The browser check is a single question: does the stylesheet render
the variable, and Chrome says yes.

## The contract, declared once

Targets, slots and inputs used to be strings written twice, in Rust and in
the page, with nothing checking they agreed. `logfold_core::slots!` is the
single declaration:

```rust
logfold_core::slots! {
    pub mod ui;
    root {
        class running;                                         // html.running, present or absent
        class mode: enum { idle, cleaning, docking, stopped };  // html.mode-cleaning
        var fps: int = 4;                                       // --fps, @property <integer>, initial 4
    }
    family cell((brunhilda::W * brunhilda::H)) { class cleaned; }   // targets cell-0 … cell-95
    inputs { run, pause, start, dock, estop, north, east, south, west }
    consts { room_w: brunhilda::W, room_h: brunhilda::H, cell_px: 40 }
}
```

It expands to typed constants, `ui::mode.slot()`, `ui::mode::cleaning`,
`ui::cell::cleaned.at(i)`, and a `Manifest`. Names are the identifiers as
written, so what you read in Rust is what you write in CSS.

From the manifest, the app crate's `build.rs` writes the page's side into
`www/gen/` on every build:

- `<app>.css`: a typed `@property` per variable and the constants on
  `:root`. Appearance is never generated; the stylesheet stays the
  designer's. The studio's room is `repeat(var(--room_w), calc(var(--cell_px) * 1px))`.
- `<app>.manifest.mjs`: names by id, inputs in dispatch order, families
  and their sizes, constants, and how each slot's number is spelled.

The host interns the manifest's names first, so ids are fixed by the
declaration and the shim resolves them without asking. The shim spells a
boolean class as present or absent and an enum class as `name-value`, so
the studio's stylesheet reads `html.mode-cleaning` and `html.running`.
Families cross as a target id plus an index, and the page names its cells
`cell-<i>`.

One assertion guards it: the component's inputs must equal the manifest's,
in order (the host checks at construction). The fragments cannot go stale
separately from the bundle, because the same build writes both; the
component crate carries no test about pages.

The like button and the counter have no declaration and lose nothing:
without a manifest the shim asks for names and writes numbers, as before.

## Adding a component

Three things are yours: the state, the step, and the projection. One
declaration generates the rest. The like button, whole:

```rust
logfold_core::component! {
    pub mod ui;
    domain Like;
    inputs { toggle => Toggle }
    root { attr liked: bool; }
    state View;
    step = step;
    project = project;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct View { pub liked: bool }

pub fn step(v: View, _: u64, ev: &Ev) -> View {
    match ev {
        Event::Input { input: Input::Toggle, .. } => View { liked: !v.liked },
        _ => v,
    }
}

pub fn project(v: &View) -> Projection {
    Projection::new().set(ui::liked.slot(), u8::from(v.liked))
}
```

`component!` takes the `slots!` grammar plus four clauses and expands to the
domain marker and its `Domain` impl, an `Input` enum with one variant per
`data-on` name, `Ev`, `INPUTS`, and `component()` with the manifest
attached. Components with a world add `sense T;`, `effect T;`,
`effects = f;` and `simulate = f;`. Then one line in an app crate,
`logfold_web::export_component!(LikeApp, like_local::Like, like_local::View, like_local::component());`,
or `export_raw!` for the raw boundary, and `scripts/build-www.sh` builds it
into its own bundle.

The page:

```html
<button data-fold="like" data-on="click:toggle">like</button>
```
```css
html[data-liked="1"] [data-fold=like]::before { content: "♥ "; }
```
```js
import init, { LikeApp } from "./pkg/like-app/like_app.js";
import { mount } from "./logfold.mjs";
await init();
mount(new LikeApp());
```

Pass `{ manifest }` from the generated module to `mount` and the shim
spells booleans by presence and enums by name, `html[data-liked]`; without
it, numbers, as above. `mount` installs one delegated listener per event
type the skeleton mentions, resolves `data-on` names to input ids once, and
applies patches. `timeline` is a second host for the same log. Neither knows
what a like is. Two components on one page are two `mount` calls with a
`root` option each.

## The studio: effects, a simulated world, one log

`examples/studio` is the first real app, and it added the last two things a
component can declare:

- **`effects`**: the effects that should be in flight, from state. After
  every append the host diffs them against the `in_flight` fold and records
  a `Started` for each one to start. Performing an effect is the page's
  business; the studio's only effect, the e-stop, is read straight back
  from the log by the fold as a latch, so nothing needs performing.
- **`simulate`**: what the world does in one frame, from state. The host's
  `frame(dt)` appends those senses, then a tick, then acts on effects. A
  component with a real world (a network, a robot) leaves this out and the
  page appends senses itself.

The studio's fold has three parts that used to be three things: the panel
(UI state), Brunhilda's brain (unchanged, driven by the policy the panel
chose), and the world the sim kept in memory, now derivable because your
arrow keys are inputs. One log, so one timeline scrubs everything.

The page has 96 cells that always exist, each a target with `data-cleaned`,
`data-furniture` and `data-dock` attributes, and three actors positioned by
`translate: calc(var(--her-x) * 40px) …` with a transition. She glides
because CSS interpolates the variable change; no animation value exists in
Rust. Enums and booleans arrive as attributes on the root and drive the
panel's highlights, her colour, her heading arrow, the mode text, and the
ring around a fresh sighting. Counts render as CSS counters.

The hand-written canvas host it replaced is gone.

## The raw boundary

The boundary is a few functions over numbers and a few names, so it does
not need wasm-bindgen. `export_raw!` in `logfold-web` puts the same `Host`
behind `extern "C"` exports, `lf_dispatch(input, index, len) -> len`, `lf_patch_ptr()`,
`lf_name_ptr(id)` and `lf_name_len(id)`, one instance per module. The
loader in `www/logfold-raw.mjs` instantiates the module with no imports,
decodes each name once from linear memory with `TextDecoder`, and reads a
patch in place as a `Float64Array` view, which is valid until the next call
and is applied synchronously by `mount`. It presents the same method names
as a generated class, so `mount` and `timeline` do not know which kind of
app they were given.

The host library's wasm-bindgen surface is a cargo feature, `bindgen`, on
by default. A raw app turns it off, because any crate that links
wasm-bindgen gets its runtime exports whether it uses them or not.
`like-raw` is the like button built this way, with plain `cargo` and
`wasm-opt`. Measured: 38.6 KB raw, 13.6 KB brotli, and no glue file, against
43.0 KB plus 8 KB of generated JavaScript for the wasm-bindgen build. Inside
the wasm, wasm-bindgen's own share was about 4 KB; most of what looked like
glue in the profile is the host itself.

What the raw path gives up: `JsValue` handles. Text that originates in the
browser goes in as bytes: the loader asks for `lf_scratch(len)`, writes the
UTF-8 there, and calls `lf_dispatch(input, index, len)`; text comes back out of
the log as `lf_text_ptr(i)` and `lf_text_len(i)`, decoded by the loader on
demand. Same shape as names: bytes in linear memory, no handle, no copy the
module did not already have to make to keep the event.

## The floor, measured

A second pass over the raw like bundle, 38.6 KB, asked what is left to
take. Everything below was measured, not estimated.

- **Toolchain knobs are spent.** All the extra wasm-opt passes together
  (`--converge`, stripping producers and target features, low-memory-unused,
  zero-filled memory) save 0.5 KB and are now on for every bundle.
  `--remap-path-prefix` saves 0.2 KB of panic-location paths. `opt-level`
  `z` and `s` make the raw file smaller and the brotli file *larger*, by
  0.7 and 0.4 KB, so opt-level 3 stays. `#[inline(never)]` on the
  fold-resume path saves 0.5 KB by not duplicating it into four callers.
- **The panic path is 5.9 KB** in the named build: std's hook, `core::fmt`,
  the payload types, the OOM hook. A `no_std` raw build with an abort-only
  panic handler would keep core's argument formatters, which are reachable
  through vtables from bounds checks, and drop the std half: about 2 KB.
- **The allocator is 4.3 KB.** `talc` with its memory-growing OOM handler.
  A bump allocator is a few hundred bytes but leaks, which a growing log
  cannot afford; a free-list allocator with a static arena is about 1.5 KB.
  Two to three kilobytes available.
- **The uninhabited-effect machinery is small.** For a domain with no
  effects the in-flight set is a `BTreeSet<Infallible>`; the compiler
  removes the unreachable inserts and what remains is under a kilobyte of
  fold plumbing.
- **The host is the host.** `append`, `render_at`, `act` and `frame` are
  about 9 KB between them: the checkpoint resume, the fold chain through
  boxed closures, the diff and the encoder. That is the design's cost,
  and the B-trees are a decision. Together they are roughly 20 KB of the
  38.

So the remaining honest target is about 33 KB raw, 11.5 KB brotli, by going
`no_std` and swapping the allocator, and nothing below that without
touching the design or the B-trees. For scale, 13.6 KB today is already
under a third of a React runtime.

## Measured at scale

`examples/bench` is the js-framework-benchmark table as a component: rows
are addressed by position, a label is three enum classes the stylesheet
spells out, and rows are never removed from the DOM, only hidden. Its page
sweeps N = 10, 100, 1 000, … and stops when an operation passes a gap or
the next tenfold step is projected to. Chrome 152 on the development
machine, one run, milliseconds; "wasm" is the fold, projection and diff,
"DOM" the shim's writes, "style" a forced style and layout pass after:

| N | op | wasm | DOM | writes | style | total |
|---|---|---|---|---|---|---|
| 1,000 | create | 4 | 20 | 6,001 | 36 | 60 |
| 1,000 | update every 10th | 0.1 | 0 | 100 | 9 | 9 |
| 1,000 | select | 0.1 | 0 | 1 | 0 | 0 |
| 1,000 | swap | 0.1 | 0 | 8 | 3 | 3 |
| 1,000 | remove | 1 | 10 | 3,982 | 25 | 36 |
| 1,000 | append 1,000 | 2 | 11 | 6,001 | 56 | 69 |
| 1,000 | clear | 1 | 21 | 11,996 | 12 | 34 |
| 10,000 | create | 12 | 99 | 60,001 | 323 | 434 |
| 10,000 | select | 0.2 | 0 | 1 | 0 | 0 |
| 10,000 | remove | 10 | 84 | 39,960 | 287 | 381 |
| 10,000 | append 10,000 | 20 | 108 | 60,001 | 546 | 675 |
| 100,000 | create | 152 | 989 | 600,001 | 2,772 | 3,913 |
| 100,000 | select | 1 | 0 | 1 | 0 | 1 |
| 100,000 | remove | 113 | 847 | 399,136 | 3,274 | 4,234 |
| 100,000 | append 100,000 | 171 | 1,091 | 600,001 | 4,068 | 5,331 |

What the numbers say:

- **A small operation now costs what it changes.** Selecting one row is
  2 ms at 100,000 rows, because the framework derives what the event
  changed from the rows' own change log and the host sends exactly that.
  It was 825 ms when every event rebuilt a B-tree projection and diffed it
  by lookup, and 91 ms after the projection became a sorted `Vec`; the two
  sections after the comparison tell that story.
- **Position addressing makes remove O(n).** Removing row 3 rewrites every
  row after it: 40,000 writes at 10,000 rows. Keying members by row id
  would make it one clear, at the price of the DOM growing with every
  create. Neither is free; this one keeps the DOM bounded.
- **Style and layout dominate a create**, more than the DOM writes: the
  stylesheet renders 100,000 labels from classes and two counters. That is
  the browser's cost for the "CSS renders numbers" bet, and it is in the
  same range as the DOM writes a conventional framework would make.
- **The shim was the first bottleneck**, not the design. Before the sweep,
  each new member was found with a `querySelector` over the document and
  inserted on its own, which made append 3.8 s at 10,000 rows. Indexing
  targets once at mount and inserting a grown family as one fragment
  brought it to 96 ms.

### Against vanilla JS and React 18, same machine, same harness

`www/bench-vs.html` runs the same seven operations through three
implementations in one tab: this component, a vanilla implementation in
the reference benchmark's shape (keyed rows, direct DOM, `textContent`),
and React 18 with memoised rows and `flushSync`. Same timing: the
operation, then a forced style and layout pass. Totals in milliseconds:

| op | N | LogFold | vanilla | React 18 |
|---|---|---|---|---|
| create | 1,000 | 60 | 22 | 29 |
| update every 10th | 1,000 | 9 | 4 | 6 |
| select | 1,000 | 0 | 0 | 1 |
| swap | 1,000 | 3 | 3 | 24 |
| remove | 1,000 | 36 | 2 | 2 |
| append | 1,000 | 69 | 28 | 31 |
| clear | 1,000 | 34 | 4 | 8 |
| create | 10,000 | 434 | 236 | 408 |
| update every 10th | 10,000 | 86 | 39 | 46 |
| select | 10,000 | 0 | 0 | 4 |
| swap | 10,000 | 33 | 15 | 45 |
| remove | 10,000 | 381 | 15 | 22 |
| append | 10,000 | 675 | 282 | 487 |
| clear | 10,000 | 257 | 41 | 70 |
| create | 100,000 | 4,715 | 2,995 | 17,344 |
| update every 10th | 100,000 | 930 | 543 | 869 |
| select | 100,000 | 2 | 0 | 24 |
| swap | 100,000 | 356 | 249 | 409 |
| remove | 100,000 | 4,234 | 285 | 319 |
| append | 100,000 | 8,311 | 2,934 | 17,661 |
| clear | 100,000 | 2,719 | 433 | 2,674 |

Read across:

- **Creating is two to three times vanilla and about twice React**, until
  100,000 rows, where React's reconciliation falls behind and this is
  1.6× vanilla. Two things make the gap: six property or class writes per
  row instead of one `innerHTML`, and a style pass roughly twice
  vanilla's, because labels are rendered from classes, pseudo-elements and
  counters rather than text nodes.
- **Small operations no longer pay for the whole table.** Selecting one
  row is 2 ms at 100,000 rows against vanilla's 0 and React's 24; it was
  797 ms with the B-tree projection and 91 ms with the sorted `Vec`. The
  derived derivative did that; see below. What remains on update and swap is
  style and layout, the same as everyone's.
- **Remove is the worst ratio**, 20× vanilla at 1,000 rows and 30× at
  10,000, because a row is a position and removing one rewrites every row
  below it. Vanilla removes one node.
- **Clear** is a full diff to NaN for every slot plus one class removal
  per row, against vanilla's single `textContent = ""`.
- **CSS counters are sequential.** The first page rendered each row's id
  with `counter-reset: id var(--id)` and `content: counter(id)`. A counter's
  value depends on every element before it, so changing one row's counter
  restyles every row after it: swapping two rows of 100,000 cost 363 ms of
  style. Rendering the number with `content: attr(data-id)` instead, which
  is per element, took that to 282 ms, and create's style pass from 3.7 s
  to 2.8 s. The 282 ms that remain are the browser's automatic table
  layout reflowing the table, which vanilla pays too (294 ms). Numbers
  that other elements never depend on belong in attributes, not counters.
- **Automatic table layout is the other sequential thing.** A `<table>`
  sizes its columns by measuring every cell, so any change to any row
  reflows the table: swap cost 282 ms of layout at 100,000 rows for us and
  294 for vanilla. Rows laid out as grid rows with fixed columns do not do
  that, and `content-visibility: auto` on a row lets the browser skip
  style and layout for rows that are off screen. Applied to all three
  implementations, so the comparison stays about the frameworks: append
  at 100,000 went from 5.3 s to 1.8 s for us and from 3.9 s to 0.9 s for
  vanilla; swap from 282 ms to 6 ms for us and to 96 for vanilla. The
  tables below predate this; the section after them has the numbers.
- **Measure in a foreground tab.** A background tab runs everything,
  wasm included, about ten times slower; the first numbers from the
  standalone page were all wrong for that reason.
- **Hidden rows are not free.** Rows are never removed from the DOM, so a
  page that has held 100,000 rows keeps 200,000 hidden `<tr>`s, and a
  1,000-row create on it costs 205 ms instead of 65: the style pass still
  visits them. The harness reloads between runs for that reason. Real
  removal wants keyed members, the same change that makes remove O(1).

So the design is within a small constant of the DOM floor for building a
table, and with a derivative pays nothing linear for a small change. The
remaining gap to vanilla is remove, a fact about position addressing, and
the style pass, the browser's price for the numbers approach.

### Where the Rust time went, and the projection that came out of it

Natively, in release, per event at 100,000 rows (600,001 slots), before
and after the projection changed:

| phase | B-tree, insert per slot | sorted Vec, batch |
|---|---|---|
| fold (`step`) | 0.3 ms | 0.3 ms |
| checkpoint clone | 0.1 ms | 0.1 ms |
| build the projection | 182 ms | 5 ms writes + 37 ms sort and merge |
| diff, nothing changed | 225 ms | 2 ms |
| host `dispatch(select)` | 418 ms | 45 ms |

Two mistakes, neither of them the tree's fault. The diff looked every slot
up in the other map, 1.2 million tree lookups with string keys to find
zero changes; a merge walk over two sorted sequences is one comparison per
slot. And the projection was built by 600,001 single inserts of keys that
arrive nearly in order. Fixing the second the tree's way, by collecting a
sorted batch and bulk-building, linked the standard library's sort into
every bundle: the like button grew from 43.5 KB to 60.8 KB. So the
projection is now a sorted `Vec` with a pending batch, its own forty-line
stable merge sort, a slice merge for the diff, and a pointer-equality
fast path when comparing the static names in keys. Bundles are back to
size and a select is nine times cheaper. The B-tree stays where its job is
random insertion and nearest-key lookup: the checkpoint store.

What remained was the sort itself, 37 ms for 600,001 slots, because a
row's seven writes arrive in the order the `project` function makes them,
not in key order. The derivative removes that too.

### With grid rows and content-visibility, all three

Same operations, same machine, after the page change above. Totals, and
in brackets the style-and-layout share, milliseconds:

| op | N | LogFold | vanilla | React 18 |
|---|---|---|---|---|
| create | 1,000 | 32 (7) | 14 (6) | 16 (4) |
| append | 1,000 | 18 (6) | 7 (3) | 12 (3) |
| remove | 1,000 | 16 (4) | 1 (1) | 2 (1) |
| clear | 1,000 | 19 (3) | 2 (0) | 7 (0) |
| create | 10,000 | 168 (47) | 95 (42) | 329 (42) |
| update every 10th | 10,000 | 10 (8) | 1 (0) | 8 (0) |
| swap | 10,000 | 4 (4) | 7 (7) | 25 (11) |
| remove | 10,000 | 123 (27) | 7 (7) | 14 (9) |
| append | 10,000 | 165 (51) | 81 (36) | 233 (34) |
| create | 100,000 | 1,648 (553) | 990 (439) | 16,011 (450) |
| update every 10th | 100,000 | 119 (93) | 21 (0) | 207 (0) |
| select | 100,000 | 55 (53) | 62 (62) | 89 (62) |
| swap | 100,000 | 6 (5) | 96 (96) | 310 (99) |
| remove | 100,000 | 1,288 (326) | 119 (119) | 222 (115) |
| append | 100,000 | 1,784 (552) | 860 (376) | 15,495 (368) |
| clear | 100,000 | 1,743 (353) | 217 (0) | 601 (11) |

With the browser's sequential costs gone, the style share is small and
close to vanilla's, and what separates a LogFold create from a vanilla
one is the shim's write loop: about 1 ms per thousand rows, six writes a
row. Swap and select are at or under vanilla. Remove and clear remain
position addressing.

### The shim: fewer, bigger DOM operations

What separated a LogFold create from a vanilla one was the shim's write
loop: a `querySelector`-free lookup, a spelling decision and a DOM call
per write, six per row, on live nodes. Two changes, both in
`www/logfold.mjs`, nothing in Rust:

- **New members are one HTML string.** When a patch names members a
  family does not have yet, their writes are collected, each member is
  rendered as its template's markup with the classes, attributes and
  custom properties already in the tag, and the batch goes in with one
  `insertAdjacentHTML`. The parser does in C++ what six property writes
  did from JavaScript. Text slots are filled after insertion.
- **One writer per slot.** The spelling of a slot (bool class, enum
  class, attribute, variable, text) is resolved once into a function, and
  an enum class remembers the class it last put on an element, so a
  change is one `remove` and one `add`, not a sweep over every value.

DOM milliseconds, before → after: create at 100,000 rows 989 → 422,
append 1,091 → 504, create at 10,000 100 → 42. Same page, all three:

| op | N | LogFold | vanilla | React 18 |
|---|---|---|---|---|
| create | 1,000 | 25 | 22 | 20 |
| append | 1,000 | 17 | 10 | 14 |
| remove | 1,000 | 15 | 1 | 2 |
| create | 10,000 | 110 | 100 | 347 |
| append | 10,000 | 121 | 88 | 236 |
| remove | 10,000 | 106 | 7 | 13 |
| create | 100,000 | 1,110 | 1,276 | 17,837 |
| update every 10th | 100,000 | 126 | 15 | 281 |
| select | 100,000 | 60 | 226 | 110 |
| swap | 100,000 | 8 | 102 | 272 |
| remove | 100,000 | 917 | 121 | 221 |
| append | 100,000 | 1,391 | 1,070 | 15,475 |
| clear | 100,000 | 1,142 | 202 | 663 |

Create is now at vanilla's speed at every size and ahead of it at
100,000, because one parse of a 15 MB string beats 100,000 `innerHTML`
calls. Select and swap are ahead of both. What is left is position
addressing: remove, clear, and the 100 ms an update spends restyling
10,000 rows whose attribute changed.

### Keyed members

The last structural gap was position addressing: a row was its position,
so removing row 3 rewrote every row below it, clear hid every row one
class at a time, and hidden rows stayed in the DOM forever. A `keyed`
family addresses members by a key the component gives (`key = row_key`),
and a member's position becomes one more number on it, an `order` slot
the framework writes. Nothing else in the model changes: the diff of two
renders emits the removed member's clears and new order numbers for the
rows below; the derived derivative does the same from the change log,
matching members by key. On the wire, a cleared order means "dropped",
and the host sends no other clears for that member. The shim drops the
node, places members by order number lowest first (a member already in
place costs one comparison), and inserts fresh members sorted by their
order so a create or append needs no moves.

Same page, same machine, keyed against positional, milliseconds:

| op | N | positional | keyed | vanilla |
|---|---|---|---|---|
| remove | 10,000 | 106 | 43 | 6 |
| clear | 10,000 | 100 | 27 | 17 |
| remove | 100,000 | 917 | 436 | 112 |
| clear | 100,000 | 1,142 | 337 | 209 |
| swap | 100,000 | 8 | 77 | 86 |
| create | 100,000 | 1,110 | 1,319 | 881 |

Remove at 100,000 is now a hash map over the keys (125 ms in wasm),
one node removed, and 279 ms of the browser's layout, which vanilla pays
too (112). Clear is 100,000 node removals, about what vanilla's one
`textContent = ""` costs. Swap became two real node moves, so it now
pays the browser's relayout like everyone else instead of two class
writes. Create carries 100,000 order numbers more and about 200 ms of
shim work more; of its 543 ms of DOM time, 319 ms is the browser parsing
20 MB of HTML, which is the floor. And after a session the DOM holds
exactly the rows the state holds.

### The derivative, derived

The projection is a function of state, and `Projection` is a change
structure: `Vec<Change>` is its delta, `apply` is ⊕, `diff` is ⊖. So the
host was computing `project(s ⊕ e) ⊖ project(s)` from scratch on every
event. The derivative of a sum over family members is the sum over the
members that changed, so if the state can say which members changed, the
framework can take the derivative itself. A `TrackedVec` is a `Vec` that
logs its own mutations; a family declared with `project row from rows =
project_row` is drawn member by member; and after each event the host
takes the log and redraws only the touched members, diffing each against
how it was, plus the shifted range, the vanished ones, and a diff of the
small root projection. `context`/`affects` covers a slot that depends on
something outside its member. A log that says "everything changed", or
a shift over most of the family, makes the host rebuild instead, which is
always correct. `derivative_law` checks every derived answer against a
full render. `delta = f;` remains for a hand-written derivative.

Same table, same machine, wasm milliseconds per event:

| op | N | rebuild + diff | derived |
|---|---|---|---|
| select | 10,000 | 8 | 0.2 |
| select | 100,000 | 91 | 1.9 |
| swap | 100,000 | 89 | 0.7 |
| update every 10th | 100,000 | 99 | 17 |
| remove | 100,000 | 113 | rebuild (a shift over the whole table) |
| create | 100,000 | 131 | rebuild |

Selecting one of 100,000 rows is 2 ms against vanilla's 0 and React's 24,
with nothing written by hand. Two things had to change underneath for
that number. The host keeps the state at the head and advances it one
step per event instead of re-folding from a checkpoint. And the sorted
`Vec` projection keeps a cleared slot in place as a tombstone, and a
family's render reserves every slot a member could set, so a later set is
a replacement, not an insertion that moves a 38 MB store: the first
version of this select cost 8 ms in memmove.

Cost of the machinery: the generic host and projection grew by about 4 KB
of wasm (the like button is 47.2 KB from 43.2). The family algorithms sit
behind a trait object, so a component without a family compiles none of
them; what every bundle carries is the head state, `apply` with its
in-place path, tombstones and the derivative call.

## What is in a bundle

Measured with twiggy on a named build of the like app (code bytes before
wasm-opt; release builds strip the names):

| what | share |
|---|---|
| host and wasm-bindgen glue: the exported methods, `Vec<f64>` and handle marshalling, the externref table, the boxed closures of the fold chain | ~25% |
| `BTreeMap`/`BTreeSet` machinery for the projection, the checkpoints and the in-flight set | ~15% |
| logfold-core: fold, checkpoints, projection diff | ~13% |
| the allocator (`talc`, set once in `logfold-web` for every bundle) | ~10% |
| `core::fmt`, reached only from std's panic hook, which formats the message and the source location before aborting; a floor on stable Rust | ~8% |
| read-only data and drop glue | ~12% |
| the like button itself | ~2% |

So a bundle is mostly a fixed cost of about 40 KB raw, 14 KB brotli, and
the app rides on top. The studio, a real app, is 94 KB raw, 30 KB brotli,
a third of it Brunhilda's planner. Devtools methods are exported by a
separate `export_devtools!` line and cost under a kilobyte. A sorted
vector in place of the B-trees would take another 5 to 12 KB off, and was
declined: we like B-trees.

## What this does not decide

- **Animations as values** (proposal §4.5) are not needed for anything a CSS
  transition can express, which is everything the like button and Brunhilda
  do. Sampled trajectories can come back if a host needs them.
- **Effects.** Network requests are not outputs and do not go through the
  patch. The networked like button adds an effects stream and `io(req,
  result)` back, with the same discipline: numbers and handles.
- **Canvas.** A canvas is not a skeleton. Brunhilda's first host drew one;
  the studio replaced it with 96 projected cells and the canvas host was
  removed. If a real canvas is ever needed, it is a bespoke output drawn
  from the same numbers.
- **Browser support.** Style container queries and `@property` are 2023 to
  2025 features; verified in the user's Chrome, not yet in Firefox or Safari.
  The fallback is a `data-liked` attribute selected by `[data-liked="1"]`,
  which is the same design with older syntax.
- **Rust as the shim.** The shim is a dozen lines of JavaScript. It could be
  Rust through `web-sys`, at which point the page is HTML, CSS and `init()`.
  Not worth it until there is a reason to remove the JavaScript entirely.
