// currently just a placeholder.

use alloc::sync::Arc;

use crate::prelude::*;

/// The scheduler trait
pub trait SchedTrait {
    const EMPTY: Self;
    /// Add a task to the ready queue of the current processor.
    fn add_to_ready(&mut self, task: Arc<Task>);
    /// Fetch new task. This will remove the task from the ready queue.
    fn fetch_next(&mut self) -> Option<Arc<Task>>;
}

/// The architecture-specific scheduler trait. This is used for context
/// switching.
pub trait SchedArchTrait {
    /// Task Context type
    type TaskContext: TaskContextArch;
    /// Switch from the current task to the next task. Saves and loads the
    /// callee-saved registers.
    fn switch(cur: *mut TaskContext, next: *const TaskContext);
}

pub trait TaskContextArch {
    const ZEROED: Self;
    fn from_kernel_fn(
        entry: VirtAddr,
        stack_top: VirtAddr,
        irq_flags: IrqFlags,
        args: ParameterList,
    ) -> Self;
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
