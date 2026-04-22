#![doc = include_str!("../README.md")]
#![no_std]

pub extern crate alloc;

mod allocator;
mod sys;

pub mod env;
pub mod fs;
pub mod io;
pub mod os;
pub mod process;
pub mod runtime;

// TODO: mod path.

pub mod prelude;
pub use anemone_abi as abi;
pub use anemone_rs_macros::main;

unsafe extern "Rust" {
    /// Entrypoint for main function.
    fn anemone_main() -> Result<(), anemone_abi::errno::Errno>;
}
