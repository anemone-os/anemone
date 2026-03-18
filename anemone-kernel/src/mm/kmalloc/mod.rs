//! Kernel memory allocator.
//! Mostly kernel heap management, but also some temporary allocations for
//! bootstrapping and other early-stage code.

use crate::{mm::kmalloc::allocator::KernelAllocator, prelude::*};

pub mod allocator;

#[global_allocator]
static KERNEL_ALLOCATOR: KernelAllocator = KernelAllocator::new();
