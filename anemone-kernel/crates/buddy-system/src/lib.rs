#![cfg_attr(not(test), no_std)]
#![allow(dead_code)]

#[macro_use]
mod macros;

mod adapter;
mod aligned;
mod bitmap;
mod zone;

pub mod error;
#[cfg(feature = "stats")]
pub mod stats;
mod system;

pub use aligned::AlignedAddr;
pub use error::BuddyError;
pub use system::BuddySystem;
