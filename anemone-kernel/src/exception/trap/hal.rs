use core::fmt::Debug;

pub trait TrapArchTrait {
    type TrapFrame: TrapFrameArch;

    unsafe fn load_utrapframe(trapframe: Self::TrapFrame) -> !;
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

    /// Advance the program counter to the next instruction.
    ///
    /// Usually used by system call handling.
    fn advance_pc(&mut self);

    /// Set the stack pointer in the trap frame.
    fn set_sp(&mut self, sp: u64);

    /// Set thread local storage pointer in the trap frame.
    fn set_tls(&mut self, tls: u64);

    /// Set the scratch register. (e.g. sscratch in RiscV, save0 in LoongArch)
    ///
    /// **Note that on all architectures, this register is always used to store
    /// kernel stack of a user task.**
    fn set_scratch(&mut self, scratch: u64);

    /// Set the n-th argument register according to C ABI.
    fn set_arg<const IDX: usize>(&mut self, arg: u64);

    /// Set the return value of the system call.
    ///
    /// # Safety
    ///
    /// This is only meaningful in a syscall context, and the behavior is
    /// undefined otherwise.
    unsafe fn set_syscall_ret_val(&mut self, retval: u64);
}
