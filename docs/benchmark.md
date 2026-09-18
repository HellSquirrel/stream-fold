# Benchmark results

The js-framework-benchmark operations (create N rows, update every tenth,
select one, swap two, remove one, append N, clear) run by the harness in
`www/bench-vs.html` against three implementations in the same tab:
LogFold (`examples/bench`), vanilla JavaScript in the reference
benchmark's shape, and React 18 with memoised keyed rows. Same timing for
all: the operation, then a forced style and layout pass. One run,
foreground tab, Chrome 152, the development machine, 2026-09-17.

The table is an HTML `<table>` with rows laid out as grid rows with
fixed columns and `content-visibility: auto`, for all three. Numbers
rendered with `attr()`, not CSS counters. LogFold's rows are a keyed
family. Why each of those matters is in `docs/boundary.md`.

## Totals, milliseconds

Run 2026-09-18 after the four follow-ups (see `docs/boundary.md`, "Four
more"); the previous run's totals are in the progression table below.

| op | N | LogFold | vanilla | React 18 |
|---|---|---|---|---|
| create | 1,000 | 22 | 10 | 17 |
| update every 10th | 1,000 | 1 | 1 | 3 |
| select | 1,000 | 1 | 0 | 1 |
| swap | 1,000 | 1 | 1 | 6 |
| remove | 1,000 | 5 | 0 | 1 |
| append | 1,000 | 14 | 10 | 13 |
| clear | 1,000 | 2 | 2 | 6 |
| create | 10,000 | 103 | 102 | 283 |
| update every 10th | 10,000 | 9 | 1 | 8 |
| select | 10,000 | 2 | 5 | 9 |
| swap | 10,000 | 5 | 5 | 19 |
| remove | 10,000 | 42 | 6 | 11 |
| append | 10,000 | 114 | 77 | 428 |
| clear | 10,000 | 18 | 17 | 115 |
| create | 100,000 | 968 | 912 | 16,163 |
| update every 10th | 100,000 | 115 | 16 | 222 |
| select | 100,000 | 66 | 88 | 94 |
| swap | 100,000 | 81 | 93 | 174 |
| remove | 100,000 | 386 | 119 | 154 |
| append | 100,000 | 1,598 | 1,009 | 15,163 |
| clear | 100,000 | 231 | 221 | 588 |

## LogFold's split at 100,000 rows

Where each operation's time goes: `wasm` is the fold, the derivative or
the projection rebuild and diff; `DOM` is the shim's writes; `style` is
the browser's style and layout pass.

| op | wasm | DOM | style | total |
|---|---|---|---|---|
| create | 44 | 507 | 418 | 968 |
| update every 10th | 9 | 8 | 98 | 115 |
| select | 2 | 0 | 52 | 66 |
| swap | 2 | 10 | 69 | 81 |
| remove | 53 | 38 | 283 | 386 |
| append | 98 | 569 | 931 | 1,598 |
| clear | 34 | 196 | 1 | 231 |

Of create's DOM share about 320 ms is the browser parsing the 20 MB of
HTML the rows are inserted as; vanilla's equivalent JavaScript work is
536 ms. Select's 52 ms of style is what the browser charges for any
change in a 100,000-row container; vanilla's select pays 88.

## Where it started, at 100,000 rows

Totals before and after the day's changes, in the order they were made:
a B-tree projection diffed by lookup, then a sorted `Vec` projection,
then the derivative, then the page's sequential costs removed, then the
shim's HTML path, then keyed members.

| op | start | sorted Vec | derivative | grid rows | HTML path | keyed | interned + drop-all |
|---|---|---|---|---|---|---|---|
| create | 5,141 | 4,715 | 4,879 | 1,648 | 1,110 | 1,319 | 968 |
| select | 834 | 91 | 2 | 55 | 60 | 55 | 66 |
| swap | 1,209 | 443 | 365 | 6 | 8 | 77 | 81 |
| remove | 4,780 | 4,234 | 4,416 | 1,288 | 917 | 436 | 386 |
| append | 9,370 | 8,311 | 7,618 | 1,784 | 1,391 | 1,425 | 1,598 |
| clear | 2,741 | 2,719 | 2,629 | 1,743 | 1,142 | 337 | 231 |

Swap went up at the last step because it became two real node moves,
which pay the browser's relayout like everyone's, instead of two class
writes on rows that stayed put.

## The official driver: js-framework-benchmark on this machine

LogFold, the vanilla reference and three React 19 entries (`react-hooks`
on 19.2, `react-compiler-hooks` and `react-classes` on 19.0; the in-house
pages above use React 18.3 from a CDN) under
`krausest/js-framework-benchmark`'s own driver: its page, its Bootstrap table with automatic layout, its
Puppeteer runner with Chrome tracing so paint is included, its
plausibility and keyed checks (LogFold passes `isKeyed` for run, remove
and swap). Medians; CPU rows in ms, memory in MB, sizes in KB, first
paint in ms. In brackets, LogFold's script share, the part that is Rust
and glue rather than the browser. Raw files and the glue are in
`docs/results/js-framework-benchmark/`.

| benchmark | LogFold | LogFold, first run | vanilla | React 19.2 hooks | React 19 + compiler | React 19 classes |
|---|---|---|---|---|---|---|
| 01 create 1,000 rows | 34.1 (6.6) | 35.7 | 30.5 | 37.8 | 37.6 | 38.3 |
| 02 replace all 1,000 rows | 39.1 (10.8) | 40.6 | 33.7 | 46.6 | 45.7 | 45.3 |
| 03 partial update, every 10th | 19.0 (1.8) | 19.9 | 16.8 | 23.8 | 24.1 | 22.6 |
| 04 select row | 5.0 (0.7) | 5.1 | 5.7 | 9.7 | 12.9 | 10.8 |
| 05 swap rows | 20.3 (0.9) | 21.3 | 20.1 | 156.0 | 143.5 | 146.4 |
| 06 remove row | 18.3 (2.2) | 19.0 | 17.2 | 19.7 | 19.6 | 18.6 |
| 07 create 10,000 rows | 354.7 (50.1) | 378.0 | 319.2 | 565.8 | 629.9 | 632.2 |
| 08 append 1,000 to 1,000 | 39.5 (6.6) | 40.5 | 35.7 | 44.0 | 43.8 | 43.2 |
| 09 clear 1,000 rows | 15.0 (11.7) | 19.1 | 14.5 | 27.0 | 26.7 | 26.7 |
| 21 ready memory | 1.7 | 1.7 | 0.5 | 1.1 | 1.2 | 1.1 |
| 22 memory after create 1,000 | 4.0 | 4.0 | 1.9 | 4.4 | 4.5 | 4.5 |
| 25 memory after create/clear ×5 | 2.8 | 2.9 | 0.6 | 1.9 | 1.8 | 2.0 |
| 41 size, uncompressed | 91.0 | 89.8 | 11.3 | 190.3 | 183.0 | 184.6 |
| 42 size, compressed | 30.8 | 30.2 | 2.5 | 51.4 | 50.0 | 50.2 |
| 43 first paint | 52.0 | 49.3 | 57.3 | 288.2 | 267.2 | 271.8 |

The second LogFold column is the first official run; the first is after
interned slot names, pre-sorted family writes, a drop-all instruction and
an Fx hasher (`docs/boundary.md`, "Four more"). Raw files for that first
run are under `before-interning/`.

Read across: within 10 to 15% of vanilla on every CPU benchmark and
ahead of every React 19 variant on all of them; at vanilla's speed on
select, swap and clear, with React behind; on swap, React is seven times
slower. The React compiler changes nothing on this workload. The
script share is small everywhere except clear, where it is 15 of 19 ms:
a thousand drops sent one by one, where vanilla does one
`textContent = ""`. Memory is about twice vanilla's, the wasm heap and
the log; the bundle is 30 KB compressed against vanilla's 2.5 and
React's 51, and first paint is ahead of both.

None of this needed a change to the framework. The glue applies the
component's patches to the reference markup and turns the label's three
numbers into text; the real pages keep rendering labels from classes.

## Not measured

Plain `<div>` rows under `display: grid` and `display: flex`, which the
same harness should be run on next. Published js-framework-benchmark
numbers, which are on other hardware; the official driver was run here
instead, above.

## Reproduce

```sh
./scripts/build-www.sh bench-app
scripts/serve.py
# open http://127.0.0.1:8765/bench-vs.html in a foreground tab, press "run all"
# or, for LogFold alone with the exponential sweep: http://127.0.0.1:8765/bench.html
```
