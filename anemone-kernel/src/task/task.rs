use core::{
    fmt::{Debug, Display},
    ptr::NonNull,
};

use crate::{
    mm::stack::KernelStack,
    prelude::{dt::UserWritePtr, *},
    sched::class::{SchedClassPrv, SchedEntity},
    sync::mono::MonoFlow,
    task::{
        cpu_usage::CpuUsage,
        files::FilesState,
        task_fs::FsState,
        tid::{Tid, TidHandle, alloc_tid},
        topology::RegisterGuard,
    },
};

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

// region: spawn

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
    ///
    /// The created task will run on certain cpu decided by the scheduler. See
    /// [pick_next_cpu] for details. If ‵cpu` is specified, the created task
    /// will be pinned to that cpu. For general cases, this should not be used.
    pub unsafe fn new_kernel(
        name: &str,
        entry: *const (),
        args: ParameterList,
        sched: SchedEntity,
        flags: TaskFlags,
        cpu: Option<CpuId>,
    ) -> Result<(Task, RegisterGuard), SysError> {
        let stack = KernelStack::new()?;
        let stack_top = stack.stack_top();
        let task = Self {
            tid: alloc_tid().ok_or(SysError::OutOfMemory)?,
            kstack: stack,
            name: RwLock::new((String::from("@kernel/") + name).into_boxed_str()),
            flags: RwLock::new(flags | TaskFlags::KERNEL),
            uspace: RwLock::new(None),
            cpuid: if let Some(cpu) = cpu {
                cpu
            } else {
                pick_next_cpu()
            },
            sched_ctx: unsafe {
                MonoFlow::new(TaskContext::from_kernel_fn(
                    VirtAddr::new(entry as u64),
                    stack_top,
                    args,
                ))
            },
            sched_entity: SpinLock::new(sched),
            utrapframe: unsafe { MonoFlow::new(None) },
            fs_state: Arc::new(RwLock::new(FsState::new_hanging())),
            files_state: Arc::new(RwLock::new(FilesState::new())),
            cpu_usage: RwLock::new(CpuUsage::ZERO),
            exit_code: AtomicI8::new(0),
            child_exited: Event::new(),

            status: RwLock::new(TaskStatus::Runnable),
            clear_child_tid: RwLock::new(None),
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
                tid: TidHandle::IDLE,
                kstack: stack,
                name: RwLock::new(Box::from("@idle")),
                flags: RwLock::new(TaskFlags::IDLE | TaskFlags::KERNEL),
                uspace: RwLock::new(None),
                cpuid: cur_cpu_id(),
                sched_ctx: unsafe {
                    MonoFlow::new(TaskContext::from_kernel_fn(
                        VirtAddr::new(entry as u64),
                        stack_top,
                        ParameterList::empty(),
                    ))
                },
                sched_entity: SpinLock::new(SchedEntity::new(SchedClassPrv::Idle(()))),
                utrapframe: unsafe { MonoFlow::new(None) },
                fs_state: Arc::new(RwLock::new(FsState::new_hanging())),
                files_state: Arc::new(RwLock::new(FilesState::new())),
                cpu_usage: RwLock::new(CpuUsage::ZERO),
                exit_code: AtomicI8::new(0),
                child_exited: Event::new(),

                status: RwLock::new(TaskStatus::Runnable),
                clear_child_tid: RwLock::new(None),
            },
            RegisterGuard,
        ))
    }
}

// endregion: spawn

// region: context accessors

impl Task {
    /// Get a pointer to this task's scheduling context.
    ///
    /// # Safety
    ///
    /// **Only scheduler's code can call this function.**
    ///
    /// **Preemption and interrupts must be disabled during the window when the
    /// returned pointer might be accessed.**
    #[track_caller]
    pub unsafe fn get_sched_ctx(&self) -> *const TaskContext {
        debug_assert!(IntrArch::local_intr_disabled());
        self.sched_ctx.with(|inner| inner as *const TaskContext)
    }

    /// Get a mutable pointer to this task's scheduling context.
    ///
    /// # Safety
    ///
    /// **Only scheduler's code can call this function.**
    ///
    /// **Preemption and interrupts must be disabled during the window when the
    /// returned pointer might be accessed.**
    #[track_caller]
    pub unsafe fn get_sched_ctx_mut(&self) -> *mut TaskContext {
        debug_assert!(IntrArch::local_intr_disabled());
        self.sched_ctx.with_mut(|inner| inner as *mut TaskContext)
    }

    /// Set user trapframe pointer. Called by user trap handler when a task
    /// traps into kernel.
    ///
    /// # Safety
    ///
    /// **Only user trap handler's code can call this function.**
    #[track_caller]
    pub unsafe fn set_utrapframe(&self, trapframe: *mut TrapFrame) {
        self.utrapframe.with_mut(|inner| {
            *inner = Some(NonNull::new(trapframe).expect("trapframe pointer cannot be null"))
        });
    }

    /// Get a copy of the user trapframe.
    ///
    /// TODO: explain **Why no accessors are provided**.
    ///
    /// # Panics
    #[track_caller]
    pub fn utrapframe(&self) -> TrapFrame {
        unsafe {
            self.utrapframe.with(|inner| {
                inner
                    .as_ref()
                    .expect("trapframe pointer is not set")
                    .as_ref()
                    .clone()
            })
        }
    }
}

// endregion: context accessors

// region: common field accessors

impl Task {
    /// Get the task ID.
    pub fn tid(&self) -> Tid {
        Tid::new(self.tid.get())
    }

    /// Get the task name. This introduces a heap allocation. Pay attention.
    pub fn name(&self) -> Box<str> {
        self.name.read().clone()
    }

    /// Get the task flags.
    pub fn flags(&self) -> TaskFlags {
        self.flags.read().clone()
    }

    /// Get the user-space memory context of this task.
    ///
    /// # Panics
    ///
    /// For a pure kernel task, this function will panic immediately **since
    /// caller misused it**. If you call this function, then you should have
    /// already ensured that this task is indeed a user task (e.g. by calling
    /// [TaskFlags::is_kernel] to check). **If you want a convenient helper,
    /// call [Self::try_clone_uspace] instead.**
    pub fn clone_uspace(&self) -> Arc<UserSpace> {
        self.uspace
            .read()
            .as_ref()
            .expect("cannot access user space of a pure kernel task")
            .clone()
    }

    /// See [Self::clone_uspace] for details.
    pub fn try_clone_uspace(&self) -> Option<Arc<UserSpace>> {
        self.uspace.read().as_ref().map(|uspace| uspace.clone())
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

    /// Switch the execution context of this task to `ctx`.
    ///
    /// Note that this method always expects `uspace` to be [Some]. This is
    /// intentional. Kernel threads should never comes from a [kernel_execve]
    /// call or something similar.
    ///
    /// # Safety
    ///
    /// It's quite obvious. Almost always this function should only be called
    /// when doing an [kernel_execve] or something similar.
    pub unsafe fn switch_exec_ctx(&self, name: Box<str>, uspace: Arc<UserSpace>, flags: TaskFlags) {
        // NOTE THE LOCK ORDERING
        let mut uspace_ptr = self.uspace.write();
        let mut flags_ptr = self.flags.write();
        let mut name_ptr = self.name.write();
        *uspace_ptr = Some(uspace);
        *flags_ptr = flags;
        *name_ptr = name;
    }

    /// Set `tid_ptr` as the clear-child-tid target pointer.
    pub fn set_clear_child_tid(&self, tid_ptr: Option<UserWritePtr<Tid>>) {
        *self.clear_child_tid.write() = tid_ptr;
    }

    /// Get the current clear-child-tid target pointer.
    pub fn get_clear_child_tid(&self) -> Option<UserWritePtr<Tid>> {
        self.clear_child_tid.read().clone()
    }
}

// endregion: common field accessors

impl Drop for Task {
    fn drop(&mut self) {
        kdebugln!("{}({}) dropped", self.tid(), self.name());
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
