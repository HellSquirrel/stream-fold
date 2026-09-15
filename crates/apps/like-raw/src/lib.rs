//! The like button through the raw boundary. Built by `scripts/build-www.sh`
//! with plain `cargo` and `wasm-opt`; loaded by `www/logfold-raw.mjs`.

logfold_web::export_raw!(like_local::Like, like_local::View, like_local::component());
