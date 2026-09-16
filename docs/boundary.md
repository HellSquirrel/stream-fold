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
| Rust → JS | patch | `Float64Array` of `[target, index, kind, name, value]`; kind 0 = custom property, 1 = attribute, 2 = text; `NaN` = clear |
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
    Projection::new().attr("root", "data-liked", u8::from(v.liked))
}
pub const INPUTS: &[(&str, Click)] = &[("toggle", Click)];
```

The attribute lives on the root so that `html[data-liked="1"] …` can select
anything on the page. The transition on `scale` means the button animates
on every click and on every scrub. Nothing in Rust knows there is a heart.

## Two ways a number can land: a variable or an attribute

A slot is either a CSS custom property on the target (`--count`) or an
attribute (`data-count`). Same number, same diff, same host loop; the
projection chooses per slot and the shim calls `setProperty` or
`setAttribute`. They differ in what the stylesheet can do with them:

| | custom property | attribute |
|---|---|---|
| inherits down the tree | yes | no |
| usable in `calc()` and transitions | yes | through typed `attr()` (Chrome 133+) |
| conditional rules | `@container style()` (2023+) | attribute selectors, every browser |
| visible in the markup and serialisable | no | yes |

The bridge is one declaration: `html { --count: attr(data-count
type(<integer>), 0); }` turns an attribute on the root into an inherited
variable, so both worlds are available from one write. The like button uses
the attribute alone with `html[data-liked="1"] …` selectors, which works in
every browser. The counter writes `data-count`, bridges it, selects on it
for the zero state, and lights its bar with `sibling-index()` so the cells
carry no per-cell markup. Both were verified in Chrome 152.

There is no reverse bridge: nothing on the platform sets an attribute from a
style. State flows Rust → attribute or variable → stylesheet, never back.

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
   from the row's `data-done` attribute.

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
        attr running: bool;                                   // data-running, present or absent
        attr mode: enum { idle, cleaning, docking, stopped };  // data-mode="cleaning"
        var fps: int = 4;                                      // --fps, @property <integer>, initial 4
    }
    family cell((brunhilda::W * brunhilda::H)) { attr cleaned: bool; }   // targets cell-0 … cell-95
    inputs { run, pause, start, dock, estop, north, east, south, west }
    consts { room_w: brunhilda::W, room_h: brunhilda::H, cell_px: 40 }
}
```

It expands to typed constants, `ui::mode.slot()`, `ui::mode::cleaning`,
`ui::cell::cleaned.at(i)`, and a `Manifest`. Names are the identifiers as
written, so what you read in Rust is what you write in CSS.

From the manifest, `cargo xtask gen` writes the page's side into `www/gen/`:

- `<app>.css`: a typed `@property` per variable and the constants on
  `:root`. Appearance is never generated; the stylesheet stays the
  designer's. The studio's room is `repeat(var(--room_w), calc(var(--cell_px) * 1px))`.
- `<app>.manifest.mjs`: names by id, inputs in dispatch order, families
  and their sizes, constants, and how each slot's number is spelled.

The host interns the manifest's names first, so ids are fixed by the
declaration and the shim resolves them without asking. The shim spells a
boolean attribute as present or absent and an enum attribute by its value's
name, so the studio's stylesheet reads `html[data-mode="cleaning"]` and
`html[data-running]` rather than `="1"`. Families cross as a target id plus
an index, and the page names its cells `cell-<i>`.

Two tests guard it: the component's inputs must equal the manifest's, in
order (the host asserts it), and the generated fragments must equal what
the manifest produces now (`the_generated_fragments_are_current`), so a
stale page fails `cargo test` with "run `cargo xtask gen`".

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
