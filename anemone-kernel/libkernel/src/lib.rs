#![doc = include_str!("../README.md")]
#![cfg_attr(not(test), no_std)]

// the naming here is a bit weird, but it allows us to have a clean separation
// between the kernel and the library code, otherwise namespace pollution would
// be a problem. (e.g. how can we export both libkernel::mm and kernel::mm
// without causing confusion?)

// all top-level hal trait are suffixed with "Trait" to avoid conflicts with the
// arch-specific implementations.

#[path = "debug/mod.rs"]
pub mod libdebug;
#[path = "device/mod.rs"]
pub mod libdevice;
#[path = "driver/mod.rs"]
pub mod libdriver;
#[path = "exception/mod.rs"]
pub mod libexception;
#[path = "mm/mod.rs"]
pub mod libmm;
#[path = "power.rs"]
pub mod libpower;
#[path = "sched/mod.rs"]
pub mod libsched;
#[path = "sync/mod.rs"]
pub mod libsync;
#[path = "syscall/mod.rs"]
pub mod libsyscall;
#[path = "time/mod.rs"]
pub mod libtime;
pub mod utils;
