//! WIT-generated export shape for modules with no callback entry.

wit_bindgen::generate!({
    path: "../../wit",
    world: "anemone:nemophila/lifecycle-module@0.1.0",
    pub_export_macro: true,
    default_bindings_module: "$crate::__lifecycle_bindings",
    disable_run_ctors_once_workaround: true,
});
