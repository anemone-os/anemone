use crate::prelude::*;

/// Task context switching.
pub trait SchedArchTrait {
    /// Task Context type
    type TaskContext: TaskContextArch;
    /// Switch from the current task to the next task. Saves and loads the
    /// callee-saved registers.
    ///
    /// **This function does not switch the MemSpace**,
    /// because a [TaskContext] does not necessarily have a corresponding
    /// [Task].     It may point to the context of the scheduling loop.
    /// Therefore, the operation of switching the MemSpace should be
    /// performed by the scheduling system.
    ///
    /// **Must be called with interrupts disabled.**
    unsafe fn switch(cur: *mut TaskContext, next: *const TaskContext, save_fr: bool, load_fr: bool);
}

pub trait TaskContextArch {
    const ZEROED: Self;
    fn from_kernel_fn(entry: VirtAddr, stack_top: VirtAddr, args: ParameterList) -> Self;
    fn from_user_fn(entry: VirtAddr, ustack_top: VirtAddr, kstack_top: VirtAddr) -> Self;
    fn pc(&self) -> u64;
    fn sp(&self) -> u64;
}

pub struct ParameterList {
    args: [u64; 7],
}

impl ParameterList {
    pub const fn empty() -> Self {
        Self { args: [0; 7] }
    }

    pub const fn new<const N: usize>(args: &[u64; N]) -> Self {
        const_assert!(N <= 7, "ParameterList can only hold up to 7 arguments");
        let mut args_array = [0; 7];
        let mut i = 0;
        while i < N {
            args_array[i] = args[i];
            i += 1;
        }
        Self { args: args_array }
    }

    pub const fn as_array(&self) -> &[u64; 7] {
        &self.args
    }
}

impl AsRef<[u64; 7]> for ParameterList {
    fn as_ref(&self) -> &[u64; 7] {
        &self.args
    }
}
