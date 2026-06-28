//! POSIX interval timers.
//!
//! Obsolete in POSIX.1-2008. But for compatibility with existing software, we
//! still support it.

mod api;
pub use api::*;
