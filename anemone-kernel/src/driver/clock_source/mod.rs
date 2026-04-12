//! Clock sources are used to provide a timebase for the kernel. They are used
//! by the scheduler to determine when to switch tasks, and by the timer
//! interrupt handler to determine how long to wait before the next interrupt.
//!
//! In our current design, this module is actually unused. Since our supported
//! architectures (riscv and loongarch) both have their clock sources exposed as
//! CSRs, which can be accessed directly by the kernel without any additional
//! hardware support. That's why they are implemented as
//! [crate::time::LocalClockSourceArch] traits instead of device-level
//! instances.
//!
//! But, we leave this module here for potential future use, when we might want
//! to support architectures that require a more traditional clock source
//! implementation, or when we want to implement additional features that
//! require a more flexible clock source subsystem.
