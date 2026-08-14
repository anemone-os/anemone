#![no_std]

extern crate alloc;

#[doc(hidden)]
#[path = "bindings.rs"]
pub mod __bindings;
#[doc(hidden)]
#[path = "export.rs"]
pub mod __private;
mod call;
mod lifecycle;
pub mod services;
pub mod weave;

pub use call::CallbackContext;
pub use lifecycle::{LoadContext, Module};

/// Exports one Rust module through the fixed WIT-derived load and callback
/// entries.
#[macro_export]
macro_rules! export_module {
    ($module:ty) => {
        struct __NemophilaModuleGuest;

        impl $crate::__private::Guest for __NemophilaModuleGuest {
            fn load() -> Result<(), ()> {
                $crate::__private::load::<$module>()
            }

            fn observe_clone(creator_tid: u32, child_tid: u32) {
                $crate::__private::observe_clone(creator_tid, child_tid)
            }
        }

        $crate::__bindings::export!(__NemophilaModuleGuest with_types_in $crate::__bindings);
    };
}
