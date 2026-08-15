//! WIT-generated export shape for clone plus thread-exit observers.

wit_bindgen::generate!({
    path: "../../wit",
    world: "anemone:nemophila/task-lifecycle-module@0.1.0",
    pub_export_macro: true,
    default_bindings_module: "$crate::__task_lifecycle_bindings",
    disable_run_ctors_once_workaround: true,
});
