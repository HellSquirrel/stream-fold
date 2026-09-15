# LogFold — an event-log runtime for UI and beyond

*Working proposal, v0.1 — 14 Sep 2026. The code has diverged since; section 11 records each decision and points at the code that is now authoritative. Section numbers are stable: the crates cite them.*

## 1. One-paragraph pitch

Replace mutable application state with an append-only **event log** and a set of **pure folds** (projections) over it. The UI is `render(fold(log))`. Side effects are returned as **values** and interpreted by a thin host shim; their results come back as events. **Time is an event.** Behavioural expectations (`expect(button).visible().when(...)`) are written inline in components and are simultaneously runtime guards, property-based tests, and documentation. The core is Rust compiled to WASM; the DOM is just one effect interpreter, so the same core drives robot fleets, simulations, or servers.

## 2. Why not MV* / signals / Elm

| Problem in existing models | How LogFold answers it |
|---|---|
| State is mutable; bugs are "how did we get here" | There is no state; every value is derived from an ordered log, so "how" is always answerable |
| Async is scattered `await`s, races, stale closures | Async = two events (`Effect` out, `Done/Failed` in); races resolved by fold logic over request ids |
| Tests live elsewhere and drift | Expectations are part of the component and fuzzed automatically |
| Time (debounce, animation, retry) is imperative | Time is data; tests advance a virtual clock |
| Replaying a bug needs a repro | The log **is** the repro |

Elm and Redux got the "pure update" half. They kept a single mutable-in-spirit model, had no first-class time, no effect fuzzing, and no inline specification.

## 3. Core principles

1. **Log is the only truth.** Global, append-only, physically sharded by key.
2. **Folds are pure and scoped.** A projection declares the event keys it reads. Scope drives invalidation and test isolation.
3. **Effects are values.** The core never touches the world.
4. **Time is an input.** `Tick` events; no wall-clock reads inside the core.
5. **Intent, not frames.** Animations, trajectories, drags are logged once as intent and sampled by time.
6. **Expectations are executable.** Each `.expect` is a property checked by fuzzing, a dev-mode guard, and (where typestate allows) a compile-time proof.
7. **Text stays in the host.** Strings cross the boundary as `externref` handles / interned ids; the fold treats them as opaque unless a projection explicitly inspects text.

## 4. Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ Host shim (JS / ROS driver / sim)                            │
│   • appends events         • interprets effects              │
│   • runs rAF sampling      • owns escape-hatch buffers        │
└───────────────▲────────────────────────────┬─────────────────┘
                │ events                     │ effects + view tree
┌───────────────┴────────────────────────────▼─────────────────┐
│ Core (Rust → WASM)                                           │
│   Log (sharded, persistent)                                  │
│   Projection graph (salsa-style memo, scope-keyed)           │
│   Fold + Effect emission                                     │
│   Expectation engine (guard mode)                            │
│   Clock (virtual)                                            │
└──────────────────────────────────────────────────────────────┘
        ▲
        │ same core, different shim
┌───────┴─────────────┐  ┌─────────────────┐  ┌──────────────┐
│ Fuzzer / test host  │  │ Devtools host   │  │ Sim / twin   │
└─────────────────────┘  └─────────────────┘  └──────────────┘
```

### 4.1 Event model

> Superseded in v0.2, see section 11 (a) and (b): events are parametric over a `Domain`, `Sense` was added, and `Started` is a logged host event.

```rust
type Key = SmolStr;            // "post:42", "conn:feed", "robot:7", "io"
type ReqId = u64;
type ConnId = u32;

enum Event {
    Ui   { key: Key, ev: UiEvent },                     // Click, Input(commit), DragEnd{velocity}, ...
    Io   { key: Key, req: ReqId, res: IoResult },       // Done / Failed / Cancelled
    Conn { id: ConnId, ev: ConnEvent },                 // Opened / Closed{code} / Error
    Msg  { id: ConnId, seq: u64, payload: Handle },     // coalesced per frame if high-rate
    Tick { ms: u64 },                                   // virtual time
    Cmd  { key: Key, cmd: Command },                    // coordinator → log (robots)
    Meta { ev: MetaEvent },                             // Checkpoint, SchemaVersion, Redacted
}
```

Every event carries a monotonic index and a hybrid logical clock (HLC) so multi-node logs merge causally.

### 4.2 Effects

> Superseded in v0.2, see section 11 (c), (e) and (f): effects are a desired-set projection the host diffs, `Render` is an output rather than an effect, and `Delay` carries only a request id.

```rust
enum Effect {
    Post   { req: ReqId, url: Handle, body: Handle, idem_key: IdemKey },
    Cancel { req: ReqId },
    Delay  { until_ms: u64, then: Box<Event> },     // becomes a Tick + follow-up event
    Connect{ id: ConnId, url: Handle },
    Send   { id: ConnId, payload: Handle },
    Disconnect { id: ConnId },
    RequestFrames { until_ms: u64 },                // animation sampling window
    Compute{ req: ReqId, job: Job },                // heavy pure work on a worker
    Render { patch: PatchStream },                  // DOM shim only
    Trajectory { robot: RobotId, seg: Segment },    // robot shim only
    EStop  { robot: RobotId },
}
```

Rule: every effect that is not idempotent carries an `idem_key` derived from `(scope, log index)`. The shim keeps a durable outbox so a crash between "effect fired" and "result logged" cannot double-fire on restart.

### 4.3 Projections

```rust
#[projection(scope = "post:{id}")]
fn liked(id: PostId, log: &View<Log>) -> bool { ... }
```

- Memoised via a salsa-like database keyed by `(fn, args)`.
- Dependency tracking records which log slices were read → gives invalidation **and** provenance (devtools "why is this X").
- Cross-cutting projections subscribe to key patterns (`conn:*`, `robot:*`).
- Independent scopes may recompute in parallel (wasm threads / rayon) since they share only immutable log slices.

### 4.4 Effect emission and feedback

> Superseded in v0.2, see section 11 (c): nothing is emitted; the host diffs the desired set against `in_flight`.

A frame: `append(events) → invalidate → recompute dirty projections → collect effects → hand to shim`. Effects may produce new events (coordinator pattern). Guard: per-frame event budget + cycle detection on projection→event→projection edges; exceeding either raises a dev-mode error.

### 4.5 Time and animation

- `Tick` cadence is shim-defined (rAF for UI, control-loop period for robots).
- `Anim { start, dur, ease, track }` is a value in the view tree. The shim samples `anim.sample(now)` each frame without calling into the core. Interruption: a new intent seeds `from`/velocity by sampling the previous intent at the interrupt time (guarantees continuity).
- Debounce, backoff, timeouts are folds over `Tick`.

### 4.6 Connections (websocket / robot link)

Small state machine per connection: `Idle → Connecting{attempt} → Open → Backoff{until} → Connecting…`. Desired subscriptions are a projection; acked subscriptions come from `Msg`; the diff emits `Send(Subscribe)` — reconnect resubscription is automatic. Outbound queue = issued − acked, replayed in order on `Opened`. Sequence gaps emit `Resync`.

### 4.7 Escape hatches (deliberate, bounded)

| Domain | Local buffer | Committed event |
|---|---|---|
| Pointer drag | shim drives transform | `DragEnd { velocity }` |
| Text input / IME | controlled-input bridge | `Input(final)` |
| 1 kHz control loop | on-robot controller | setpoints / decisions |
| High-rate telemetry | coalesce per frame | `Msg(batch)` with truncated payload after checkpoint |

Every hatch must declare its committed event type; the fuzzer treats hatch outputs as ordinary inputs.

### 4.8 Expectations

```rust
.expect(target).predicate().when(cond)                    // invariant under condition
.expect(target).predicate().after(seq...)                 // temporal
.expect(target).predicate().at(t).after(seq)              // time-sampled (animation)
.expect(Effect::X).emitted_once().after(seq)              // effect-level
.expect(target).continuous().after(seq)                   // no discontinuity in sampled value
```

Three execution modes:

1. **Fuzz** (`cargo test`): proptest generates event sequences within the projection's scope, plus adversarial interleavings (`Failed` mid-flight, duplicate `Opened`, clock jumps, response-after-navigation). Shrinks to a minimal failing log.
2. **Guard** (dev build): re-evaluated on every frame; failures link to the offending event index.
3. **Static** (opt-in): flows encoded as typestate; illegal transitions don't compile.

### 4.9 Log lifecycle

- Shards per key; per-shard checkpoint after N events (checkpoint = fold output at index i, hashed).
  > Superseded in v0.2, see section 11 (d): a checkpoint is `fold.from(state, upto)`; the cadence is the host's policy.
- Flight-recorder window: on any `Failed`/`Fault`/expectation breach, retain full-rate events ±window around it.
- Export = `{ build_hash, schema_version, checkpoints, tail }`.
- **Versioning:** each build embeds a schema version; events are upcast via versioned decoders. Replays of an older log run against the archived build by `build_hash`; cross-version replay is best-effort and flagged.
- **Privacy:** event payloads are typed; `Sensitive<T>` fields are redacted on export. Redacted logs replay structurally (same control flow) but not byte-identically; devtools shows the difference.

## 5. Host integration (browser)

- Rust core via `wasm-bindgen`; strings as `externref` using JS String Builtins where available (Chrome 130+, Firefox 134+); fallback to interned ids + `TextDecoder` on Safari.
- Render shim applies `PatchStream` (create/attr/text/move/remove) to the DOM; no virtual DOM in JS.
- Animations sampled in JS from `Anim` values; `RequestFrames` bounds the rAF loop.
- Workers via `wasm-bindgen-rayon` for parallel projection recompute and `Compute` jobs.
- Memory: keep text out of linear memory; monitor high-water mark; `memory.discard` when it ships.

## 6. Devtools

Single panel, all derived from the log:

1. **Timeline** — events by key lane, scrubbable `now`, connection lanes with state phases.
2. **Projections** — live table of memoised cells with value + contributing event indices; "explain" replays one cell step by step.
3. **Effects** — in-flight requests, scheduled delays, open subscriptions.
4. **Expectations** — live green/red list linking to breaching event.
5. **Inject** — append a synthetic event (e.g. fake `Failed`) to explore branches.
6. **Export / Import** — log bundle.

## 7. Milestones

### M0 — Spike (2–3 weeks)
- Log + one shard type, persistent vec (`im`/`rpds`).
- Minimal salsa-style memo with dependency recording.
- Virtual clock, `Tick`.
- Like button end-to-end in a browser: click → optimistic → `Post` → `Done/Failed`.
- Two expectations fuzzed with proptest.
**Exit:** double-click dedupe bug found by fuzzer, not by hand.

*As built:* a plain `Vec` log, no memo layer, folds as values with host-owned checkpoints (section 11 (d), (g)); the exit criterion was met by the like button in `crates/like-button`, and the second host arrived early as a robot vacuum in `crates/brunhilda` rather than waiting for M3.

### M1 — Real UI (6–8 weeks)
- Patch-stream renderer; externref strings.
- Controlled input bridge, IME.
- `Anim` values + rAF sampling + interruption continuity.
- Connection state machine, resubscribe, outbound queue.
- Devtools panes 1–3.
**Exit:** a chat client (list, composer, websocket, typing indicator, optimistic send) with ≥ 20 expectations and a replayable bug bundle.

### M2 — Hardening (6 weeks)
- Idempotent outbox, crash-restart safety.
- Checkpoints, flight-recorder window, export/import, redaction.
- Schema versioning + upcasters; archived-build replay.
- Event budget + cycle detection.
- Perf pass: 10k-row table, scope granularity tooling.
**Exit:** replay of a 1-week-old log on the archived build is bit-identical.

### M3 — Second host (8 weeks)
- Sim shim (physics) + fleet coordinator projection.
- HLC ordering, multi-node log merge.
- Safety expectations (`never().inside(zone)`, `EStop within 50ms`).
**Exit:** same WASM binary runs a 10-robot sim and a browser dashboard from one log.

## 8. Known weak spots and the design answer

| Weak spot | Mitigation baked in | Residual risk |
|---|---|---|
| Fold versioning breaks replay | schema versions, upcasters, archived builds by hash | cross-version replay is best-effort |
| Browser measurement leaks into purity | log measurements as events only for layout-dependent projections; render is "pure given measurements" | replay on other devices diverges in layout |
| Escape hatches grow | each hatch must declare a committed event; audited list | hatch code isn't fuzzed |
| Exactly-once at effect boundary | idempotency keys + durable outbox | still at-least-once semantics for the world |
| State machines everywhere | flow combinators (`sequence`, `race`, `retry`) over folds; typestate helpers | readability tax vs `await` |
| Expectations give false confidence | mandatory adversarial generators; coverage report per scope | doesn't cover "looks wrong" |
| Feedback loops | per-frame budget, cycle detection | complex coordinators still hard to reason about |
| Memory high-water mark | text in host; small shards; checkpoints | linear memory never shrinks in browsers today |
| Log is a surveillance record | typed `Sensitive<T>`, structural redaction | redacted replays aren't byte-identical |
| Hard real-time | control loop in hatch; core is decision layer | novelty is planning-level, not control-level |

## 9. Open questions to resolve on the laptop

1. Salsa vs Adapton vs hand-rolled memo — measure recompute overhead on a 10k-cell projection graph.
2. `externref` strings from Rust: ergonomics of an opaque `Text` handle type; where do comparisons happen?
3. Event schema encoding: postcard vs a custom tagged format with stable ids for upcasting.
4. HLC vs Lamport for the browser-only case (do we need it before M3?).
5. Expectation DSL surface: Rust-only, or a TS mirror for component authors?
6. Flow combinators: can `sequence(a, b, c)` desugar to a fold without hiding the state machine from devtools?
7. Patch-stream format: reuse Dioxus/Leptos internals or write fresh?
8. What's the smallest demo that convinces a sceptical frontend engineer? (Candidate: the chat client with a "replay this bug" button.)

## 10. Naming and non-goals

- Working name: **LogFold**.
- Non-goals for v1: SSR beyond `fold(empty_log)`, Safari-optimised strings, hard real-time control, a component library.


## 11. Decisions since v0.1

Each entry is a fact about the code, not a design argument; the argument lives in the rustdoc it points at. Sections above are annotated where superseded and are otherwise left as written.

- **(a) Events are domain-parametric.** `Event<D>` over a `Domain` trait (`Input`, `Sense`, `Effect`) replaces the closed enum of 4.1. `Ui` became `Input`, `Cmd` folded into it, and `Sense` was added for unsolicited world input (a bump, a sighting, a socket message), distinct from `Io`, which answers a request. See `logfold_core::event`.
- **(b) `Started` is a logged host event.** The host appends it *before* performing an effect. It carries no request id of its own; a result-bearing effect carries its own id through `Action::req`. See `logfold_core::event::Event::started`.
- **(c) Effects are a diffed projection, not an emitted stream.** A projection returns the set of effects that should be in flight; the host diffs it against the `in_flight` fold (started minus answered) and starts the difference. The desired set is level-triggered: an effect in flight stays desired until it is answered. Fire-and-forget effects stay in `in_flight` forever as the record that they happened. See `logfold_core::effect` and `logfold_core::fold::in_flight`.
- **(d) A checkpoint is a fold with a different start.** `Fold::from(state, upto)` is the same fold starting from a saved state; the left-fold law (`checkpoint_law`) is what makes it honest. `Checkpoints` is an ordered store of saved states with nearest-at-or-before lookup; when to take one is the host's policy, never the core's. See `logfold_core::fold`.
- **(e) Outputs are not effects.** The DOM, a canvas, a motor heading: idempotent functions of state that the host diffs against the world every frame and never logs. `Render` is therefore not in any effect type. Effects proper (a request, an e-stop) are logged. See `logfold_core::effect` and `brunhilda`.
- **(f) `Delay` carries only a request id.** The host answers it with an ordinary `Io` event rather than re-injecting an embedded event, so the shim stays dumb. Not yet exercised by an example.
- **(g) Folds are values, `foldl` style.** Init, step and done, with `run`, `scan`, `map`, `zip` and `scoped`. `scoped` is the declared scope of 3.2. The salsa-style memo of 4.3 is not built; the checkpoint store covers M0's needs. See `logfold_core::fold::Fold`.
- **(h) Expectations are a fold plus a predicate.** `Expectation::on` checks every prefix in one scan; `raw` is the quadratic escape hatch. The fluent DSL of 4.8 is deferred until there are enough expectations to generalise from. See `logfold_core::expect`.
- **(i) Text stays in the host, so far only for labels.** Labels cross the boundary as JS string handles made once; numbers cross as numbers. Event keys are plain `String`s inside WASM and never leave it. Text that originates in the browser has no event type yet. See `logfold_web`.
- **(j) The second host is a simulated robot vacuum, not a chat client.** It exercises time as the driver, outputs versus effects, temporal expectations and dead reckoning against a sim, and it found two safety bugs in the "careful" controller before it held. See `brunhilda`.

---

*Prior art to read before starting:* Elm architecture; Redux + redux-saga; Adapton / salsa (incremental computation); event sourcing upcasters (Axon, EventStoreDB); Jepsen-style fault injection; FoundationDB's deterministic simulation testing; ROS 2 lifecycle nodes; Rust `im`/`rpds`; Dioxus/Leptos renderer internals; WebAssembly JS String Builtins & memory-control proposals.
