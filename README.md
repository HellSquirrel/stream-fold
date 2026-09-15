# LogFold

An event-log runtime: the UI is `render(fold(log))`, effects are values,
time is an event. Design document: [`docs/logfold-proposal.md`](docs/logfold-proposal.md).

## Layout

- `crates/logfold-core` — log, event model, effects (as a diffed projection), expectations.
- `crates/like-local` — the whole idea in one file: a like button, click flips a flag. Start here.
- `crates/like-button` — M0 example with a request, an honest host, a server model and an adversarial fuzz harness.
- `crates/logfold-web` — browser host for `like-local`: the log lives in WASM, the DOM is an output, a slider scrubs history through checkpoints.
- `crates/brunhilda` — a robot vacuum cleaner as a fold, a simulated room as a second host, and a fuzzer that plays the human she keeps attacking.

## Run

```sh
cargo test --workspace
```

## Browser demo

```sh
cargo install wasm-pack            # once
wasm-pack build crates/logfold-web --target web --out-dir pkg
cd crates/logfold-web && python3 -m http.server 8765
# open http://127.0.0.1:8765/www/index.html      (the like button)
# open http://127.0.0.1:8765/www/brunhilda.html  (you vs. Brunhilda; arrow keys move you)
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
- [x] Effects as a level-triggered desired-set projection; `in_flight` fold (started − answered; result-less effects stay as the record); the host diff runs both ways: start = desired − in-flight, cancel = in-flight − desired
- [x] Expectations: guard mode and all-prefix (fuzz) mode
- [x] Like-button fold + benign proptest harness
- [x] Adversarial generators (out-of-order answers, `Failed`/`Cancelled` mid-flight, duplicate and bogus answers, clock jumps)
- [x] Client/server agreement expectation (the test plays the server as a fold)
- [x] **Exit criterion met.** The fuzzer broke the naive one-request-per-click fold with a 9-event log (three concurrent requests, answered out of order) and shrank it. The fold now coalesces clicks into one in-flight request; the steps that produced the log replay as a regression test (the log itself cannot, since its `Started` events were the old host's output).

- [x] Browser host: `like-local` in WASM with a scrubbable history (`crates/logfold-web`)
- [x] Events are domain-parametric (`Domain` trait: `Input`, `Sense`, `Effect`); `Sense` is unsolicited world input; effects live in each domain
- [x] Second host: Brunhilda. Time drives her, a heading is an *output* the sim reads every frame, an e-stop is a fire-and-forget *action* the host latches. Three fold-based expectations (never in the human's cell, frozen after e-stop, coverage monotone) plus dead-reckoning and attack-count agreement with the sim at frame boundaries.
- [x] **Exit criterion met, twice.** The fuzzer breaks the naive policy (drives adjacent to you, you step into her path). It also broke the *careful* policy twice before it held: `Dock` after an e-stop resumed her brain while the host latch still held, and `Start` moved her before she had sensed anything, running over anyone standing by the dock.

- [x] Brunhilda plans: nearest uncleaned cell by breadth-first search over the map she knows, docks when the job is done, waits when the only cells left are the ring around you. The sim keeps a host-side checkpoint so each frame costs the events since the last one.
- [x] Brunhilda in the browser (`www/brunhilda.html`): you are the human, arrow keys move you, the lower canvas replays what she knew at any index. Switch the policy to *naive* and step in front of her.

Next: the networked like button in the browser against a fake server; the timeout ambiguity (`Failed` then `Done`) and a resync effect.
