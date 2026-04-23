use core::{
    fmt::{Debug, Display},
    mem::swap,
    ops::{Deref, DerefMut},
};

use crate::{
    mm::stack::KernelStack,
    prelude::{dt::UserWritePtr, *},
    sync::mono::MonoFlow,
    task::{
        boot::root_task,
        clone::CloneFlags,
        cpu_usage::CpuUsage,
        files::FilesState,
        task_fs::FsState,
        tid::{Tid, TidHandle, alloc_tid},
        wait::WaitQueue,
    },
};

/// Parent/children links maintained for each task.
///
/// These information are grouped together with a hope to prevent races as much
/// as possible.
pub struct TaskHierarchy {
    /// Weak reference to parent task, or [None] for root-like tasks.
    parent: Option<Weak<Task>>,
    /// Strong references to all direct children.
    children: Vec<Arc<Task>>,
}

impl TaskHierarchy {
    /// Set `parent` as the parent of this task node.
    pub fn set_parent(&mut self, parent: &Arc<Task>) {
        self.parent = Some(Arc::downgrade(parent));
    }

    /// Get the current parent weak reference.
    pub fn parent(&self) -> Option<Weak<Task>> {
        self.parent.clone()
    }

    /// Add `child` into the direct children list.
    pub fn add_child(&mut self, child: Arc<Task>) {
        self.children.push(child);
    }

    /// Remove `child` from the direct children list.
    ///
    /// Returns `true` if `child` existed and was removed.
    fn remove_child(&mut self, child: &Arc<Task>) -> bool {
        if let Some(index) = self.children.iter().position(|x| x.eq(child)) {
            self.children.remove(index);
            true
        } else {
            false
        }
    }

    /// Remove and return all direct children.
    pub fn clear(&mut self) -> Vec<Arc<Task>> {
        let mut temp = vec![];
        swap(&mut temp, &mut self.children);
        temp
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

/// Execution context of a task.
pub struct TaskExecCtx {
    /// Command line shown for this task.
    pub cmdline: Box<str>,
    /// Task attribute flags.
    pub flags: TaskFlags,
    /// User address space, or [None] for pure kernel tasks.
    pub uspace: Option<Arc<UserSpace>>,
}

/// Scheduling-related context of a task.
#[repr(C)]
pub struct TaskSchedCtx {
    /// Used for soft switching
    task_context: TaskContext,
    /// Points to the TrapFrame saved on the kernel stack during the last user
    /// trap entry.
    utrap_frame: Option<*const TrapFrame>,
}
unsafe impl Send for TaskSchedCtx {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskStatus {
    /// Task is currently running on a CPU.
    Running,
    /// Task is runnable and waiting to be scheduled.
    Ready,
    /// Task has exited and is waiting to be reaped.
    Zombie,
    /// Task is blocked in a wait state.
    Waiting { interruptible: bool },
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct TaskFlags: u8{
        /// No special flags.
        const NONE = 0;
        /// Marks a kernel task.
        const KERNEL = 1 << 0;
        /// Marks an idle task.
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
    ///
    /// This function is unsafe because:
    ///  * The parent task of the created task is set to [None] by default.
    ///    However, according to the task tree structure, there could be only
    ///    one root task whose `parent` is [None]. Casually creating multiple
    ///    root tasks will break the task hierarchy and cause undefined
    ///    behavior.
    ///
    /// # Notes
    ///
    /// [FsState] is set to hanging by default, which is suitable for most
    /// kernel threads. If a kernel thread needs to execute a user program which
    /// requires filesystem access, make sure to set its [FsState] to a valid
    /// one before calling `kernel_execve`.
    pub unsafe fn new_kernel(
        name: &str,
        entry: *const (),
        args: ParameterList,
        irq_flags: IrqFlags,
        flags: TaskFlags,
        create_flags: CloneFlags,
    ) -> Result<(Task, RegisterGuard), SysError> {
        let stack = KernelStack::new()?;
        let stack_top = stack.stack_top();
        let task = Self {
            status: RwLock::new(TaskStatus::Ready),
            tid: alloc_tid().ok_or(SysError::OutOfMemory)?,
            kstack: stack,

            fs_state: Arc::new(RwLock::new(FsState::new_hanging())),
            files_state: Arc::new(RwLock::new(FilesState::new())),

            cpu_usage: RwLock::new(CpuUsage::ZERO),

            sched_ctx: unsafe {
                MonoFlow::new(TaskSchedCtx {
                    task_context: TaskContext::from_kernel_fn(
                        VirtAddr::new(entry as u64),
                        stack_top,
                        irq_flags,
                        args,
                    ),
                    utrap_frame: None,
                })
            },
            exec_ctx: RwLock::new(TaskExecCtx {
                cmdline: (String::from("@kernel/") + name).into_boxed_str(),
                flags: flags | TaskFlags::KERNEL,
                uspace: None,
            }),
            hierarchy: RwLock::new(TaskHierarchy {
                parent: None,
                children: vec![],
            }),
            exit_code: AtomicI8::new(0),
            create_flags,
            clear_child_tid: RwLock::new(None),
            wait_childexit: WaitQueue::new(),
        };
        Ok((task, RegisterGuard))
    }

    /// Create a new idle task with `tid` [TIDH_IDLE] and the [TaskFlags::IDLE]
    /// flag.
    ///
    /// # Safety
    /// This function is unsafe because idle tasks are special tasks that should
    /// not be created casually.
    pub unsafe fn new_idle(entry: *const ()) -> Result<(Task, RegisterGuard), SysError> {
        let stack = KernelStack::new()?;
        let stack_top = stack.stack_top();
        Ok((
            Task {
                status: RwLock::new(TaskStatus::Ready),
                tid: TidHandle::IDLE,
                kstack: stack,
                sched_ctx: unsafe {
                    MonoFlow::new(TaskSchedCtx {
                        task_context: TaskContext::from_kernel_fn(
                            VirtAddr::new(entry as u64),
                            stack_top,
                            IntrArch::ENABLED_IRQ_FLAGS,
                            ParameterList::empty(),
                        ),
                        utrap_frame: None,
                    })
                },
                exec_ctx: RwLock::new(TaskExecCtx {
                    cmdline: Box::from("@idle"),
                    flags: TaskFlags::IDLE | TaskFlags::KERNEL,
                    uspace: None,
                }),
                hierarchy: RwLock::new(TaskHierarchy {
                    parent: None,
                    children: vec![],
                }),

                fs_state: Arc::new(RwLock::new(FsState::new_hanging())),
                files_state: Arc::new(RwLock::new(FilesState::new())),

                cpu_usage: RwLock::new(CpuUsage::ZERO),

                exit_code: AtomicI8::new(0),
                create_flags: CloneFlags::empty(),
                clear_child_tid: RwLock::new(None),
                wait_childexit: WaitQueue::new(),
            },
            RegisterGuard,
        ))
    }

    /// Get the parent task ID, if the parent task exists.
    ///
    /// This function will return [None] only if this task has no parent, e.g.
    /// the init task, the idle task, a exited task or the kinit task.
    pub fn parent_tid(&self) -> Option<Tid> {
        self.hierarchy
            .read()
            .parent
            .as_ref()
            .and_then(|weak| weak.upgrade().map(|parent| parent.tid()))
    }
}

impl Task {
    /// Get the task context used by the scheduler.
    ///
    /// # Safety
    /// * **Make sure interrupts are disabled before calling this function,
    /// otherwise undefined behavior or unexpected panics may occur.**
    ///
    /// * **This function may only be called within a single execution flow,
    /// typically the task's own execution context.
    /// Parallel access will lead to data races.**
    pub unsafe fn get_task_context(&self) -> *const TaskContext {
        debug_assert!(IntrArch::local_intr_disabled());
        self.sched_ctx
            .with(|inner| &inner.task_context as *const TaskContext)
    }

    /// Get a mutable pointer to the task context used by the scheduler.
    ///
    /// # Safety
    /// * **Make sure interrupts are disabled before calling this function,
    /// otherwise undefined behavior or unexpected panics may occur.**
    ///
    /// * **This function may only be called within a single execution flow,
    /// typically the task's own execution context.
    /// Parallel access will lead to data races.**
    pub unsafe fn get_task_context_mut(&self) -> *mut TaskContext {
        debug_assert!(IntrArch::local_intr_disabled());
        self.sched_ctx
            .with_mut(|inner| &mut inner.task_context as *mut TaskContext)
    }

    /// Set the user trap frame for this task, called by the trap handler.
    ///
    /// # Safety
    /// * **Make sure interrupts are disabled before calling this function,
    /// otherwise undefined behavior or unexpected panics may occur.**
    ///
    /// * **This function may only be called within a single execution flow,
    /// typically the task's own execution context.
    /// Parallel access will lead to data races.**
    pub unsafe fn set_utrapframe(&self, trap_frame: *const TrapFrame) {
        debug_assert!(IntrArch::local_intr_disabled());
        self.sched_ctx
            .with_mut(|inner| inner.utrap_frame = Some(trap_frame));
    }

    /// Get the user trap frame for this task, if it exists.
    ///
    /// # Safety
    /// * **Make sure interrupts are disabled before calling this function,
    /// otherwise undefined behavior or unexpected panics may occur.**
    ///
    /// * **This function may only be called within a single execution flow,
    /// typically the task's own execution context.
    /// Parallel access will lead to data races.**
    pub unsafe fn get_utrapframe(&self) -> Option<*const TrapFrame> {
        debug_assert!(IntrArch::local_intr_disabled());
        self.sched_ctx.with(|inner| inner.utrap_frame)
    }
}

impl Task {
    /// Get the task ID.
    pub fn tid(&self) -> Tid {
        Tid::new(self.tid.get())
    }

    /// Get the task name.
    pub fn cmdline(&self) -> Box<str> {
        self.exec_ctx.read_irqsave().cmdline.clone()
    }

    /// Get the task flags.
    pub fn flags(&self) -> TaskFlags {
        self.exec_ctx.read_irqsave().flags
    }

    /// Get the user-space memory context of this task, if any.
    pub fn clone_uspace(&self) -> Option<Arc<UserSpace>> {
        self.exec_ctx.read_irqsave().uspace.clone()
    }

    /// Get this task's kernel stack.
    pub fn kstack(&self) -> &KernelStack {
        &self.kstack
    }

    /// Load the exit code atomically.
    pub fn exit_code(&self) -> i8 {
        self.exit_code.load(Ordering::SeqCst)
    }

    /// Store `code` as the task exit code atomically.
    pub fn set_exit_code(&self, code: i8) {
        self.exit_code.store(code, Ordering::SeqCst);
    }

    /// Switch the executable context of this task to `ctx`.
    ///
    /// # Safety
    ///
    /// It's quite obvious. Almost always this function should only be called
    /// when doing an [kernel_execve] or something similar.
    pub unsafe fn switch_exec_ctx(&self, ctx: TaskExecCtx) {
        *self.exec_ctx.write_irqsave() = ctx;
    }

    /// Run `f` with an immutable reference to this task's hierarchy links.
    ///
    /// # Locking Rules
    /// When nesting hierarchy-lock acquisition across multiple tasks, callers
    /// must follow parent-to-child (or the same consistent ancestor chain)
    /// order. Acquiring hierarchy locks out of hierarchy order can deadlock.
    ///
    /// Nested acquisition of multiple tasks that are not on one parent-child
    /// chain is forbidden, because it may cause unexpected deadlocks.
    ///
    /// # Safety
    /// Caller must ensure no conflicting mutable hierarchy access happens
    /// concurrently.
    pub unsafe fn with_task_hierarchy<F: FnOnce(&TaskHierarchy) -> R, R>(&self, f: F) -> R {
        let hierarchy = self.hierarchy.read();
        f(hierarchy.deref())
    }

    /// Run `f` with a mutable reference to this task's hierarchy links.
    ///
    /// # Locking Rules
    /// When nesting hierarchy-lock acquisition across multiple tasks, callers
    /// must follow parent-to-child (or the same consistent ancestor chain)
    /// order. Acquiring hierarchy locks out of hierarchy order can deadlock.
    ///
    /// Nested acquisition of multiple tasks that are not on one parent-child
    /// chain is forbidden, because it may cause unexpected deadlocks.
    ///
    /// # Safety
    /// Caller must ensure the hierarchy mutation is synchronized with all other
    /// hierarchy readers/writers.
    pub unsafe fn with_task_hierarchy_mut<F: FnOnce(&mut TaskHierarchy) -> R, R>(&self, f: F) -> R {
        let mut hierarchy = self.hierarchy.write();
        f(hierarchy.deref_mut())
    }

    /// Get the clone flags used when creating this task.
    pub fn clone_flags(&self) -> CloneFlags {
        self.create_flags
    }

    /// Set `tid_ptr` as the clear-child-tid target pointer.
    pub fn set_clear_child_tid(&self, tid_ptr: Option<UserWritePtr<Tid>>) {
        *self.clear_child_tid.write() = tid_ptr;
    }

    /// Get the current clear-child-tid target pointer.
    pub fn get_clear_child_tid(&self) -> Option<UserWritePtr<Tid>> {
        self.clear_child_tid.read().clone()
    }

    /// Get the current task status.
    pub fn status(&self) -> TaskStatus {
        self.status.read().clone()
    }

    /// Update task status to `status`.
    pub fn set_status(&self, status: TaskStatus) {
        *self.status.write() = status;
    }
}

/// Extra task-tree and wait helpers implemented on [Arc<Task>].
pub trait ArcTaskImpls {
    /// Attach this task as a child of `parent`.
    unsafe fn add_as_child(&self, parent: &Arc<Task>);

    /// Notify parent waiters that this task has exited.
    unsafe fn note_exited(&self);

    /// Wait for a child selected by `target`, then reap and return it.
    unsafe fn waitpid(
        &self,
        target: WaitObject,
        sleep: bool,
    ) -> Result<Option<Arc<Task>>, SysError>;
}
impl ArcTaskImpls for Arc<Task> {
    unsafe fn add_as_child(&self, parent: &Arc<Task>) {
        unsafe {
            if !parent.with_task_hierarchy_mut(|par_hier| {
                if parent.status() == TaskStatus::Zombie {
                    return false;
                }
                self.with_task_hierarchy_mut(|hier| {
                    debug_assert!(hier.parent.is_none());
                    hier.set_parent(&parent);
                    par_hier.add_child(self.clone());
                    true
                })
            }) {
                let root = root_task();
                root.with_task_hierarchy_mut(|root_hier| {
                    self.with_task_hierarchy_mut(|hier| {
                        hier.set_parent(root);
                        root_hier.add_child(self.clone());
                    })
                })
            }
        }
    }

    unsafe fn note_exited(&self) {
        let parent = unsafe { self.with_task_hierarchy(|hier| hier.parent()) }
            .unwrap_or_else(|| panic!("cannot note exited for a root task: {}", self.tid()))
            .upgrade()
            .unwrap_or_else(|| panic!("dangling task with parrent dropped: {}", self.tid()));
        parent.wait_childexit.wake(self, false);
    }

    /// This is the only way to remove a task from its children list
    unsafe fn waitpid(
        &self,
        target: WaitObject,
        sleep: bool,
    ) -> Result<Option<Arc<Task>>, SysError> {
        unsafe {
            self.with_task_hierarchy(|hier| {
                if hier
                    .children
                    .iter()
                    .position(|val| target.match_task(val))
                    .is_none()
                {
                    Err(SysError::ChildrenNotFound)
                } else {
                    Ok(())
                }
            })?;
        }
        loop {
            let child = self
                .wait_childexit
                .wait_if(true, || unsafe {
                    self.with_task_hierarchy_mut(|hier| {
                        for ch in &hier.children {
                            if target.match_task(ch) && ch.status() == TaskStatus::Zombie {
                                return Err(Some(ch.clone()));
                            }
                        }
                        if !sleep { Err(None) } else { Ok(()) }
                    })
                })
                .and_then(|a| Ok(Some(a)))
                .unwrap_or_else(|e| e);
            let child = match child {
                Some(child) => child,
                None => return Ok(None),
            };
            if target.match_task(&child) {
                unsafe {
                    self.with_task_hierarchy_mut(|hier| {
                        let res = hier.remove_child(&child);
                        debug_assert!(res);
                    });
                }
                self.on_reap_child(&child);
                return Ok(Some(child));
            }
        }
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        kdebugln!("{}({}) dropped", self.tid(), self.cmdline());
    }
}

impl PartialEq for Task {
    fn eq(&self, other: &Self) -> bool {
        match (self.tid(), other.tid()) {
            (Tid::IDLE, Tid::IDLE) => {
                // TODO: there do exist multiple idle tasks, one per cpu. we should find a way
                // to distinguish them. but for now, we just panic if such
                // comparison happens, because it is likely a bug in the code.
                panic!("comparing idle tasks is not supported");
            },
            (x, y) => x == y,
        }
    }
}

impl Eq for Task {}

pub enum WaitObject {
    /// Wait for a thread group id (not implemented yet).
    Tgid(u32), // not implemented
    /// Wait for a specific task id, or any child when `None`.
    Tid(Option<Tid>),
}

impl WaitObject {
    /// Check whether `task` matches this wait target.
    pub fn match_task(&self, task: &Arc<Task>) -> bool {
        match self {
            Self::Tgid(_) => unimplemented!(),
            Self::Tid(tid) => match tid.as_ref() {
                Some(tid) => task.tid().eq(tid),
                None => true,
            },
        }
    }
}
