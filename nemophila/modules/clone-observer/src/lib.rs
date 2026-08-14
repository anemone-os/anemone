#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

use alloc::{format, rc::Rc, string::String, vec::Vec};
use core::{alloc::Layout, cell::Cell, panic::PanicInfo};
use nemophila_sdk::{LoadContext, LogLevel, Module, RegistrationError};

#[global_allocator]
static ALLOCATOR: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

struct CloneObserver;

impl Module for CloneObserver {
    fn load(context: &mut LoadContext<'_>) -> Result<(), RegistrationError> {
        let callback_released = Rc::new(Cell::new(false));
        let release_probe = ReleaseProbe(callback_released.clone());
        let mut prefix = Vec::from("clone creator=".as_bytes());
        prefix.reserve(16);

        let registration =
            context
                .weave()
                .task()
                .clone_observer()
                .register(move |event, callback| {
                    let _retain_until_instance_destroy = &release_probe;
                    let mut message = String::from_utf8(prefix.clone()).expect("ASCII prefix");
                    message.push_str(&format!("{} child={}", event.creator_tid, event.child_tid));
                    callback.logging().write(LogLevel::Info, &message);
                });

        if let Err(error) = registration {
            // The SDK must destroy the pending environment before returning a
            // dynamic registration failure to module code.
            assert!(callback_released.get());
            let message = match error {
                RegistrationError::ProviderUnavailable => {
                    "clone provider unavailable; callback environment released"
                },
                RegistrationError::AlreadyRegistered => {
                    "clone observer already registered; callback environment released"
                },
            };
            context.logging().write(LogLevel::Debug, message);
            return Err(error);
        }
        Ok(())
    }
}

struct ReleaseProbe(Rc<Cell<bool>>);

impl Drop for ReleaseProbe {
    fn drop(&mut self) {
        self.0.set(true);
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
