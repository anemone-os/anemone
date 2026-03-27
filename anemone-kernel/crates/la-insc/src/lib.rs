//! Basic Loongarch64 instruction support, containing privileged instructions
//! and some csr/iocsr registers. Only used for kernel.
//! `[deny(missing_docs)]` is used to ensure all instructions and registers are
//! documented.

#![deny(missing_docs)]
#![feature(stdarch_loongarch)]
#![no_std]
pub mod insc;
pub mod reg;
pub mod utils;
