# LogFold

An event-log runtime: the UI is `render(fold(log))`, effects are values,
time is an event. Design document: [`docs/logfold-proposal.md`](docs/logfold-proposal.md).

## Layout

The framework:

- `crates/logfold-core` — the runtime: log, event model, folds as values with checkpoints, effects as a diffed projection, expectations, the `slots!` and `component!` declarations.
- `crates/logfold-web` — the generic browser host and the export macros (`export_component!`, `export_devtools!`, `export_raw!`). A library; it exports nothing itself.
- `www/logfold.mjs`, `www/timeline.mjs`, `www/logfold-raw.mjs` — the shim every page mounts, the devtools, the raw loader. Generic; they know no component.

The examples:

- `examples/like-local` — the whole idea in one file: a toggle. Start here, with [`docs/guide.md`](docs/guide.md).
- `examples/counter` — the second component: three inputs, one number, CSS renders text and a bar from it.
- `examples/like-button` — the networked like: a request, an honest host, a server model and an adversarial fuzz harness. No page.
- `examples/brunhilda` — a robot vacuum cleaner as a fold, a simulated room as a host, and a fuzzer that plays the human she keeps attacking.
- `examples/todo` — a todo list being built step by step; see its module doc for the order. First slice: Enter adds an item with the typed text, without a string crossing the boundary.
- `examples/bench` — the js-framework-benchmark table (create, update, select, swap, remove, append, clear) with an in-page harness that sweeps N = 10, 100, 1 000, … and stops at a time gap; `www/bench-vs.html` runs the same operations against vanilla JS and React 18 in the same tab. Numbers in `docs/boundary.md`.
- `examples/studio` — the real app: her brain, her world and the control panel as one component on one log, rendered by 96 projected cells.
- `examples/apps/*` — one tiny crate per bundle (`like-app`, `counter-app`, `studio-app`, and `like-raw` through the raw boundary).
- `www/*.html` — the pages; `www/gen/` the generated contracts; `www/pkg/` the built bundles.

How to build a component: [`docs/guide.md`](docs/guide.md). Benchmark results: [`docs/benchmark.md`](docs/benchmark.md). Why the boundary looks like this, with measurements: [`docs/boundary.md`](docs/boundary.md). Design record: [`docs/logfold-proposal.md`](docs/logfold-proposal.md). Deferred work: [`docs/maybe_todo_someday.md`](docs/maybe_todo_someday.md).

## Run

```sh
cargo test --workspace
```

## Browser demo

```sh
cargo install wasm-pack            # once
./scripts/build-www.sh             # every app bundle into www/pkg/<app>/
scripts/serve.py            # www/ on http://127.0.0.1:8765 with no-cache headers, so a reload is a reload
# open http://127.0.0.1:8765/index.html      (the like button)
# open http://127.0.0.1:8765/like-raw.html   (the same button, raw boundary, no wasm-bindgen)
# open http://127.0.0.1:8765/counter.html    (the counter)
# open http://127.0.0.1:8765/studio.html     (Brunhilda's studio: the real app)
# open http://127.0.0.1:8765/todo.html       (the todo list)
# open http://127.0.0.1:8765/bench.html      (the benchmark; press "run sweep")
# open http://127.0.0.1:8765/bench-vs.html   (the same operations vs vanilla JS and React 18)
```

Bundle sizes after wasm-opt, brotli in brackets: like 43 KB (15) plus 8 KB (2) of generated glue, like-raw 39 KB (14) with no glue file, counter 43 KB (15), studio 94 KB (30). The previous single bundle carrying everything was 168 KB (48). What is in them and why is in `docs/boundary.md`.

The shim in `www/index.html` appends an event per click, writes the view
into the DOM, and asks for the view at any index when the slider moves.
Checkpoints are taken roughly every 8 events, decided by the host, and the provenance line
shows which one the scrubber resumed from.

## Status

M0:

- [x] Log; views with absolute indices (`prefix`, `suffix`, `slice`); virtual `now()` (itself a fold)
- [x] `Fold` as a value (`foldl` style): `run`, `scan`, `map`, `zip`, `scoped`; a checkpoint is `fold.from(state, upto)`; `checkpoint_law` checks resume == run at every split; ordered `Checkpoints` store with nearest-at-or-before lookup
- [x] Expectations built from a fold + predicate (`Expectation::on`): all-prefix mode is one `scan`; `raw` escape hatch for non-fold properties
- [x] Events split by origin: `Tick` and `Input` (pure), `Sense` and `Io` (world), `Started` (host bookkeeping); `Log::inputs()` seeds re-execution (not yet exercised)
- [x] Effects as a level-triggered desired-set projection; `in_flight` fold (started − answered; result-less effects stay as the record); the host diff runs both ways: start = desired − in-flight, cancel = in-flight − desired
- [x] Expectations: guard mode and all-prefix (fuzz) mode
- [x] Like-button fold + benign proptest harness
- [x] Adversarial generators (out-of-order answers, `Failed`/`Cancelled` mid-flight, duplicate and bogus answers, clock jumps)
- [x] Client/server agreement expectation (the test plays the server as a fold)
- [x] **Exit criterion met.** The fuzzer broke the naive one-request-per-click fold with a 9-event log (three concurrent requests, answered out of order) and shrank it. The fold now coalesces clicks into one in-flight request; the steps that produced the log replay as a regression test (the log itself cannot, since its `Started` events were the old host's output).

Since M0:

- [x] Browser host: `like-local` in WASM with a scrubbable history (`crates/logfold-web`)
- [x] Events are domain-parametric (`Domain` trait: `Input`, `Sense`, `Effect`); `Sense` is unsolicited world input; effects live in each domain
- [x] Second host: Brunhilda. Time drives her, a heading is an *output* the sim reads every frame, an e-stop is a fire-and-forget *effect* the host latches. Three fold-based expectations on every prefix (never in the human's cell, frozen after e-stop, coverage monotone), the checkpoint law, and three host checks at every frame boundary (her dead-reckoned position equals the sim's, nothing left to start, attacks in the log equal attacks the sim counted).
- [x] **Exit criterion met, twice.** The fuzzer breaks the naive policy (drives adjacent to you, you step into her path). It also broke the *careful* policy twice before it held: `Dock` after an e-stop resumed her brain while the host latch still held, and `Start` moved her before she had sensed anything, running over anyone standing by the dock.

- [x] Brunhilda plans: nearest uncleaned cell by breadth-first search over the map she knows, docks when the job is done, waits when the only cells left are the ring around you. The sim checkpoints her brain and the in-flight set together, so each frame costs the events since the last one.
- [x] `component!`: one declaration generates the domain marker, the `Input` enum, `Ev`, `INPUTS` and `component()` with the manifest attached. The like button went from eleven items to four: the declaration, the state, the step, the projection. The counter likewise; the studio stays on the explicit builder for now.
- [x] The contract, declared once: `logfold_core::slots!` declares a component's targets, slots with types, a family of repeated targets, inputs and constants, and expands to typed Rust constants plus a `Manifest`. The bundle's build writes the page's side from it: `@property` registrations and `:root` constants as CSS, names and value tables as an ES module. Booleans and enums are classes, so the studio's stylesheet reads `html.mode-cleaning` and `html.running`; the app crate's `build.rs` writes the fragments on every build, so they are as fresh as the bundle. The studio's projection is typed slots end to end and its page has no hand-written registrations.
- [x] Bundle floor, measured: toolchain knobs are spent (all wasm-opt passes together half a kilobyte, now on for every bundle; lower opt-levels compress worse), the remaining honest target is about 33 KB raw by going `no_std` and swapping the allocator, and the rest is the design and the B-trees. Details in `docs/boundary.md`.
- [x] The raw boundary: `export_raw!` puts the same host behind plain `extern "C"` exports, numbers only, names as bytes decoded once, patches read in place as a `Float64Array` view. The host library's wasm-bindgen surface became a feature (`bindgen`, default on); a raw app turns it off and links no wasm-bindgen at all. Same page, same shim, 13.6 KB over the wire instead of 17.1. Measured: wasm-bindgen's share of the wasm was 4 KB; the generated JS was the other 2 KB brotli.
- [x] Bundle diet, measured with twiggy: the allocator is `talc` (4 KB instead of dlmalloc's 8), no integer formatting or `format!` on library paths, and the devtools methods are a separate `export_devtools!` so a page without a timeline drops them (0.8 KB). The `core::fmt` that remains is std's panic hook, a floor on stable Rust. B-trees stay because we like them.
- [x] Every app is its own bundle: `examples/apps/<name>-app` is one `export_component!` line over the `logfold-web` host library, built by `scripts/build-www.sh` into `www/pkg/<app>/`. The like page loads 16 KB brotli instead of 48.
- [x] **The studio.** Two applications of the framework in one app on one log: the panel (run, policy, dropouts, rate), Brunhilda's brain unchanged, and the world the sim used to keep in memory (you, the latch, the counts), all one fold, because your arrow keys are inputs too. A component can now declare its effects and a simulated world; the host gained `frame(dt)` and records `Started` after every append. The page is 96 cells and three dots positioned by variables; CSS transitions do the gliding; the slider replays the whole session, you included. No canvas.
- [x] Benchmarked at scale with the js-framework-benchmark operations, N = 10 to 100,000: at 1,000 rows every operation is under 70 ms; at 100,000 the worst is append at 8.3 s, and 1,000,000 is projected past a 10 s gap. Against vanilla JS and React 18 on the same machine: creating rows costs 2 to 3× vanilla and about 2× React (and beats React at 100,000), every small operation pays a linear full-projection tax the others do not (91 ms to select one of 100,000 rows), and removing a row is 20 to 30× vanilla because rows are addressed by position. Style and layout are the largest share of a create. Two shim fixes came out of it: targets are indexed once instead of found by `querySelector` per write, and a grown family is inserted as one fragment.
- [x] Keyed families: `family row keyed` plus `key = row_key` addresses members by identity; a member's position is an `order` slot the framework writes, a cleared order drops the node, and the shim places nodes by order number. Remove at 100,000 rows 917 → 436 ms, clear 1,142 → 337, and no dead rows remain in the DOM. Positional families are unchanged.
- [x] The shim inserts a family's new members as one HTML string with their first writes already in the tags, and resolves each slot's spelling into one writer. DOM time for creating 100,000 rows went from 989 ms to 422; create is now at vanilla's speed at every size and ahead of it at 100,000. Nothing moved to Rust: the DOM calls have to be made from JavaScript either way, so the win is making fewer, bigger ones.
- [x] Page-side layout findings from the benchmark, recorded in `docs/boundary.md`: CSS counters and automatic table layout are both sequential, so one changed row restyles or reflows every row after it; numbers rendered with `attr()` and rows as grid rows with `content-visibility: auto` remove both. Append at 100,000 rows went from 5.3 s to 1.8 s (vanilla 3.9 s to 0.9 s, same page); swap from 282 ms to 6 ms.
- [x] The projection's derivative, derived. A large family keeps its members in a `TrackedVec`, a `Vec` that logs its own mutations, and is declared `project row from rows = project_row;` with a function from one member to its slots; from the log the framework redraws only the touched members and diffs each against how it was, so the host never rebuilds the page for a small change. `derivative_law` checks every derived answer against a full render; `delta = f;` remains for a hand-written one. Selecting one of 100,000 rows: 91 → 2 ms in wasm, swap 89 → 0.7, update 99 → 17, nothing written by hand. Along the way: the host keeps the head state instead of re-folding, and cleared slots stay as tombstones so a set never moves the store.
- [x] The projection is a sorted `Vec`, not a B-tree. The benchmark's per-event Rust cost was a diff doing 1.2 million tree lookups to find nothing and a projection built by 600,001 single inserts; a merge diff and a batched build cut a select at 100,000 rows from 418 ms to 45 ms natively (797 to 91 in the browser). Bulk-building the tree instead would have linked the standard library's sort, 17 KB in every bundle, so the projection carries its own small merge sort. B-trees stay for the checkpoint store, whose job they fit.
- [x] Inputs from inside a family member carry the member's index: `toggle: index => Toggle` is `Toggle(u32)`, and a checkbox inside `item-3` says "row 3". The checkbox's native toggle is cancelled; the tick is drawn from the row's `done` class, so scrubbing unticks it. Families without a count grow from a `<template>` on first use.
- [x] Text, still as numbers: an input declared `add: text => Add` carries what was typed into the log as its payload; a `text` slot's number is the log index of that input, and the shim fetches the text by index into the target's `[data-text]` child. The raw boundary gets it as bytes through a scratch buffer both ways. `data-on="enter:add"` on a field fires on Enter and empties it. The todo list is the example.
- [x] Slots come in three spellings, one number each: a CSS custom property (`calc()`, transitions, style queries), an attribute (typed `attr()` and value selectors like `[data-count="0"]`), or a class for booleans and enums (`html.liked`, `html.mode-cleaning`). Measured in Chrome: restyling 5,000 rows off a root class is 1.7× faster than off a root attribute for a boolean and 3.6× for an enum switch, so state is classes and numbers are attributes. The counter bridges `data-count` and lights its bar with `sibling-index()`.
- [x] Adding a component is three things you write (state, step, projection) plus a declaration; one `export_component!` line in an app crate; a skeleton page that imports `logfold.mjs` and calls `mount`. The counter renders its number as text with a CSS counter and lights a bar with arithmetic on the same attribute.
- [x] The boundary, designed and built for the like button: fold state projects to numbers on named targets (`logfold_core::project`); the host writes CSS custom properties and the stylesheet renders. The DOM is `project(fold(log[..at]))`; scrubbing and rendering are one map diff. The page has no like-specific JavaScript. See `docs/boundary.md`.
- [x] Review pass: three bugs fixed (an unincremented bump counter, a debug-only skip check in `Fold::zip`, an edge-triggered desired set), `Started` lost its redundant request id, checkpoints resume from the store everywhere. The design doc's section 11 records every decision since v0.1.
- [x] Brunhilda in the browser, first as a hand-written canvas host with a second canvas replaying what she knew. Superseded by the studio and removed.

Next: the networked like button in the browser against a fake server; the timeout ambiguity (`Failed` then `Done`) and a resync effect.

Deferred with reasons: [`docs/maybe_todo_someday.md`](docs/maybe_todo_someday.md).
