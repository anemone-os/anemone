//! System Power Subsystem.
//!
//! TODO: use intrusive linked list to store the handlers to avoid heap
//! allocation.

use spin::Lazy;

use crate::prelude::*;

pub trait PowerOffHandler: Send {
    unsafe fn poweroff(&self) -> !;
}

pub trait RebootHandler: Send {
    unsafe fn reboot(&self) -> !;
}

static POWER_OFF_HANDLER: Lazy<SpinLock<Vec<Box<dyn PowerOffHandler>>>> =
    Lazy::new(|| SpinLock::new(Vec::new()));

static REBOOT_HANDLER: Lazy<SpinLock<Vec<Box<dyn RebootHandler>>>> =
    Lazy::new(|| SpinLock::new(Vec::new()));

/// Register a power off handler to be called when the system is powered off.
pub fn register_power_off_handler(handler: Box<dyn PowerOffHandler>) {
    POWER_OFF_HANDLER.lock_irqsave().push(handler);
}

/// Register a reboot handler to be called when the system is rebooted.
pub fn register_reboot_handler(handler: Box<dyn RebootHandler>) {
    REBOOT_HANDLER.lock_irqsave().push(handler);
}

/// Power off the system.
///
/// Internally, this function will call all registered power off handlers in
/// order until one of them succeeds. If no handler succeeds, it will panic.
pub unsafe fn power_off() -> ! {
    let handlers = POWER_OFF_HANDLER.lock_irqsave();
    for handler in handlers.iter() {
        unsafe {
            handler.poweroff();
        }
    }
    kemergln!("no power off handler succeeded, halting the system");
    loop {
        core::hint::spin_loop();
    }
}

/// Reboot the system.
///
/// Internally, this function will call all registered reboot handlers in
/// order until one of them succeeds. If no handler succeeds, it will panic.
pub unsafe fn reboot() -> ! {
    let handlers = REBOOT_HANDLER.lock_irqsave();
    for handler in handlers.iter() {
        unsafe {
            handler.reboot();
        }
    }
    kemergln!("no reboot handler succeeded, halting the system");
    loop {
        core::hint::spin_loop();
    }
}
