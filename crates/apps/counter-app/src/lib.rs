//! The `CounterApp` bundle: one component, one wasm module, its own JS glue.
//! The second line adds the timeline's methods; a page without devtools
//! drops it. Built by `scripts/build-www.sh` into `www/pkg/counter-app/`.

logfold_web::export_component!(
    CounterApp,
    counter::Counter,
    counter::View,
    counter::component()
);
logfold_web::export_devtools!(CounterApp);
