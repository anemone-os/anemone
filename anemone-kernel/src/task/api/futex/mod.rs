//! Futex-related system calls.
//!
//! T.B.D.
//!
//! References:
//! - https://www.man7.org/linux/man-pages/man2/set_robust_list.2.html
//! - https://www.man7.org/linux/man-pages/man2/futex.2.html

pub mod futex;
pub mod get_robust_list;
pub mod set_robust_list;
