use crate::{libexception::PageFaultInfo, libsyscall::SysNo};

pub trait TrapArchTrait {
    type TrapFrame: TrapFrameArch;
}

pub trait TrapFrameArch {
    /// Get the system call argument at the given index.
    ///
    /// # Safety
    ///
    /// This is only meaningful when the trap reason is
    /// [ExceptionReason::Syscall], and the behavior is undefined otherwise.
    unsafe fn syscall_args<const IDX: usize>(&self) -> usize;
    fn advance_pc(&mut self);
}

/// The abstract reason for a trap.
///
/// Some architectures may regard Syscall as a special kind of exception, while
/// others may treat it as a separate category. For uniformity, we take the
/// former approach.
///
/// For [TrapReason::Exception] handling, current kernel stack is used, while
/// for [TrapReason::Interrupt] handling, separate interrupt stacks are used.
/// This convention must be adhered to by architecture-specific trap handlers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapReason {
    Exception(ExceptionReason),
    Interrupt(InterruptReason),
}

/// General exceptions that can occur on almost any architecture.
///
/// For some highly architecture-specific exceptions, we don't enumerate them
/// here and instead let architecture-specific code handle them directly, or
/// parse them to some more general exception reasons. (e.g. both
/// DivisionByZero)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExceptionReason {
    Syscall(SysNo),
    Breakpoint,
    PageFault(PageFaultInfo),
    DivisionByZero,
    InvalidOpcode,
    /// Unrecoverable and architecture-specific fatal exception, e.g. triple
    /// fault on x86_64, load/store alignment fault on RISC-V, etc.
    ///
    /// For traps from user space, this will lead to process termination, while
    /// for traps from kernel space, this will cause a kernel panic. If the
    /// latter is the case, the architecture-specific trap handler is
    /// responsible for logging necessary information for debugging before
    /// halting the system.
    ArchFatal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptReason {
    Timer,
    External,
    Ipi,
}
