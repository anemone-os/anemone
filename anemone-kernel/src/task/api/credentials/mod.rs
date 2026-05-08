//! Task credentials.
//!
//! In Anemone, there is only a root user (with UID 0 and GID 0), and all tasks
//! run with root privileges.

pub mod getgid;
pub mod getuid;
pub mod setgid;
pub mod setuid;

// TODO: e[x]id.
