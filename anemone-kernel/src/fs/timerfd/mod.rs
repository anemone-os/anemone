//! Anonymous timerfd files.
//!
//! Timerfd readiness is owned by the private timerfd state. The anonymous inode
//! only provides a stable fd identity; timer expiration, blocking reads, poll
//! readiness, and logical cancellation all live in `TimerFdCore`.

mod abi;
mod api;
mod core;

use self::core::{TimerFdClock, create_timerfd, gettime, settime};
