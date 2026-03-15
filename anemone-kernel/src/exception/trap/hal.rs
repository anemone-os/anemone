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
