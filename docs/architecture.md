# LogFold, as built

The current shape of the runtime, what lives where, and every decision
that got it here with the reason. `docs/logfold-proposal.md` is the
original design and its dated decision log; `docs/boundary.md` is the
long argument about the browser boundary with its measurements;
`docs/guide.md` is how to write a component; `docs/benchmark.md` is the
numbers. This is the map.

## One paragraph

Everything that happens is an event appended to a log. State is a fold
over the log: an initial value and a pure step. The page is a projection
of state, a flat map of numbers onto named targets, and the browser's own
stylesheet turns those numbers into appearance. The host owns the log,
folds it, projects it, diffs the projection against the one the page
holds, and sends the difference as numbers. Moving the page to any point
in history is the same diff. Effects are values the fold wants in flight;
the host records what it starts. Time is an event like any other.

## The picture

```
 page (HTML skeleton, CSS)                 wasm module (one per app)
 ──────────────────────────                ─────────────────────────────────────
 click / keydown / enter ──► shim ──dispatch(id, index, bytes)──► Host
                                                                   │ event_for → Event
                                                                   │ append: log.push, head = step(head, ev), checkpoint every 8
                                                                   │ act: effects(state) − in_flight → Started events
                                                                   │ derive (family logs) ── or ── render(head) → diff(current, new)
                                                                   ▼
 DOM: classes, attrs, vars,  ◄── shim.apply ◄── patch: Float64Array of [target, index, kind, name, value]
 text, node order; CSS renders
 timeline ──render_at(n)──────────────────────────────────────────► Host: projection at n from a checkpoint, diff
```

Nothing in the wasm module knows what a target looks like; nothing in the
page knows what an input means. Names cross once, as interned ids.

## Where things live

### Repository

| path | what |
|---|---|
| `crates/logfold-core` | the runtime, no browser: `log` (append-only log and views by absolute index), `event` (`Event<D>`, `Domain`, origins), `fold` (`Fold` as a value, `Checkpoints`, `checkpoint_law`, `now`, `in_flight`), `effect` (`diff_effects`, `IdemKey`), `expect` (expectations as fold + predicate), `project` (`Slot`, `Projection`, `diff`, `apply`, the name interner), `tracked` (`TrackedVec`), `slots` (`slots!`, `Manifest`, `Declared`), `component` (`Component`, `component!`, families, `derive`, `derivative_law`) |
| `crates/logfold-web` | the generic browser host: `Host` (log, head state, checkpoints, the projection the DOM holds, dispatch/tick/frame/render_at, the wire encoding), `export_component!` and `export_devtools!` (wasm-bindgen classes), `raw` and `export_raw!` (the same host behind `extern "C"`) |
| `www/logfold.mjs` | the shim: `mount(app, { manifest })`; resolves targets, applies patches, grows families from templates, fires inputs from `data-on` |
| `www/timeline.mjs`, `www/logfold-raw.mjs` | the devtools scrubber; the loader for a raw module |
| `examples/*` | components: `like-local`, `counter`, `todo`, `bench`, `studio` (with `brunhilda`), `like-button` (networked, no page) |
| `examples/apps/*` | one two-line crate per bundle; its `build.rs` writes `www/gen/<app>.css` and `<app>.manifest.mjs` from the manifest |
| `www/*.html`, `www/gen/`, `www/pkg/` | the pages; the generated contracts; the built bundles (ignored) |
| `scripts/build-www.sh`, `scripts/serve.py` | build every bundle with wasm-pack and wasm-opt; serve `www/` without caching |
| `docs/results/` | raw benchmark results and the disposable glue that produced the official ones |

### At run time, inside the wasm module

| what | where | lifetime and size |
|---|---|---|
| the log | `Host.log: Log<Event<D>>`, a `Vec` with a `base` | append-only, at most 65,536 events behind the head: past that the horizon moves to the checkpoint nearest to half the budget back (`Log::advance`), so it moves once per 32,768 events; indices stay absolute, the slider starts at `base`; an event is a few dozen bytes plus any text it carries |
| the state at the head | `Host.head`, advanced one step per append | one copy; never re-folded from a checkpoint |
| checkpoints | `Host.checkpoints: Checkpoints<(X, in_flight)>`, a `BTreeMap` by log index | one clone of the state every 8 events, at most 64 kept: over budget the older half is thinned by half (`Checkpoints::thin`), so the newest stay 8 apart and history gets sparser with age back to the horizon; scrubbing resumes from the nearest one at or before the index and folds the rest |
| forgotten text | `Host.texts: Vec<(Index, Box<str>)>` | the text of every text input before the horizon, so a `text` slot's index still answers after the event is gone; bounded by what was typed |
| the projection the DOM holds | `Host.current: Projection`, a sorted `Vec<(Slot, f64)>` | one; 24 bytes a slot; cleared slots stay as tombstones until the next full build |
| a member's change log | inside the state, in each `TrackedVec` | drained by the host after every event |
| the name table | a process-wide `Vec<&'static str>` behind a mutex | one per module; ids are two bytes and are what cross the boundary |
| text a person typed | in the log, as the input event's payload | for the page's life; a `text` slot points at it by log index |
| in-flight effects | derived by the `in_flight` fold, checkpointed with the state | started minus answered |

### In the page

| what | where |
|---|---|
| the skeleton | HTML the page author wrote: targets as `data-fold`, inputs as `data-on`, one `<template data-fold="fam">` per grown family |
| appearance | the page's stylesheet, never generated; `www/gen/<app>.css` adds only `@property` registrations and constants |
| what the host wrote | on the elements: classes, `data-*` attributes, custom properties, text nodes, node order |
| DOM-owned state | only form fields the host never writes (a text field before Enter) and the shim's per-family member maps |
| the log, persisted | nowhere yet; reload and it is gone (deferred, see `docs/maybe_todo_someday.md`) |

## The loop, one input

1. The shim finds the input's id, the family member index or key it was
   fired from (from the nearest `data-fold`), and the field's value if it
   was fired from a form field, and calls `dispatch(id, index, bytes)`.
2. `Component::event_for` turns that into an `Event`: a unit variant, a
   text-carrying variant decoded from the bytes, or an index-carrying
   variant.
3. `Host::append` pushes the event, steps the head state, and takes a
   checkpoint if one is due. Past 65,536 events behind the head it moves
   the horizon: the checkpoint nearest to half that is promoted, the log
   and the store forget what came before it, and the text of forgotten
   inputs moves to `Host.texts`.
4. `Host::act` asks the component which effects should be in flight,
   diffs that against what the `in_flight` fold says has started, and
   appends a `Started` per new one. Performing them is the page's business.
5. If the DOM shows the head and nothing was started, `Component::derive`
   asks each family for its change log and, matching members by position
   or key, produces exactly the writes that event caused; a hand-written
   `delta` takes precedence if there is one. Otherwise, or when a log says
   everything changed, `render` builds the whole projection and `diff`
   finds the writes.
6. `encode` turns writes into a `Float64Array` of five numbers each; a
   keyed family that emptied becomes one "all gone" entry.
7. `shim.apply` walks it: new family members are rendered as one HTML
   string and inserted once; other writes go through one prepared writer
   per slot; drops and moves settle last, by order number.

Scrubbing is step 5's rebuild path at another index, from a checkpoint.

## Decisions, and why

Each is a fact about the code; the pointer is where the argument lives.

**The model**

- **Append-only log; events by origin.** `Tick` and `Input` are pure;
  `Sense` and `Io` come from the world; `Started` is host bookkeeping.
  Replay without the world is a filter on origin. (`event.rs`)
- **Folds are values.** `Fold::new(init, step)` with `run`, `scan`, `map`,
  `zip`, `scoped`; a checkpoint is `fold.from(state, upto)`, the same
  fold with a different start, and `checkpoint_law` proves resuming
  equals folding from zero at every split. (`fold.rs`)
- **Checkpoints are a B-tree by index, policy in the host.** Random
  insertion and nearest-key lookup are the whole job; the core never
  decides when to take one. The host takes one every eight events.
- **Effects are a level-triggered projection, not a stream.** The fold
  says what should be in flight; the host starts the difference and logs
  `Started` first. Fire-and-forget effects stay in `in_flight` as the
  record that they happened. (`effect.rs`, proposal §11 c)
- **Outputs are not effects.** The DOM and a motor heading are idempotent
  functions of state, diffed against the world, never logged. Nothing
  about rendering is in the log. (proposal §11 e)
- **Time is the host's.** The page owns the clock and calls `tick` or
  `frame`; the fold only ever sees ticks. A simulated world is a function
  from state and a frame length to senses.

**The boundary**

- **Numbers only, and CSS renders.** No virtual DOM, no tree diff: a
  projection is a flat map from slots to numbers, the host diffs two of
  them, the stylesheet does everything visible. Scrubbing and rendering
  are the same operation. (`docs/boundary.md`, "The one invariant")
- **Three spellings for a number, plus text.** A class for state (the
  cheapest selector to match and invalidate, measured 1.7 to 3.6× over
  attributes), an attribute for a number the stylesheet reads with typed
  `attr()` or selects by value, a custom property for `calc()`. A text
  slot's number is a log index. (`docs/boundary.md`, "Three ways")
- **Text crosses once, into the log, as an input's payload.** A `text`
  slot points back at it by log index; the page asks the host for the
  string. Nothing textual is DOM-owned state, and replay shows the right
  words. Labels and glyphs never cross at all; they are in the stylesheet.
- **Inputs carry what the skeleton knows.** A member's index or key from
  the nearest `data-fold`; a field's value; nothing else.
- **Names are interned ids.** Two bytes a name, twelve bytes a slot, and
  the ids cross as-is; the shim asks the app once per id for the string.
  The component interns its manifest's names first, so a fresh module's
  ids are the manifest's indices.
- **One module per app, and a raw variant.** Each bundle is its own wasm
  with its own glue; wasm-bindgen is a cargo feature of the host, and
  `export_raw!` puts the same host behind `extern "C"` with names as bytes
  in linear memory. Talc for the allocator, no formatting on library paths,
  devtools methods in a separate export. (`docs/boundary.md`, "The floor")

**Structure on the page**

- **Bounded structure is pre-rendered**; a family with a count is N
  targets that always exist, toggled by numbers.
- **Unbounded structure grows from a `<template>`**, one flat member each,
  created as one HTML string with the first writes in the tags.
- **Positional families hide; keyed families remove.** A positional
  member is its index, never removed, hidden by CSS. A `keyed` member is
  a key the component gives, carries its position as an `order` slot the
  framework writes, and the page drops and moves real nodes; a family
  that empties is one instruction. (`docs/boundary.md`, "Keyed members")
- **Nothing diffs a tree.** Arbitrary nesting is not supported until an
  example needs it.

**The contract**

- **One declaration.** `component!` declares inputs (unit, `: text`,
  `: index`), root slots, families, constants, and generates the domain
  marker, the `Input` enum, `Ev`, the manifest and `component()`; the
  author writes the state, the step and the projection.
- **The page's side is generated by the bundle's build**, `build.rs`
  writing `www/gen/<app>.css` (typed `@property` registrations,
  constants) and `<app>.manifest.mjs` (names, inputs, families, value
  tables). Appearance is never generated. The fragments cannot be staler
  than the bundle, so no component carries a test about pages.

**Performance, in the order it was found**

- **The projection is a sorted `Vec`, not a B-tree.** Built from a batch
  the families emit already sorted (the first member's slot order,
  learned once), merged linearly, diffed by a merge walk; cleared slots
  are tombstones so a set never moves the store; small batches settle in
  place. A B-tree bulk build would have linked the standard library's
  sort, 17 KB, into every bundle. B-trees stay for checkpoints.
  (`docs/boundary.md`, "Where the Rust time went")
- **The derivative is derived.** A family's members live in a
  `TrackedVec` that logs its own mutations, so `step` does not change;
  from the log and the per-member projection the framework redraws only
  touched, shifted, vanished or context-affected members, each diffed
  against how it was. `derivative_law` checks every answer against a full
  render. Selecting one of 100,000 rows went from 825 ms to 2.
  (`docs/boundary.md`, "The derivative, derived")
- **The shim makes fewer, bigger DOM operations.** Targets indexed once;
  one writer per slot; new members as one `insertAdjacentHTML`; enum
  classes remembered per element; moves settled by order number lowest
  first, with a detach-and-reinsert fallback that is always right.
- **The page is part of the design.** CSS counters and automatic table
  layout are sequential and were the largest cost at scale; per-row
  numbers are attributes shown with `attr()`, rows are grid rows with
  `content-visibility: auto`. (`docs/guide.md`, "Rules for a page")

**Method**

- **Measure before deciding, in the user's browser, in a foreground tab.**
  Every claim in the docs has a number next to it; a background tab
  runs ten times slower and produced one set of wrong numbers before that
  was known.
- **Benchmarks get glue, not framework changes.** The official
  js-framework-benchmark driver runs the bench component through a
  disposable page that composes text from numbers; nothing in `crates/`
  or the shim changed for it, and CSS-based wins stay whether or not a
  harness can see them. (`docs/results/js-framework-benchmark/`)
- **Laws over assertions.** `checkpoint_law` and `derivative_law` are the
  tests that keep the two incremental paths honest against the plain
  ones; the fuzzers found every safety bug in Brunhilda before a page did.

## Where it stands

| | |
|---|---|
| tests | 72, native, plus the browser checks in this log |
| bundles, brotli | like 18 KB, counter 18, todo 20, bench 27, studio 36, like-raw 17 |
| official js-framework-benchmark, this machine | within 10 to 15% of vanilla on every CPU benchmark, ahead of React 19 on all, at vanilla's speed on select, swap and clear |
| the reference table at 100,000 rows, in-house | create 968 ms (vanilla 912), select 66, swap 81, clear 231 |

Open items are in `docs/maybe_todo_someday.md`: persisting the log,
div-based list benchmarks, a `no_std` floor, and the rest.
