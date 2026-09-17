//! Writes `www/gen/bench.css` and `www/gen/bench.manifest.mjs` from the
//! component's declaration on every build.

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../www/gen");
    bench::ui::MANIFEST
        .write_fragments(&dir, "bench")
        .expect("write www/gen/bench.*");
}
