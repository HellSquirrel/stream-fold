# LogFold

An event-log runtime: the UI is `render(fold(log))`, effects are values,
time is an event. Design document: [`docs/logfold-proposal.md`](docs/logfold-proposal.md).

## Layout

- `crates/logfold-core` — log, event model, effects (as a diffed projection), expectations.
- `crates/like-local` — the whole idea in one file: a like button, click flips a flag. Start here.
- `crates/like-button` — M0 example with a request, an honest host, a server model and an adversarial fuzz harness.
- `crates/logfold-web` — browser host for `like-local`: the log lives in WASM, the DOM is an output, a slider scrubs history through checkpoints.

## Run

```sh
cargo test --workspace
```

## Browser demo

```sh
cargo install wasm-pack            # once
wasm-pack build crates/logfold-web --target web --out-dir pkg
cd crates/logfold-web && python3 -m http.server 8765
# open http://127.0.0.1:8765/www/index.html
```

The shim in `www/index.html` appends an event per click, writes the view
into the DOM, and asks for the view at any index when the slider moves.
Checkpoints are taken every 8 events, in the host, and the provenance line
shows which one the scrubber resumed from.

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

- [x] Browser host: `like-local` in WASM with a scrubbable history (`crates/logfold-web`)

Next: outputs vs actions in the effect type (render is an output, and the demo treats it as one); then the networked like button in the browser against a fake server.
