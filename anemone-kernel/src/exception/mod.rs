//! Exception handling module, responsible for managing CPU exceptions and
//! interrupts, including:
//! - hardware exceptions (e.g. page faults, general protection faults, etc.)
//! - interrupts (e.g. timer interrupts, keyboard interrupts, etc.)
//! - bottom halves (e.g. deferred work that needs to be done after an interrupt
//!   is handled)

mod page_fault;
pub use page_fault::{PageFaultInfo, PageFaultType, handle_kernel_page_fault};
mod ipi;
pub use ipi::{
    IpiPayload, TlbShootdownGuard, broadcast_ipi, broadcast_ipi_async, handle_ipi, send_ipi,
    send_ipi_async,
};
mod timer;
pub use timer::handle_timer_interrupt;

pub mod intr;
pub mod trap;
