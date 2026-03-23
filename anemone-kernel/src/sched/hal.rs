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
    pub fn empty() -> Self {
        Self { args: [0; 7] }
    }
    pub fn from_1_args(arg1: u64) -> Self {
        Self {
            args: [arg1, 0, 0, 0, 0, 0, 0],
        }
    }
    pub fn from_2_args(arg1: u64, arg2: u64) -> Self {
        Self {
            args: [arg1, arg2, 0, 0, 0, 0, 0],
        }
    }
    pub fn from_3_args(arg1: u64, arg2: u64, arg3: u64) -> Self {
        Self {
            args: [arg1, arg2, arg3, 0, 0, 0, 0],
        }
    }
    pub fn from_4_args(arg1: u64, arg2: u64, arg3: u64, arg4: u64) -> Self {
        Self {
            args: [arg1, arg2, arg3, arg4, 0, 0, 0],
        }
    }
    pub fn from_5_args(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> Self {
        Self {
            args: [arg1, arg2, arg3, arg4, arg5, 0, 0],
        }
    }
    pub fn from_6_args(arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64, arg6: u64) -> Self {
        Self {
            args: [arg1, arg2, arg3, arg4, arg5, arg6, 0],
        }
    }
    pub fn from_args(args: [u64; 7]) -> Self {
        Self { args }
    }
    pub fn as_array(&self) -> &[u64; 7] {
        &self.args
    }
}
