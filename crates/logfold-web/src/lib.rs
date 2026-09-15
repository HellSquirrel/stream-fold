//! The generic browser host.
//!
//! A [`Component`](logfold_core::Component) becomes a wasm class with the
//! whole boundary API through [`Host`] and one `export_component!` line in
//! an app crate of its own (`crates/apps/*`), so every page loads only its
//! app. The page mounts it with `www/logfold.mjs`, and the skeleton's CSS
//! does the rendering. See `docs/boundary.md`.
//!
//! Checkpoint policy lives here, in the host. The core never decides when.

use logfold_core::Checkpoints;

pub mod host;
pub mod raw;

pub use host::Host;
#[cfg(feature = "bindgen")]
pub use wasm_bindgen;

/// A small allocator for every bundle. dlmalloc, Rust's default on wasm,
/// is 8 KB of code; these apps allocate small, short-lived things.
#[cfg(target_arch = "wasm32")]
#[global_allocator]
static ALLOC: talc::TalckWasm = unsafe { talc::TalckWasm::new_global() };

/// The one cadence rule hosts use: take a checkpoint once `every` events
/// have landed since the last one. Counting from the last checkpoint
/// rather than from zero keeps the rule honest when a single user action
/// appends several events.
pub fn due<X: Clone>(checkpoints: &Checkpoints<X>, n: usize, every: usize) -> bool {
    n - checkpoints.nearest(n).map_or(0, |(upto, _)| upto) >= every
}

/// A JS string handle. Outside wasm there is no JS, so native tests get
/// `undefined`; nothing in the core ever looks inside a label.
#[cfg(feature = "bindgen")]
pub fn label(s: &'static str) -> wasm_bindgen::JsValue {
    #[cfg(target_arch = "wasm32")]
    {
        wasm_bindgen::JsValue::from_str(s)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = s;
        wasm_bindgen::JsValue::UNDEFINED
    }
}
