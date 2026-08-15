#![no_std]

extern crate alloc;

#[doc(hidden)]
#[path = "bindings.rs"]
pub mod __bindings;
#[doc(hidden)]
#[path = "lifecycle_bindings.rs"]
pub mod __lifecycle_bindings;
#[doc(hidden)]
#[path = "export.rs"]
pub mod __private;
#[doc(hidden)]
#[path = "task_lifecycle_bindings.rs"]
pub mod __task_lifecycle_bindings;
mod call;
mod lifecycle;
pub mod services;
pub mod weave;

pub use call::CallbackContext;
pub use lifecycle::{LoadContext, Module};

/// Formats and emits one raw console-only fragment through a [`Logging`]
/// capability.
///
/// [`Logging`]: crate::services::logging::Logging
#[macro_export]
macro_rules! kprint {
    ($logging:expr, $($arg:tt)*) => {
        $logging.print_fmt(format_args!($($arg)*));
    };
}

/// Formats and emits one raw console-only line through a [`Logging`]
/// capability.
///
/// [`Logging`]: crate::services::logging::Logging
#[macro_export]
macro_rules! kprintln {
    ($logging:expr) => {
        $logging.println("");
    };
    ($logging:expr, $($arg:tt)*) => {
        $logging.println_fmt(format_args!($($arg)*));
    };
}

/// Exports one Rust module through a WIT-derived module world.
///
/// The default form preserves the clone-only R0 source surface. The named
/// forms select another real WIT world without forcing unrelated callback
/// exports into every artifact.
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
    ($module:ty, lifecycle) => {
        struct __NemophilaModuleGuest;

        impl $crate::__private::LifecycleGuest for __NemophilaModuleGuest {
            fn load() -> Result<(), ()> {
                $crate::__private::load::<$module>()
            }
        }

        $crate::__lifecycle_bindings::export!(
            __NemophilaModuleGuest with_types_in $crate::__lifecycle_bindings
        );
    };
    ($module:ty, task_lifecycle) => {
        struct __NemophilaModuleGuest;

        impl $crate::__private::TaskLifecycleGuest for __NemophilaModuleGuest {
            fn load() -> Result<(), ()> {
                $crate::__private::load::<$module>()
            }

            fn observe_clone(creator_tid: u32, child_tid: u32) {
                $crate::__private::observe_clone(creator_tid, child_tid)
            }

            fn observe_thread_exit(tid: u32, signaled: bool, value: u32) {
                $crate::__private::observe_thread_exit(tid, signaled, value)
            }
        }

        $crate::__task_lifecycle_bindings::export!(
            __NemophilaModuleGuest with_types_in $crate::__task_lifecycle_bindings
        );
    };
}
