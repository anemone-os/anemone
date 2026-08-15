#![no_std]
#![feature(alloc_error_handler)]

use core::{alloc::Layout, panic::PanicInfo};
use nemophila_sdk::{LoadContext, Module};

#[global_allocator]
static ALLOCATOR: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

/// Validation-only artifact for the tracked required-boot negative targets.
/// Its load error must roll back unpublished state and stop before userspace.
struct BootReject;

impl Module for BootReject {
    type Error = ();

    fn load(_context: &mut LoadContext<'_>) -> Result<(), Self::Error> {
        Err(())
    }
}

nemophila_sdk::export_module!(BootReject, lifecycle);

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

#[alloc_error_handler]
fn allocation_error(_layout: Layout) -> ! {
    core::arch::wasm32::unreachable()
}
