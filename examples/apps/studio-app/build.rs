//! Writes the page's side of the contract, `www/gen/studio.css` and
//! `www/gen/studio.manifest.mjs`, from the component's declaration on every
//! build, so the fragments are exactly as fresh as the bundle.

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../www/gen");
    studio::ui::MANIFEST
        .write_fragments(&dir, "studio")
        .expect("write www/gen/studio.*");
}
