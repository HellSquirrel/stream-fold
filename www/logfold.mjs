// LogFold host shim. Generic: it knows targets, variables and inputs by
// name and never what they mean. One `mount` per component on a page.
//
// The skeleton names targets with `data-fold="name"` (the root is "root")
// and inputs with `data-on="click:toggle"`; several entries may be
// separated by spaces. Rust decides what an input means and which numbers
// land on which target; the stylesheet decides what the numbers look like.
//
// A family of targets is `data-fold="item-0"`, `item-1`, … pre-rendered,
// or grown on demand from `<template data-fold="item">` (one element in
// it): the first patch that names `item-N` clones members up to N, in
// order, after the template. Members are never removed; hide them by CSS.
//
// An input fired from inside a family member sends the member's index with
// it (`data-on="click:toggle"` inside `data-fold="item-3"` sends 3).
//
// Text: an input fired from a form field sends the field's value with it
// (bytes; Rust decides whether the input keeps them). `data-on="enter:add"`
// fires on the Enter key and empties the field afterwards. A text slot's
// number is the log index of the input that carried the text; the shim asks
// the app for it and writes it into the target's `[data-text="name"]`
// child, or the target itself.

export function mount(app, { root = document, manifest = null } = {}) {
  // Names: from the manifest when the component declared one (ids are fixed
  // by declaration order), else asked once per id and cached as a handle.
  const names = new Map();
  const name = (id) => {
    if (!names.has(id)) names.set(id, manifest ? manifest.names[id] : app.name(id));
    return names.get(id);
  };
  const targets = new Map();
  const grown = new Map();         // family name -> members cloned from its template so far
  const grow = (n, i) => {
    const tpl = root.querySelector(`template[data-fold="${n}"]`);
    if (!tpl) return null;
    let members = grown.get(n);
    if (!members) grown.set(n, (members = []));
    while (members.length <= i) {
      const el = tpl.content.firstElementChild.cloneNode(true);
      el.dataset.fold = `${n}-${members.length}`;
      (members[members.length - 1] ?? tpl).after(el);
      members.push(el);
    }
    return members[i];
  };
  const target = (id, index) => {
    const key = index < 0 ? id : `${id}:${index}`;
    if (!targets.has(key)) {
      const n = name(id);
      targets.set(key, n === "root" ? document.documentElement
        : root.querySelector(`[data-fold="${index < 0 ? n : `${n}-${index}`}"]`) ?? (index < 0 ? null : grow(n, index)));
    }
    return targets.get(key);
  };
  const inputs = new Map((manifest ? manifest.inputs : app.input_names()).map((n, i) => [n, i]));
  const enc = new TextEncoder(), NONE = new Uint8Array(0);
  // How a slot's number is spelled on the page: a bool attribute is present or
  // absent, an enum attribute is its value's name, everything else the number.
  const spell = (kind, n, x) => {
    const d = manifest?.slots?.[n];
    if (!d || d.kind !== (kind ? "attr" : "var")) return x;
    if (d.bool) return x ? "" : null;
    if (d.values) return d.values[x] ?? x;
    return x;
  };
  const vars = new Map();          // what the DOM holds, mirrored for devtools
  const listeners = new Set();

  const host = {
    app,
    vars,
    /** Write a patch of `[target, index, kind, name, value]`; index is a
     *  family member or -1; kind 0 is a custom property, 1 an attribute,
     *  2 text (the value is a log index); NaN clears. */
    apply(patch) {
      for (let i = 0; i < patch.length; i += 5) {
        const el = target(patch[i], patch[i + 1]), kind = patch[i + 2], n = name(patch[i + 3]), x = patch[i + 4];
        if (!el) continue;
        if (kind === 2) {
          (el.querySelector(`[data-text="${n}"]`) ?? el).textContent = Number.isNaN(x) ? "" : (app.text(x) ?? "");
        } else {
          const v = Number.isNaN(x) ? null : spell(kind, n, x);
          if (v === null) { kind ? el.removeAttribute(n) : el.style.removeProperty(n); }
          else { kind ? el.setAttribute(n, v) : el.style.setProperty(n, v); }
        }
        if (name(patch[i]) === "root") { Number.isNaN(x) ? vars.delete(n) : vars.set(n, x); }   // the mirror is for devtools; root only
      }
      for (const fn of listeners) fn(host);
      return host;
    },
    /** Dispatch an input by name, with the text and the family member index it carries, if any. */
    dispatch(input, text = "", index = -1) {
      if (!inputs.has(input)) throw new Error(`unknown input "${input}"; known: ${[...inputs.keys()].join(", ")}`);
      return host.apply(app.dispatch(inputs.get(input), index, text ? enc.encode(text) : NONE));
    },
    tick(ms = performance.now()) { return host.apply(app.tick(ms)); },
    /** One frame of the component's simulated world, `dt` milliseconds long. */
    frame(dt) { return host.apply(app.frame(dt)); },
    renderAt(n) { return host.apply(app.render_at(n)); },
    onChange(fn) { listeners.add(fn); return () => listeners.delete(fn); },
  };

  // Inputs: one delegated listener per DOM event type the skeleton mentions.
  // `enter` is keydown filtered to the Enter key.
  const DOM_TYPE = { enter: "keydown" };
  const fires = (t, e) => (DOM_TYPE[t] ?? t) === e.type && (t !== "enter" || (e.key === "Enter" && !e.isComposing));
  // Templates are inert, so their inputs are scanned explicitly: a family
  // grown later must find its listeners already in place.
  const declared = [...root.querySelectorAll("[data-on]"),
    ...[...root.querySelectorAll("template")].flatMap((t) => [...t.content.querySelectorAll("[data-on]")])];
  const types = new Set();
  for (const el of declared)
    for (const entry of el.dataset.on.trim().split(/\s+/)) types.add(DOM_TYPE[entry.split(":")[0]] ?? entry.split(":")[0]);
  for (const type of types) {
    root.addEventListener(type, (e) => {
      const el = e.target.closest?.("[data-on]");
      if (!el) return;
      const field = el.matches("input, textarea, select") ? el : null;
      const member = /-(\d+)$/.exec(el.closest("[data-fold]")?.dataset.fold ?? "");
      const index = member ? Number(member[1]) : -1;
      for (const entry of el.dataset.on.trim().split(/\s+/)) {
        const [t, input] = entry.split(":");
        if (!fires(t, e) || !inputs.has(input)) continue;
        e.preventDefault();
        host.dispatch(input, field ? field.value : "", index);
        if (t === "enter" && field) field.value = "";
      }
    });
  }

  return host.apply(app.render_at(0));
}
