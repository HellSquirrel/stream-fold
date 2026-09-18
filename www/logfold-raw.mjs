// The raw loader: instantiates a module built without wasm-bindgen and
// presents the same method names as a generated class, so `mount` and
// `timeline` do not know the difference. Names are decoded once per id
// from linear memory; a patch is read in place as a Float64Array view.

const KINDS = ["input", "tick", "sense", "io", "started"];

export async function rawApp(url) {
  const { instance } = await WebAssembly.instantiateStreaming(fetch(url), {});
  const ex = instance.exports;
  const dec = new TextDecoder();
  ex.lf_init();
  const str = (p, n) => (n ? dec.decode(new Uint8Array(ex.memory.buffer, p, n)) : undefined);
  const name = (id) => str(ex.lf_name_ptr(id), ex.lf_name_len(id));
  const input = (i) => str(ex.lf_input_ptr(i), ex.lf_input_len(i));
  // A view, valid until the next call into the module: `mount` applies it synchronously.
  const patch = (n) => new Float64Array(ex.memory.buffer, ex.lf_patch_ptr(), n);
  // An input's text goes into the module's scratch buffer first. Take the
  // memory view after `lf_scratch`: growing memory detaches the old buffer.
  const dispatch = (i, index, bytes) => {
    const n = bytes?.length ?? 0;
    if (n) new Uint8Array(ex.memory.buffer, ex.lf_scratch(n), n).set(bytes);
    return patch(ex.lf_dispatch(i, index, n));
  };
  return {
    input_names: () => Array.from({ length: ex.lf_input_count() }, (_, i) => input(i)),
    name,
    dispatch,
    text: (i) => str(ex.lf_text_ptr(i), ex.lf_text_len(i)),
    tick: (ms) => patch(ex.lf_tick(ms)),
    frame: (dt) => patch(ex.lf_frame(dt)),
    render_at: (n) => patch(ex.lf_render_at(n)),
    at: () => ex.lf_at(),
    len: () => ex.lf_len(),
    base: () => ex.lf_base(),
    checkpoint_for: (n) => ex.lf_checkpoint_for(n),
    kind: (i) => KINDS[ex.lf_kind(i)],
    tick_ms: (i) => ex.lf_tick_ms(i),
  };
}
