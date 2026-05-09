pub trait TrapArchTrait {
    type TrapFrame: TrapFrameArch;
    type SyscallCtx: SyscallCtxArch;

    unsafe fn load_utrapframe(trapframe: Self::TrapFrame) -> !;

    /// Get a syscall context from the given trap frame.
    ///
    /// The returned [Self::SyscallCtx] is said to be a snapshot. Modifying it
    /// does not change the original trap frame. For the latter,
    /// [Self::TrapFrame] implements [SyscallCtxArch] as well.
    fn syscall_ctx_snapshot(trapframe: &Self::TrapFrame) -> Self::SyscallCtx;

    /// Restore the syscall context to the given trap frame.
    ///
    /// Mainly used to restart a system call after handling signals.
    fn restore_syscall_ctx(trapframe: &mut Self::TrapFrame, syscall_ctx: &Self::SyscallCtx);
}

pub trait TrapFrameArch: SyscallCtxArch {
    const ZEROED: Self;

    /// Get the stack pointer from the trap frame.
    fn sp(&self) -> u64;

    /// Set the stack pointer in the trap frame.
    fn set_sp(&mut self, sp: u64);

    /// Set thread local storage pointer in the trap frame.
    fn set_tls(&mut self, tls: u64);

    /// Set the scratch register. (e.g. sscratch in RiscV, save0 in LoongArch)
    ///
    /// **Note that on all architectures, this register is always used to store
    /// kernel stack of a user task.**
    fn set_scratch(&mut self, scratch: u64);

    /// Set the n-th argument register, according to C calling convention.
    ///
    /// Used by kernel thread context initialization.
    fn set_arg<const IDX: usize>(&mut self, arg: u64);
}

/// Syscall register context, which contains:
/// - argument registers,
/// - syscall number register, and
/// - program counter.
///
/// Some of them may overlap with each other, on almost all architectures (e.g.
/// syscall number register and a argument register). But it should be fine if
/// used correctly.
pub trait SyscallCtxArch: Clone + Copy {
    /// Get the system call argument at the given index.
    fn syscall_arg<const IDX: usize>(&self) -> u64;

    /// Set the system call argument at the given index.
    fn set_syscall_arg<const IDX: usize>(&mut self, arg: u64);

    /// Get the system call number.
    fn syscall_no(&self) -> usize;

    /// Set the system call number.
    fn set_syscall_no(&mut self, sysno: usize);

    /// Get the program counter from the syscall context.
    fn syscall_pc(&self) -> u64;

    /// Advance the program counter from the current system call instruction to
    /// the next instruction.
    fn advance_syscall_pc(&mut self);

    /// Get the return value of the system call.
    fn syscall_retval(&self) -> u64;

    /// Set the return value of the system call.
    fn set_syscall_retval(&mut self, retval: u64);
}
