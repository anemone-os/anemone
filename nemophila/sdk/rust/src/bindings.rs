//! WIT-generated lowering used by [`crate::export_module!`].
//!
//! This module is public only because Rust cross-crate macro expansion must
//! resolve the generated export glue from the consuming module crate. It is
//! not the supported module-author API or a source of runtime policy.

wit_bindgen::generate!({
    path: "../../wit",
    world: "anemone:nemophila/module@0.1.0",
    pub_export_macro: true,
    default_bindings_module: "$crate::__bindings",
    // Nemophila has one explicit lifecycle entry. The default bindgen
    // workaround would run static constructors before every export,
    // including a callback invoked before `load`.
    disable_run_ctors_once_workaround: true,
});
