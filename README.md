# LogFold

An event-log runtime: the UI is `render(fold(log))`, effects are values,
time is an event. Design document: [`docs/logfold-proposal.md`](docs/logfold-proposal.md).

## Layout

- `crates/logfold-core` — log, event model, effects (as a diffed projection), expectations.
- `crates/like-local` — the whole idea in one file: a like button, click flips a flag. Start here.
- `crates/like-button` — M0 example with a request, an honest host, a server model and an adversarial fuzz harness. No browser, no WASM yet.

## Run

```sh
cargo test --workspace
```

## M0 status

- [x] Log; views with absolute indices (`prefix`, `suffix`, `slice`); virtual `now()` (itself a fold)
- [x] `Fold` as a value (`foldl` style): `run`, `scan`, `map`, `zip`, `scoped`; a checkpoint is `fold.from(state, upto)`; `checkpoint_law` checks resume == run at every split; ordered `Checkpoints` store with nearest-at-or-before lookup
- [x] Expectations built from a fold + predicate (`Expectation::on`): all-prefix mode is one `scan`; `raw` escape hatch for non-fold properties
- [x] Events split by origin: `Pure` (input), `Io` (world), `Started` (host bookkeeping); `Log::inputs()` seeds re-execution
- [x] Effects as desired-set projection; `InFlight` fold (started − answered, result-less effects resolve on start); host diff = desired − in-flight
- [x] Expectations: guard mode and all-prefix (fuzz) mode
- [x] Like-button fold + benign proptest harness
- [x] Adversarial generators (out-of-order answers, `Failed`/`Cancelled` mid-flight, duplicate and bogus answers, clock jumps)
- [x] Client/server agreement expectation (the test plays the server as a fold)
- [x] **Exit criterion met.** The fuzzer broke the naive one-request-per-click fold with a 9-event log (three concurrent requests, answered out of order) and shrank it. The fold now coalesces clicks into one in-flight request; the shrunk sequence is a regression test.

Next: M1 groundwork — outputs vs actions in the effect type (render is an output), then the WASM boundary and a DOM patch host.
