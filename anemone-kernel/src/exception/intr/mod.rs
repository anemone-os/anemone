//! Irq subsystem along with some helpers related to interrupt handling.

mod hal;
pub use hal::*;

mod scoped;
pub use scoped::{IntrGuard, with_intr_disabled, with_intr_enabled};

mod irq;
pub use irq::*;
