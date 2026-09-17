//! The `BenchApp` bundle. Built by `scripts/build-www.sh bench-app` into
//! `www/pkg/bench-app/`.

logfold_web::export_component!(BenchApp, bench::Bench, bench::State, bench::component());
logfold_web::export_devtools!(BenchApp);
