//! The `LikeApp` bundle: one component, one wasm module, its own JS glue.
//! The second line adds the timeline's methods; a page without devtools
//! drops it. Built by `scripts/build-www.sh` into `www/pkg/like-app/`.

logfold_web::export_component!(
    LikeApp,
    like_local::Like,
    like_local::View,
    like_local::component()
);
logfold_web::export_devtools!(LikeApp);
