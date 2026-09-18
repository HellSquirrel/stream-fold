// Benchmark glue, and only that. LogFold renders from numbers and lets CSS
// draw text; js-framework-benchmark asserts on text nodes and on the
// reference markup, so this file applies the component's patches to that
// markup and composes the label text from the numbers. It stands in for
// www/logfold.mjs on this page and nowhere else. It is not the framework
// and is dropped when the numbers are in.

import init, { BenchApp } from "../dist/bench_app.js";
import manifest from "../dist/bench.manifest.mjs";

await init();
const app = new BenchApp();
const names = manifest.names;
const inputs = new Map(manifest.inputs.map((n, i) => [n, i]));
const ADJ = manifest.slots.adj.values, COL = manifest.slots.colour.values, NOUN = manifest.slots.noun.values;
const tbody = document.getElementById("tbody");
const NONE = new Uint8Array(0);

// key -> { tr, id, adj, colour, noun, bangs, selected }
const rows = new Map();
const label = (r) => `${ADJ[r.adj]} ${COL[r.colour]} ${NOUN[r.noun]}` + " !!!".repeat(r.bangs);
const markup = (r) =>
  `<tr data-key="${r.key}"${r.selected ? ' class="danger"' : ""}><td class='col-md-1'>${r.id}</td><td class='col-md-4'><a>${label(r)}</a></td><td class='col-md-1'><a><span class='glyphicon glyphicon-remove' aria-hidden='true'></span></a></td><td class='col-md-6'></td></tr>`;

function apply(patch) {
  const fresh = new Map(), touched = new Set(), drops = [], moves = [];
  for (let i = 0; i < patch.length; i += 5) {
    const target = names[patch[i]], key = patch[i + 1], kind = patch[i + 2], name = names[patch[i + 3]], x = patch[i + 4];
    if (target !== "row") continue;                                  // the root's --count is not shown
    let r = rows.get(key) ?? fresh.get(key);
    if (kind === 4) {                                                // order: NaN drops, else a position
      if (Number.isNaN(x)) drops.push(key);
      else if (rows.has(key)) moves.push([x, key]);
      else (r ?? fresh.set(key, (r = { key, id: 0, adj: 0, colour: 0, noun: 0, bangs: 0, selected: false, order: 0 })).get(key)).order = x;
      continue;
    }
    if (!r) fresh.set(key, (r = { key, id: 0, adj: 0, colour: 0, noun: 0, bangs: 0, selected: false, order: undefined }));
    if (rows.has(key)) touched.add(key);
    const v = Number.isNaN(x) ? 0 : x;
    if (kind === 3) { if (name === "selected") r.selected = v === 1; else if (name !== "present") r[name] = v; }
    else if (kind === 1) { if (name === "data-id") r.id = v; else if (name === "data-bangs") r.bangs = v; }
  }
  // new rows, in order-number order, as one HTML string
  if (fresh.size) {
    const list = [...fresh.values()].sort((a, b) => (a.order ?? Infinity) - (b.order ?? Infinity));
    const placed = list.every((r, i) => r.order === rows.size + i);
    const first = tbody.childElementCount;
    tbody.insertAdjacentHTML("beforeend", list.map(markup).join(""));
    list.forEach((r, i) => { r.tr = tbody.children[first + i]; r.shownId = r.id; r.shownLabel = label(r); rows.set(r.key, r); if (!placed && r.order !== undefined) moves.push([r.order, r.key]); });
  }
  for (const key of touched) {                                     // write only what changed
    const r = rows.get(key), tr = r.tr, text = label(r);
    if (r.shownId !== r.id) { tr.firstChild.textContent = r.id; r.shownId = r.id; }
    if (r.shownLabel !== text) { tr.children[1].firstChild.textContent = text; r.shownLabel = text; }
    tr.classList.toggle("danger", r.selected);
  }
  for (const key of drops) { rows.get(key)?.tr.remove(); rows.delete(key); }
  if (moves.length) {
    moves.sort((a, b) => a[0] - b[0]);
    const at = (p) => tbody.children[p] ?? null;
    for (const [p, key] of moves) {
      const el = rows.get(key)?.tr, ref = at(p);
      if (!el || ref === el) continue;
      if (ref && el.compareDocumentPosition(ref) & Node.DOCUMENT_POSITION_FOLLOWING) tbody.insertBefore(el, ref.nextSibling);
      else tbody.insertBefore(el, ref);
    }
    if (moves.some(([p, key]) => at(p) !== rows.get(key)?.tr)) {   // always-right fallback
      const movers = moves.map(([p, key]) => [p, rows.get(key)?.tr]).filter(([, el]) => el);
      for (const [, el] of movers) el.remove();
      for (const [p, el] of movers) tbody.insertBefore(el, at(p));
    }
  }
}

const dispatch = (input, key = -1) => apply(app.dispatch(inputs.get(input), key, NONE));
apply(app.render_at(0));

document.getElementById("main").addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (b) { e.preventDefault(); dispatch({ run: "run", runlots: "runlots", add: "add", update: "update", clear: "clear", swaprows: "swap" }[b.id]); return; }
  const a = e.target.closest("a"), tr = e.target.closest("tr");
  if (!a || !tr) return;
  e.preventDefault();
  dispatch(a.parentElement.classList.contains("col-md-1") ? "remove" : "select", Number(tr.dataset.key));
});
