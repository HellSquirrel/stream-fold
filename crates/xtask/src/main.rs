//! `cargo xtask gen`: write each component's contract for its page,
//! `www/gen/<app>.css` (typed `@property` registrations and constants) and
//! `www/gen/<app>.manifest.mjs` (names, inputs, value tables) from the
//! `slots!` declaration. A test in each app fails when these are stale.

use std::fs;
use std::path::PathBuf;

use logfold_core::Manifest;

fn main() {
    let cmd = std::env::args().nth(1).unwrap_or_default();
    match cmd.as_str() {
        "gen" => generate(),
        _ => {
            eprintln!("usage: cargo xtask gen");
            std::process::exit(2);
        }
    }
}

fn generate() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = root.join("www/gen");
    fs::create_dir_all(&out).expect("www/gen");
    let apps: [(&str, &Manifest); 2] = [
        ("studio", &studio::ui::MANIFEST),
        ("todo", &todo::ui::MANIFEST),
    ];
    for (name, m) in apps {
        fs::write(out.join(format!("{name}.css")), m.css()).expect("write css");
        fs::write(out.join(format!("{name}.manifest.mjs")), m.mjs()).expect("write mjs");
        println!("wrote www/gen/{name}.css and www/gen/{name}.manifest.mjs");
    }
}
