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
    tid: TidHandle,
    name: Box<str>,
    flags: TaskFlags,
    kstack: KernelStack,
    sched_info: MonoFlow<TaskInner>,
    uspace: Option<Arc<UserSpace>>,

    parent: RwLock<Option<Weak<Task>>>,
    children: RwLock<Vec<Arc<Task>>>,

    pub exit_code: AtomicI32,
    pub status: RwLock<TaskStatus>,
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
}

impl Task {
    /// Create a new kernel task with a kernel stack and kernel entry context.
    ///
    /// Parameters represented in [ParameterList] are passed to the entry
    /// function using the C calling convention.
    ///
    /// [TaskFlags::KERNEL] is added automatically.
    pub fn new_kernel(
        name: impl AsRef<str>,
        entry: *const (),
        args: ParameterList,
        irq_flags: IrqFlags,
        flags: TaskFlags,
        parent: Option<Weak<Task>>,
    ) -> Result<Self, MmError> {
        let stack = KernelStack::new()?;
        let stack_top = stack.stack_top();
        kdebugln!("created kernel task with kernel stack at {:?}", stack);
        Ok(Self {
            status: RwLock::new(TaskStatus::Ready),
            name: Box::from(name.as_ref()),
            flags: flags | TaskFlags::KERNEL,
            tid: alloc_tid(),
            kstack: stack,
            uspace: None,
            sched_info: unsafe {
                MonoFlow::new(TaskInner {
                    task_context: TaskContext::from_kernel_fn(
                        VirtAddr::new(entry as u64),
                        stack_top,
                        irq_flags,
                        args,
                    ),
                })
            },
            parent: RwLock::new(parent),
            children: RwLock::new(vec![]),
            exit_code: AtomicI32::new(0),
        })
    }

    /// Create a new user task with a kernel stack and user-space entry context.
    pub fn new_user(
        name: impl AsRef<str>,
        entry: *const (),
        uspace: Arc<UserSpace>,
        ustack_top: VirtAddr,
        parent: Option<Weak<Task>>,
    ) -> Result<Self, MmError> {
        let ustack = KernelStack::new()?;
        let kstack_top = ustack.stack_top();
        kdebugln!("create user task with kernel stack at {:?}", ustack);
        Ok(Self {
            status: RwLock::new(TaskStatus::Ready),
            name: Box::from(name.as_ref()),
            flags: TaskFlags::NONE,
            tid: alloc_tid(),
            kstack: ustack,
            uspace: Some(uspace),
            sched_info: unsafe {
                MonoFlow::new(TaskInner {
                    task_context: TaskContext::from_user_fn(
                        VirtAddr::new(entry as u64),
                        ustack_top,
                        kstack_top,
                    ),
                })
            },
            parent: RwLock::new(parent),
            children: RwLock::new(vec![]),
            exit_code: AtomicI32::new(0),
        })
    }

    /// Get the parent task ID, if the parent task exists.
    ///
    /// This function will return [None] only if this task has no parent, e.g.
    /// the init task, the idle task, a exited task or the kinit task.
    pub fn parent_tid(&self) -> Option<Tid> {
        self.parent
            .read()
            .as_ref()
            .and_then(|weak| weak.upgrade().map(|parent| parent.tid()))
    }
}

impl Task {
    /// Get the task context used by the scheduler.
    pub unsafe fn get_task_context(&self) -> *const TaskContext {
        self.sched_info
            .with(|inner| &inner.task_context as *const TaskContext)
    }

    /// Get a mutable pointer to the task context used by the scheduler.
    pub unsafe fn get_task_context_mut(&self) -> *mut TaskContext {
        self.sched_info
            .with_mut(|inner| &mut inner.task_context as *mut TaskContext)
    }
}

impl Task {
    /// Get the task ID.
    pub fn tid(&self) -> Tid {
        Tid::new(self.tid.get())
    }

    /// Get the task name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the task flags.
    pub fn flags(&self) -> TaskFlags {
        self.flags
    }

    /// Get the user-space memory context of this task, if any.
    pub fn uspace(&self) -> Option<&Arc<UserSpace>> {
        self.uspace.as_ref()
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        knoticeln!("{}({}) dropped", self.tid(), self.name());
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
