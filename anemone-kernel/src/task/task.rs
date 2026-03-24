use core::fmt::{Debug, Display};

use crate::{
    mm::stack::KernelStack,
    prelude::*,
    sync::mono::MonoFlow,
    task::tid::{Tid, TidHandle, alloc_tid},
};

/// All the information about a task
#[repr(C)]
pub struct Task {
    status: RwLock<TaskStatus>,
    flags: TaskFlags,
    tid: TidHandle,
    kstack: KernelStack,
    sched_info: MonoFlow<TaskInner>,
}

impl Debug for Task {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Task")
            .field("tid", &self.tid())
            .field("status", &*self.status.read())
            .field("flags", &self.flags)
            .finish()
    }
}

impl Display for Task {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        Display::fmt(&self.tid(), f)
    }
}

pub struct TaskInner {
    /// Used for soft switching
    task_context: TaskContext,
    /// Used for trap handling
    trap_frame: TrapFrame,
}

impl Task {
    /// Create a new kernel task.
    ///
    /// Parameters represented in [ParameterList] will be passed to the entry
    /// function, with C-Style calling convention
    ///
    /// [TaskFlags::KERNEL] will be automatically added to the flags of the
    /// created task, so it's optional.
    pub fn new_kernel(
        entry: *const (),
        args: ParameterList,
        irq_flags: IrqFlags,
        flags: TaskFlags,
    ) -> Result<Self, MmError> {
        let stack = KernelStack::new()?;
        let stack_top = stack.stack_top();
        kdebugln!("Created Task with Kernel Stack at {:?}", stack);
        Ok(Self {
            status: RwLock::new(TaskStatus::Ready),
            flags: flags | TaskFlags::KERNEL,
            tid: alloc_tid(),
            kstack: stack,
            sched_info: unsafe {
                MonoFlow::new(TaskInner {
                    task_context: TaskContext::from_kernel_fn(
                        VirtAddr::new(entry as u64),
                        stack_top,
                        irq_flags,
                        args,
                    ),
                    trap_frame: TrapFrame::ZEROED,
                })
            },
        })
    }
}

impl Task {
    pub fn tid(&self) -> Tid {
        Tid::new(self.tid.get())
    }

    pub unsafe fn get_task_context(&self) -> *const TaskContext {
        self.sched_info
            .with(|inner| &inner.task_context as *const TaskContext)
    }

    pub unsafe fn get_task_context_mut(&self) -> *mut TaskContext {
        self.sched_info
            .with_mut(|inner| &mut inner.task_context as *mut TaskContext)
    }
}

impl Task {
    pub fn flags(&self) -> TaskFlags {
        self.flags
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        knoticeln!("{} dropped", self.tid());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskStatus {
    Running,
    Ready,
    Blocked,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct TaskFlags: u8{
        const NONE = 0;
        const KERNEL = 1 << 0;
        const IDLE = 1 << 1;
    }
}
