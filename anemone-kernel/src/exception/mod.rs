//! Exception handling module, responsible for managing CPU exceptions and
//! interrupts, including:
//! - hardware exceptions (e.g. page faults, general protection faults, etc.)
//! - interrupts (e.g. timer interrupts, keyboard interrupts, etc.)
//! - bottom halves (e.g. deferred work that needs to be done after an interrupt
//!   is handled)

mod preempt_counter;
pub use preempt_counter::PreemptCounter;
mod page_fault;
pub use page_fault::{PageFaultInfo, PageFaultType};

pub mod intr;
pub mod trap;
