# Maybe, someday

Things the review pass (Sept 2026) found worth doing but not worth doing
now, plus hazards noted along the way. Each entry says where it lives and
what would make it worth picking up. None of these blocks the next
milestone. Function names are used instead of line numbers; those drift.

## Deferred review options

### Brunhilda's planner gives up when you merely stand next to her

`examples/brunhilda/src/lib.rs`, the `blocked` update in `step` and `Policy::plan`.

- **Symptom.** With most of the room dirty, a human stepping adjacent to
  her sends her to the dock for `RETRY_TICKS` ticks. Visible in
  `www/studio.html`; the fuzzer does not check progress, so it never
  complains.
- **Cause.** The give-up test asks whether any uncleaned cell is reachable
  by a search that treats her *own* cell as impassable when the human is
  within one cell. From inside the ring nothing is reachable, so `blocked`
  is set vacuously.
- **Fix.** Set `blocked` only when every uncleaned cell other than her own
  is itself forbidden; and let the search pass through her own cell
  (`through = |c| c == pos || passable(c)`) so she heads for work rather
  than the dock while standing inside a ring. Candidate moves stay
  filtered by the fresh ring, so safety is unchanged. Add a unit test
  "human adjacent with the room mostly dirty does not set blocked". Rerun
  `careful_survives` at 2000 cases and `covers_the_room_then_docks`.
- **While the file is open.** Add `Dir::clockwise_from(self) -> [Dir; 4]`
  and use it in both `wander` and `plan` instead of the copied sweep;
  delete `setpoint` (both hosts read `.heading`) and `Cell::manhattan`
  (no caller). Do not derive `Default` on `Brain`, `Cell` or `Mode`: a new
  field would silently get zero.

### `Expectation::transition` combinator

`crates/logfold-core/src/expect.rs`.

- **Why.** Three hand-rolled "sticky verdict" judges are the same skeleton:
  `started_only_when_desired` in the like-button harness,
  `stops_after_estop` and `coverage_monotone` in Brunhilda's. Grep for
  `Ok::<(), String>(())` to find them. The two harnesses build them
  inconsistently, and only core can make guard mode report the true
  breach index instead of the log end.
- **Shape.** `Expectation::transition(name, fold, pred)` where `pred`
  sees `(index, &event, &before, &after)`, built from the public
  `init`/`step`/`done`/`skip` accessors so `Fold`'s fields stay private,
  carrying the verdict as `Result<(), (Index, String)>`. Unit test:
  all-prefix mode and guard mode agree on the index.
- **When.** Before writing the resync and timeout expectations for the
  networked like button, which would be the fourth copy. Porting the three
  existing judges is optional; they can stay until the combinator
  reproduces their breach indices under the fuzzer.

## Riders: under ten lines each, pick up when the file is already open

- `Checkpoints::truncate_before(n)`, the mirror of `truncate_after`, when
  the flight-recorder window (proposal §4.9) needs it.
- A `Log::inputs()` round-trip test, so "seeds re-execution" is exercised
  rather than claimed. About thirty lines.
- `Fold::done` and `LogView::last` have no callers outside core; delete
  when tidying.
- `Brain::fresh() -> Option<Cell>` and `forbidden(target, fresh)` instead
  of matching on `Sighting.age == 0` in three places.
- Brunhilda's fuzz harness: build the human-start cells from
  `room.free_cells()` rather than a nested range.
- `RETRY_TICKS` as a private `Policy` constant rather than a crate-level one.
- A dozen one-line docs in `logfold-core` only: `Event` and `Origin`
  variants, `Event::origin`, `Log`, `Mode` variants, `Expectation`. Not
  `#![warn(missing_docs)]`: it would bury the document-the-non-obvious
  discipline under `len`/`is_empty`/`new` ceremony.
- `[profile.test] opt-level = 2` if debug fuzz wall time ever matters
  again (it went from ~20 s to ~2.5 s after the harness rewrite, so it
  does not yet).
- Assert `pos == room.dock` at every `blocked` Some-to-None transition
  over a scan, as a tighter version of the "waits at the dock" check.

## Not doing, with the reason, so nobody re-litigates them

- **A `Host` abstraction or a shared test-support crate.** One production
  host loop, one room shape, two pages. The rule of three is not met.
  Revisit when the networked like button in the browser is the third
  concrete host.
- **Splitting `brunhilda/src/lib.rs` into modules.** `Brain`, `Policy`
  and `step` are mutually recursive through private fields; a split widens
  them to `pub(crate)`. The example crates are single-file narratives by
  convention. Section banners are enough.
- **Struct-based WASM snapshots instead of `Int32Array`.** Trades a
  lifetime-free array for `free()`-owned objects and contradicts the
  stated boundary rule (numbers cross as numbers).
- **A `Policy::on_tick` hook.** The duplicated search per tick is removed
  by the planner fix above; the hook adds ordering traps for ~15% of a
  debug-only fuzz run.
- **Room width and height as fields.** Every room is `Room::default()`,
  and the change adds a heap allocation per search.
- **Invariant-based budgets in the coverage test.** Only one assertion is
  tight, and the proposed formulas are magic numbers in disguise.

## Hazards noted while building, not yet modelled

- **Clock jumps do nothing to Brunhilda.** The sim moves her one cell per
  frame regardless of `dt`, so "two cells of movement on one stale
  sighting" cannot happen yet. Modelling `dt`-scaled movement would make
  the careful policy's speed limit a real decision (the setpoint would
  need a distance bound the on-robot loop obeys).
- **The human never steps onto her cell.** "Attack" means she moved into
  you or stayed on you, never the reverse. A modelling choice; say so if
  anyone reads the numbers as symmetric.
- **The timeout ambiguity.** A world that answers `Failed` and then
  `Done` for one request is the case the like button's server model
  excludes by treating the first answer as truth. The honest response is a
  resync effect after a timeout. This is the next milestone, not a someday.
- **Text from the browser.** Labels cross as JS string handles, but text
  that originates in the browser (an input field) has no event type; it
  would sit in the log as an opaque handle the fold never inspects. Needed
  by the chat client, not before.
- **`in_flight` grows with fire-and-forget effects forever.** By design,
  and checkpointed; note it before a domain fires thousands of them.

## Benchmark the list as divs, not a table

Every number so far is for an HTML `<table>`, first with automatic
layout, then with rows forced to grid rows and `content-visibility`. The
same operations should be measured on plain `<div>` rows under
`display: grid` and under `display: flex`, for all three implementations,
since a table drags its own layout rules into the result and most real
lists are not tables. The harness and the component are unchanged; only
the skeleton and the stylesheet differ.
