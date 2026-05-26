//! Task credentials.
//!
//! In Anemone, there is only a root user (with UID 0 and GID 0) (in other
//! words, all tasks run with root privileges).
//!
//! So this module is mostly for compatibility with POSIX.

mod api;
pub use api::*;
mod id;
pub use id::*;
