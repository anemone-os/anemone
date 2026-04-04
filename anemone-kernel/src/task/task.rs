use core::fmt::{Debug, Display};

use spin::Once;

use crate::{
    mm::stack::KernelStack,
    prelude::*,
    sync::mono::MonoFlow,
    task::tid::{TID_IDLE, Tid, TidHandle, alloc_tid},
};

static TASK_ROOT: Once<Arc<Task>> = Once::new();

/// Register the root task, which is the ancestor of all tasks in the system.
///
/// **This function should only be called once during the kernel initialization,
/// otherwise it will do nothing.**
pub fn register_root_task(task: Arc<Task>) {
    TASK_ROOT.call_once(|| task);
}

/// Get the root task. Panic if the root task is not registered yet.
pub fn root_task() -> &'static Arc<Task> {
    TASK_ROOT.get().expect("root task is not registered")
}

/// Wait for the root task to be registered and return it.
pub fn wait_for_root_task() -> &'static Arc<Task> {
    TASK_ROOT.wait()
}

/// All the information about a task
#[repr(C)]
pub struct Task {
    // static information
    tid: TidHandle,
    kstack: KernelStack,
    sched_info: MonoFlow<TaskSchedInfo>,

    // execution information
    exec_info: RwLock<TaskExecInfo>,
    exit_code: AtomicIsize,

    // hierarchy information
    hierarchy: RwLock<TaskHierarchy>,

    // running status
    status: RwLock<TaskStatus>,
}

pub struct TaskHierarchy {
    parent: Option<Weak<Task>>,
    children: Vec<Arc<Task>>,
}

impl TaskHierarchy {
    pub fn set_parent(&mut self, parent: &Arc<Task>) {
        self.parent = Some(Arc::downgrade(parent));
    }
    pub fn parent(&self) -> Option<Weak<Task>> {
        self.parent.clone()
    }
    pub fn add_child(&mut self, child: &Arc<Task>) {
        self.children.push(child.clone());
    }
}

#[cfg(debug_assertions)]
impl Drop for TaskHierarchy {
    fn drop(&mut self) {
        if self.children.len() > 0 {
            panic!(
                "task dropped while there are still {} children",
                self.children.len()
            );
        }
    }
}

pub struct TaskExecInfo {
    pub cmdline: Box<str>,
    pub flags: TaskFlags,
    pub uspace: Option<Arc<UserSpace>>,
}

pub struct TaskSchedInfo {
    /// Used for soft switching
    task_context: TaskContext,
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

impl Debug for Task {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Task")
            .field("tid", &self.tid())
            .field("status", &*self.status.read())
            .field("flags", &self.flags())
            .finish()
    }
}

impl Display for Task {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        Display::fmt(&self.tid(), f)
    }
}

impl Task {
    /// Create a new kernel task with a kernel stack and kernel entry context.
    ///
    /// Parameters represented in [ParameterList] are passed to the entry
    /// function using the C calling convention.
    ///
    /// [TaskFlags::KERNEL] is added automatically.
    ///
    /// # Safety
    /// This function is unsafe because, according to the task tree structure,
    /// the could be only one root task whose `parent` is [None]. Casually
    /// creating multiple root tasks will break the task hierarchy and cause
    /// undefined behavior.
    pub unsafe fn new_kernel(
        name: impl AsRef<str>,
        entry: *const (),
        args: ParameterList,
        irq_flags: IrqFlags,
        flags: TaskFlags,
        parent: Option<&Arc<Task>>,
    ) -> Result<Self, MmError> {
        let stack = KernelStack::new()?;
        let stack_top = stack.stack_top();
        let parent = parent.and_then(|arc| Some(Arc::downgrade(arc)));
        kdebugln!("created kernel task with kernel stack at {:?}", stack);
        Ok(Self {
            status: RwLock::new(TaskStatus::Ready),
            tid: alloc_tid(),
            kstack: stack,
            sched_info: unsafe {
                MonoFlow::new(TaskSchedInfo {
                    task_context: TaskContext::from_kernel_fn(
                        VirtAddr::new(entry as u64),
                        stack_top,
                        irq_flags,
                        args,
                    ),
                })
            },
            exec_info: RwLock::new(TaskExecInfo {
                cmdline: (String::from("@kernel/") + name.as_ref()).into_boxed_str(),
                flags: flags | TaskFlags::KERNEL,
                uspace: None,
            }),
            hierarchy: RwLock::new(TaskHierarchy {
                parent,
                children: vec![],
            }),
            exit_code: AtomicIsize::new(0),
        })
    }
    /*
    pub unsafe fn new_user(
        cmdline: Box<str>,
        entry: *const (),
        ustack_top: VirtAddr,
        parent: &Arc<Task>,
        uspace: Arc<UserSpace>,
    ) -> Result<Self, MmError> {
        let kstack = KernelStack::new()?;
        let kstack_top = kstack.stack_top();
        kdebugln!("created user task with kernel stack at {:?}", kstack);
        Ok(Self {
            status: RwLock::new(TaskStatus::Ready),
            tid: alloc_tid(),
            kstack,
            sched_info: unsafe {
                MonoFlow::new(TaskSchedInfo {
                    task_context: TaskContext::from_user_fn(
                        VirtAddr::new(entry as u64),
                        ustack_top,
                        kstack_top,
                    ),
                })
            },
            exec_info: RwLock::new(TaskExecInfo {
                cmdline: cmdline,
                flags: TaskFlags::NONE,
                uspace: Some(uspace),
            }),
            hierarchy: RwLock::new(TaskHierarchy {
                parent: Some(Arc::downgrade(parent)),
                children: vec![],
            }),
            exit_code: AtomicIsize::new(0),
        })
    }*/

    /// Create a new idle task with tid [TID_IDLE] and the [TaskFlags::IDLE]
    /// flag.
    ///
    /// # Safety
    /// This function is unsafe because idle tasks are special tasks that should
    /// not be created casually.
    pub unsafe fn new_idle(entry: *const ()) -> Result<Self, MmError> {
        let stack = KernelStack::new()?;
        let stack_top = stack.stack_top();
        kdebugln!("created kernel task with kernel stack at {:?}", stack);
        Ok(Self {
            status: RwLock::new(TaskStatus::Ready),
            tid: TID_IDLE,
            kstack: stack,
            sched_info: unsafe {
                MonoFlow::new(TaskSchedInfo {
                    task_context: TaskContext::from_kernel_fn(
                        VirtAddr::new(entry as u64),
                        stack_top,
                        IntrArch::ENABLED_IRQ_FLAGS,
                        ParameterList::empty(),
                    ),
                })
            },
            exec_info: RwLock::new(TaskExecInfo {
                cmdline: Box::from("@idle"),
                flags: TaskFlags::IDLE | TaskFlags::KERNEL,
                uspace: None,
            }),
            hierarchy: RwLock::new(TaskHierarchy {
                parent: None,
                children: vec![],
            }),
            exit_code: AtomicIsize::new(0),
        })
    }

    /// Get the parent task ID, if the parent task exists.
    ///
    /// This function will return [None] only if this task has no parent, e.g.
    /// the init task, the idle task, a exited task or the kinit task.
    pub fn parent_tid(&self) -> Option<Tid> {
        self.hierarchy
            .read_irqsave()
            .parent
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
    pub fn cmdline(&self) -> Box<str> {
        self.exec_info.read_irqsave().cmdline.clone()
    }

    /// Get the task flags.
    pub fn flags(&self) -> TaskFlags {
        self.exec_info.read_irqsave().flags
    }

    /// Get the user-space memory context of this task, if any.
    pub fn clone_uspace(&self) -> Option<Arc<UserSpace>> {
        self.exec_info.read_irqsave().uspace.clone()
    }

    pub fn kstack(&self) -> &KernelStack {
        &self.kstack
    }

    pub fn exit_code(&self) -> isize {
        self.exit_code.load(Ordering::SeqCst)
    }

    pub fn set_exit_code(&self, code: isize) {
        self.exit_code.store(code, Ordering::SeqCst);
    }

    /// Set the task info.
    /// # Safety
    /// ***This operation will immediately drop the current page table, so make
    /// sure you have activated a new one before calling this function***
    pub unsafe fn set_exec_info(&self, info: TaskExecInfo) {
        *self.exec_info.write_irqsave() = info;
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        kdebugln!("{}({}) dropped", self.tid(), self.cmdline());
    }
}
