//! signal-related system call and api
//!
//! References:
//! - https://www.man7.org/linux/man-pages/man2/rt_sigqueueinfo.2.html
//! - https://www.man7.org/linux/man-pages/man2/rt_sigaction.2.html
//! - https://www.man7.org/linux/man-pages/man2/rt_sigtimedwait.2.html
//! - https://www.man7.org/linux/man-pages/man2/rt_sigprocmask.2.html

pub mod rt_sigaction;
pub mod rt_sigpending;
pub mod rt_sigprocmask;
pub mod rt_sigqueueinfo;
pub mod rt_sigreturn;
pub mod sigaltstack;
pub mod tgkill;
