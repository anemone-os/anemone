#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

use alloc::format;
use core::{alloc::Layout, panic::PanicInfo};
use nemophila_sdk::{LoadContext, Module, weave::task::clone_observer::RegistrationError};

#[global_allocator]
static ALLOCATOR: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

struct CloneObserver;

impl Module for CloneObserver {
    type Error = RegistrationError;

    fn load(context: &mut LoadContext<'_>) -> Result<(), RegistrationError> {
        context
            .weave()
            .task()
            .clone_observer()
            .register(|event, callback| {
                callback.logging().println(&format!(
                    "clone creator={} child={}",
                    event.creator_tid, event.child_tid
                ));
            })?;

        context.logging().println("clone observer registered");
        Ok(())
    }
}

nemophila_sdk::export_module!(CloneObserver);

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    core::arch::wasm32::unreachable()
}

#[alloc_error_handler]
fn allocation_error(_layout: Layout) -> ! {
    core::arch::wasm32::unreachable()
}
