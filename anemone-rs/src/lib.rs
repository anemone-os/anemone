#![doc = include_str!("../README.md")]
#![no_std]

pub use anemone_abi::errno::Errno;
pub extern crate alloc;
pub use anemone_rs_macros::main;

mod allocator;
pub mod console;
pub mod fs;
pub mod process;
pub mod runtime;
pub mod syscalls;

pub mod prelude {
    pub use crate::{
        Errno, alloc, args,
        console::{__eprint, __print},
        eprint, eprintln, main, print, println,
        process::{execve, exit, sched_yield},
    };

    pub use alloc::{format, string::String, vec, vec::Vec};
}
pub use prelude::*;

pub use runtime::{Args, args};
