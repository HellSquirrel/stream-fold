//! The `StudioApp` bundle: one component, one wasm module, its own JS glue.
//! The second line adds the timeline's methods; a page without devtools
//! drops it. Built by `scripts/build-www.sh` into `www/pkg/studio-app/`.

logfold_web::export_component!(StudioApp, studio::App, studio::State, studio::component());
logfold_web::export_devtools!(StudioApp);
