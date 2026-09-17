//! Writes the page's side of the contract, `www/gen/todo.css` and
//! `www/gen/todo.manifest.mjs`, from the component's declaration on every
//! build, so the fragments are exactly as fresh as the bundle.

fn main() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../www/gen");
    todo::ui::MANIFEST
        .write_fragments(&dir, "todo")
        .expect("write www/gen/todo.*");
}
