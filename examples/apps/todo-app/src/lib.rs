//! The `TodoApp` bundle: the todo component as its own wasm module. This
//! file never grows; the component lives in `examples/todo`. Built by
//! `scripts/build-www.sh todo-app` into `www/pkg/todo-app/`.

logfold_web::export_component!(TodoApp, todo::Todo, todo::View, todo::component());
logfold_web::export_devtools!(TodoApp);
