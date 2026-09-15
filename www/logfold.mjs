// LogFold host shim. Generic: it knows targets, variables and inputs by
// name and never what they mean. One `mount` per component on a page.
//
// The skeleton names targets with `data-fold="name"` (the root is "root")
// and inputs with `data-on="click:toggle"`; several entries may be
// separated by spaces. Rust decides what an input means and which numbers
// land on which target; the stylesheet decides what the numbers look like.

export function mount(app, { root = document, manifest = null } = {}) {
  // Names: from the manifest when the component declared one (ids are fixed
  // by declaration order), else asked once per id and cached as a handle.
  const names = new Map();
  const name = (id) => {
    if (!names.has(id)) names.set(id, manifest ? manifest.names[id] : app.name(id));
    return names.get(id);
  };
  const targets = new Map();
  const target = (id, index) => {
    const key = index < 0 ? id : `${id}:${index}`;
    if (!targets.has(key)) {
      const n = name(id);
      targets.set(key, n === "root" ? document.documentElement
        : root.querySelector(`[data-fold="${index < 0 ? n : `${n}-${index}`}"]`));
    }
    return targets.get(key);
  };
  const inputs = new Map((manifest ? manifest.inputs : app.input_names()).map((n, i) => [n, i]));
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
     *  family member or -1; kind 0 is a custom property, 1 an attribute;
     *  NaN clears. */
    apply(patch) {
      for (let i = 0; i < patch.length; i += 5) {
        const el = target(patch[i], patch[i + 1]), kind = patch[i + 2], n = name(patch[i + 3]), x = patch[i + 4];
        if (!el) continue;
        const v = Number.isNaN(x) ? null : spell(kind, n, x);
        if (v === null) { kind ? el.removeAttribute(n) : el.style.removeProperty(n); }
        else { kind ? el.setAttribute(n, v) : el.style.setProperty(n, v); }
        if (name(patch[i]) === "root") { Number.isNaN(x) ? vars.delete(n) : vars.set(n, x); }   // the mirror is for devtools; root only
      }
      for (const fn of listeners) fn(host);
      return host;
    },
    dispatch(input) {
      if (!inputs.has(input)) throw new Error(`unknown input "${input}"; known: ${[...inputs.keys()].join(", ")}`);
      return host.apply(app.dispatch(inputs.get(input)));
    },
    tick(ms = performance.now()) { return host.apply(app.tick(ms)); },
    /** One frame of the component's simulated world, `dt` milliseconds long. */
    frame(dt) { return host.apply(app.frame(dt)); },
    renderAt(n) { return host.apply(app.render_at(n)); },
    onChange(fn) { listeners.add(fn); return () => listeners.delete(fn); },
  };

  // Inputs: one delegated listener per event type the skeleton mentions.
  const types = new Set();
  for (const el of root.querySelectorAll("[data-on]"))
    for (const entry of el.dataset.on.trim().split(/\s+/)) types.add(entry.split(":")[0]);
  for (const type of types) {
    root.addEventListener(type, (e) => {
      const el = e.target.closest?.("[data-on]");
      if (!el) return;
      for (const entry of el.dataset.on.trim().split(/\s+/)) {
        const [t, input] = entry.split(":");
        if (t === type && inputs.has(input)) { e.preventDefault(); host.dispatch(input); }
      }
    });
  }

  return host.apply(app.render_at(0));
}
