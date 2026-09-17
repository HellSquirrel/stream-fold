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

| op | N | LogFold | vanilla | React 18 |
|---|---|---|---|---|
| create | 1,000 | 20 | 12 | 15 |
| update every 10th | 1,000 | 1 | 1 | 3 |
| select | 1,000 | 0 | 1 | 1 |
| swap | 1,000 | 1 | 1 | 6 |
| remove | 1,000 | 5 | 1 | 1 |
| append | 1,000 | 15 | 8 | 13 |
| clear | 1,000 | 3 | 2 | 7 |
| create | 10,000 | 129 | 120 | 348 |
| update every 10th | 10,000 | 10 | 1 | 8 |
| select | 10,000 | 0 | 5 | 11 |
| swap | 10,000 | 6 | 5 | 22 |
| remove | 10,000 | 43 | 6 | 18 |
| append | 10,000 | 116 | 76 | 232 |
| clear | 10,000 | 27 | 17 | 55 |
| create | 100,000 | 1,319 | 881 | 16,364 |
| update every 10th | 100,000 | 116 | 11 | 111 |
| select | 100,000 | 55 | 59 | 104 |
| swap | 100,000 | 77 | 86 | 163 |
| remove | 100,000 | 436 | 112 | 149 |
| append | 100,000 | 1,425 | 1,120 | 15,492 |
| clear | 100,000 | 337 | 209 | 589 |

## LogFold's split at 100,000 rows

Where each operation's time goes: `wasm` is the fold, the derivative or
the projection rebuild and diff; `DOM` is the shim's writes; `style` is
the browser's style and layout pass.

| op | wasm | DOM | style | total |
|---|---|---|---|---|
| create | 171 | 603 | 546 | 1,319 |
| update every 10th | 17 | 8 | 91 | 116 |
| select | 2 | 0 | 53 | 55 |
| swap | 3 | 7 | 67 | 77 |
| remove | 125 | 32 | 279 | 436 |
| append | 176 | 575 | 674 | 1,425 |
| clear | 42 | 294 | 0 | 337 |

Of create's 603 ms of DOM, 319 ms is the browser parsing the 20 MB of
HTML the rows are inserted as; vanilla's equivalent JavaScript work is
468 ms. Select's 53 ms of style is what the browser charges for any
change in a 100,000-row container; vanilla's select pays 59.

## Where it started, at 100,000 rows

Totals before and after the day's changes, in the order they were made:
a B-tree projection diffed by lookup, then a sorted `Vec` projection,
then the derivative, then the page's sequential costs removed, then the
shim's HTML path, then keyed members.

| op | start | sorted Vec | derivative | grid rows | HTML path | keyed |
|---|---|---|---|---|---|---|
| create | 5,141 | 4,715 | 4,879 | 1,648 | 1,110 | 1,319 |
| select | 834 | 91 | 2 | 55 | 60 | 55 |
| swap | 1,209 | 443 | 365 | 6 | 8 | 77 |
| remove | 4,780 | 4,234 | 4,416 | 1,288 | 917 | 436 |
| append | 9,370 | 8,311 | 7,618 | 1,784 | 1,391 | 1,425 |
| clear | 2,741 | 2,719 | 2,629 | 1,743 | 1,142 | 337 |

Swap went up at the last step because it became two real node moves,
which pay the browser's relayout like everyone's, instead of two class
writes on rows that stayed put.

## Not measured

Memory. First paint. Plain `<div>` rows under `display: grid` and
`display: flex`, which the same harness should be run on next. Published
js-framework-benchmark numbers, which are on other hardware.

## Reproduce

```sh
./scripts/build-www.sh bench-app
scripts/serve.py
# open http://127.0.0.1:8765/bench-vs.html in a foreground tab, press "run all"
# or, for LogFold alone with the exponential sweep: http://127.0.0.1:8765/bench.html
```
