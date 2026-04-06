use core::fmt::Debug;

pub trait TrapArchTrait {
    type TrapFrame: TrapFrameArch;
}

pub trait TrapFrameArch: Debug + Clone {
    const ZEROED: Self;
    /// Get the system call argument at the given index.
    ///
    /// # Safety
    ///
    /// This is only meaningful in a syscall context, and the behavior is
    /// undefined otherwise.
    unsafe fn syscall_args<const IDX: usize>(&self) -> u64;

    /// Get the system call number.
    ///
    /// # Safety
    ///
    /// This is only meaningful in a syscall context, and the behavior is
    /// undefined otherwise.
    unsafe fn syscall_no(&self) -> usize;

    fn advance_pc(&mut self);

    /// Set the return value of the system call.
    ///
    /// # Safety
    ///
    /// This is only meaningful in a syscall context, and the behavior is
    /// undefined otherwise.
    unsafe fn set_syscall_ret_val(&mut self, retval: u64);
}
