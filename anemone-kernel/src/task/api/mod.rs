// TODO: better clone.
pub mod clone;
pub mod credentials;
pub mod execve;
pub mod exit;
pub mod futex;
pub mod getpid;
pub mod getppid;
pub mod gettid;
pub mod set_tid_address;
// prevent ambiguous name resolution of "resource".
#[path = "resource/mod.rs"]
pub mod task_resource;
pub mod wait4;
