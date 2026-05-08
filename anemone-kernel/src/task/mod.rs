//! we expose all fields of Task, ThreadGroup, etc. to the whole task module.
//! this is a bit too much, but i think that's inevitable for kernel code. too
//! much encapsulation will just make the code too verbose and hard to read.

// infras
mod topology;
pub use topology::*;
mod tid;
pub use tid::*;

// integration with other subsystems
pub mod cpu_usage;
pub mod files;
pub mod sig;
#[path = "fs.rs"]
pub mod task_fs;
#[path = "sched.rs"]
pub mod task_sched;

mod api;
pub use api::*;

use core::{
    fmt::{Debug, Display},
    ptr::NonNull,
};

use crate::{
    mm::stack::KernelStack,
    prelude::*,
    sched::class::{SchedClassPrv, SchedEntity},
    sync::mono::MonoFlow,
    task::{
        cpu_usage::{TaskCpuUsage, ThreadGroupCpuUsage},
        files::FilesState,
    },
};

// region: definitions

/// A simple state machine. This looks a bit weird, but it's necessary to
/// support dethreading, where a non-leader thread will take the Tid of the
/// leader thread.
#[derive(Debug)]
pub enum TidRef {
    Idle,
    Owned(TidHandle),
    Leader,
}

/// What we call Task Control Block in kernel terminology.
///
/// **LOCK ORDERING**
///
/// **`uspace`->`flags`->`name`**
///
/// TODO: full lock ordering chain.
pub struct Task {
    /// Task ID.
    tid: RwLock<TidRef>,
    /// What linux folks call "real parent". Tid of the task who cloned/forked
    /// this task.
    ///
    /// For idle and init tasks, this field is set to [None].
    creator: Option<Tid>,
    /// Thread group ID.
    tgid: Tid,

    /// Kernel stack owned by this task.
    kstack: KernelStack,

    /// Name of this task.
    ///
    /// For kernel threads, it is formated in "@kernel/xxx" style, where "xxx"
    /// is the name passed when creating the task.
    ///
    /// For user processes, it is formated in "@user/xxx" style, where "xxx" is
    /// the executable name passed to [kernel_execve].
    ///
    /// Full user command line is not stored in kernel. instead, kernel only
    /// stores the address range of the user command line on user's stack.
    ///
    /// So this field is almost purely for debugging and logging purpose.
    name: RwLock<Box<str>>,

    /// Task attribute flags.
    ///
    /// Memory of TCB is quite precious, so single bool field should not be used
    /// for each flag. Instead, use this bitfield to store all flags.
    flags: RwLock<TaskFlags>,

    /// User address space, or [None] for pure kernel tasks.
    ///
    /// Multiple tasks may share the same user address space.
    uspace: RwLock<Option<Arc<UserSpace>>>,

    /// Which cpu this task is scheduled to run on.
    cpuid: CpuId,

    /// Scheduling context. Used for context switching.
    sched_ctx: MonoFlow<TaskContext>,
    /// Scheduling entity. Used for scheduling.
    sched_entity: SpinLock<SchedEntity>,

    /// User trapframe pointer. Set to:
    /// - [Some] when this task traps into kernel,
    /// - [None] when this task finishes handling the trap and is ready to
    ///   return to user space,
    /// - [None] if this task is a pure kernel thread, and
    /// - [None] if this is a newly created task that has not run yet (i.e. it
    ///   has not trapped into kernel yet).
    utrapframe: MonoFlow<Option<NonNull<TrapFrame>>>,

    /// Filesystem state shared by task-related FS operations.
    fs_state: Arc<RwLock<FsState>>,
    /// File descriptor table state.
    files_state: Arc<RwLock<FilesState>>,
    /// Cpu usage information.
    cpu_usage: RwLock<TaskCpuUsage>,

    /// Exit code of this task. Meaningful only when this task is a zombie.
    exit_code: SpinLock<Option<ExitCode>>,

    /// See [TaskStatus] for a precise definition.
    status: RwLock<TaskStatus>,

    /// Optional user pointer updated during child clear-tid handling.
    ///
    /// This pointer is not guaranteed to be valid. It's user's responsibility.
    clear_child_tid: SpinLock<Option<VirtAddr>>,

    /// TODO: when we have signals, remove this field.
    killed: AtomicBool,
}

/// **[TaskStatus] is not the objective truth of a task's state. Instead, it's
/// the intended state of the task.**
///
/// Most of the time, the actual state of a task is consistent with its
/// [TaskStatus], but there are various windows where they are inconsistent. And
/// that's not a mistake. **It's expected behavior.** It's precisely the
/// foundation upon which we build various synchronization primitives.
///
/// This is, in fact, so-called ***lock-free programming***.
///
/// TODO: explain more specifically. maybe we should write a dedicated chapter
/// about this topic in the book.
///
/// TODO: atomic enum is better.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task can be scheduled to run.
    Runnable,
    /// Task can be reapped by its parent, but cannot run anymore.
    Zombie,
    /// Task can be waken up.
    ///
    /// TODO: interruptible when we support signals.
    Waiting,
}

impl TaskStatus {
    pub const ALL_STATUSES: &[TaskStatus; 3] = &[
        TaskStatus::Runnable,
        TaskStatus::Zombie,
        TaskStatus::Waiting,
    ];

    pub fn is_sleeping(&self) -> bool {
        matches!(self, TaskStatus::Waiting)
    }
}

bitflags! {
    /// TODO: Remove current flags.
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

impl TaskFlags {
    pub const fn is_kernel(&self) -> bool {
        self.contains(TaskFlags::KERNEL)
    }

    pub const fn is_idle(&self) -> bool {
        self.contains(TaskFlags::IDLE)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    /// Normally exited with the given exit code.
    Exited(i8),
    /// Killed by a signal with the given signal number.
    Signaled(u8),
}

/// **LOCK ORDERING**
///
/// TOPOLOGY -> ThreadGroup.inner
#[derive(Debug)]
pub struct ThreadGroup {
    /// The thread group ID, which is the same as the leader thread's ID.
    tgid: TidHandle,
    /// Event that will be published when a child thread group exits.
    child_exited: Event,
    /// Mutable state.
    inner: RwLock<ThreadGroupInner>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadGroupStatus {
    /// See [kernel_execve] for details.
    is_dethreading: bool,
    life_cycle: ThreadGroupLifeCycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadGroupLifeCycle {
    /// The thread group is alive and running normally.
    Alive,
    /// The thread group is exiting, but not finished yet. This middle state
    /// prevents other threads doing any operations on this thread group.
    ///
    /// This state denies any new thread joining this thread group.
    Exiting(ExitCode),
    /// The thread group has exited and can be safely reaped.
    Exited(ExitCode),
}

impl ThreadGroupStatus {
    /// Create a new alive thread group status.
    pub fn new_alive() -> Self {
        Self {
            is_dethreading: false,
            life_cycle: ThreadGroupLifeCycle::Alive,
        }
    }

    pub fn life_cycle(&self) -> ThreadGroupLifeCycle {
        self.life_cycle
    }

    pub fn is_dethreading(&self) -> bool {
        self.is_dethreading
    }

    /// Whether clone operation can add a new thread to this thread group.
    pub fn can_join(&self) -> bool {
        matches!(self.life_cycle, ThreadGroupLifeCycle::Alive) && !self.is_dethreading
    }

    // no setter methods. state transition should only occur in task module itself,
    // where we can set value to fields directly.
}

/// course-grained locking. optimize later.
#[derive(Debug)]
struct ThreadGroupInner {
    /// Running status of this thread group.
    status: ThreadGroupStatus,
    /// TIDs of all member threads, including the leader thread.
    members: BTreeSet<Tid>,
    /// Tgid of parent thread group. [None] for init/idle thread group.
    parent_tgid: Option<Tid>,
    /// Tgids of all child thread groups.
    children_tgids: BTreeSet<Tid>,
    /// CPU usage of this thread group.
    cpu_usage: ThreadGroupCpuUsage,
}

// endregion: definitions

// region: spawn

impl Task {
    /// Create a new kernel task with a kernel stack and kernel entry context.
    ///
    /// Parameters represented in [ParameterList] are passed to the entry
    /// function using the C calling convention.
    ///
    /// [TaskFlags::KERNEL] is added automatically.
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
    ///
    /// By default, tgid is set to the same as newly allocated tid. You can also
    /// specify tgid explicitly. However, real jointing of the thread group is
    /// done when registering the task to global topology.
    ///
    /// Both leader thread and non-leader thread will get a [TidRef::Owned] at
    /// first. But when latering creating a new [ThreadGroup], the owned handle
    /// of leader thread will be converted to [TidRef::Leader], and the
    /// [TidHandle] will be put into [ThreadGroup].
    pub unsafe fn new_kernel(
        name: &str,
        entry: *const (),
        args: ParameterList,
        creator: Option<Tid>,
        tgid: Option<Tid>,
        sched: SchedEntity,
        flags: TaskFlags,
        cpu: Option<CpuId>,
    ) -> Result<(Task, PublishGuard), SysError> {
        let tid = alloc_tid().ok_or(SysError::OutOfMemory)?;
        let tgid = tgid.unwrap_or(tid.get_typed());
        let stack = KernelStack::new()?;
        let stack_top = stack.stack_top();
        let task = Self {
            tid: RwLock::new(TidRef::Owned(tid)),
            creator,
            tgid,
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
            cpu_usage: RwLock::new(TaskCpuUsage::ZERO),
            exit_code: SpinLock::new(None),
            status: RwLock::new(TaskStatus::Runnable),
            clear_child_tid: SpinLock::new(None),

            killed: AtomicBool::new(false),
        };
        Ok((task, PublishGuard))
    }

    /// Create a new idle task with `tid` [TIDH_IDLE] and the [TaskFlags::IDLE]
    /// flag.
    ///
    /// # Safety
    /// This function is unsafe because idle tasks are special tasks that should
    /// not be created casually.
    pub unsafe fn new_idle(entry: *const ()) -> Result<(Task, PublishGuard), SysError> {
        let stack = KernelStack::new()?;
        let stack_top = stack.stack_top();
        Ok((
            Task {
                tid: RwLock::new(TidRef::Idle),
                creator: None,
                tgid: Tid::IDLE,
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
                cpu_usage: RwLock::new(TaskCpuUsage::ZERO),
                exit_code: SpinLock::new(None),
                status: RwLock::new(TaskStatus::Runnable),
                clear_child_tid: SpinLock::new(None),

                killed: AtomicBool::new(false),
            },
            PublishGuard,
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
    ///
    /// Unintuitively, this function will get the lock on ‵tid‵ field...
    pub fn tid(&self) -> Tid {
        match &*self.tid.read() {
            TidRef::Idle => Tid::IDLE,
            TidRef::Owned(h) => h.get_typed(),
            TidRef::Leader => self.tgid, // for leader thread, tid is the same as tgid
        }
    }

    /// Get task ID of the creator of this task.
    ///
    /// Panics if idle or init task calls this function, since they have no
    /// creator.
    pub fn creator_tid(&self) -> Tid {
        self.creator.unwrap()
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

    /// Get the exit code of this task.
    pub fn exit_code(&self) -> Option<ExitCode> {
        self.exit_code.lock().clone()
    }

    /// Set the exit code of this task. This should only be called once.
    pub fn set_exit_code(&self, code: ExitCode) {
        let mut exit_code = self.exit_code.lock();
        if let Some(old) = *exit_code {
            panic!(
                "task {}: exit code is already set to {:?}, cannot set to {:?}",
                self.tid(),
                old,
                code
            );
        }
        *exit_code = Some(code);
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
    pub fn set_clear_child_tid(&self, tid_ptr: Option<VirtAddr>) {
        *self.clear_child_tid.lock() = tid_ptr;
    }

    /// Get the current clear-child-tid target pointer.
    pub fn get_clear_child_tid(&self) -> Option<VirtAddr> {
        self.clear_child_tid.lock().clone()
    }

    /// Whether this task is killed.
    pub fn killed(&self) -> bool {
        self.killed.load(Ordering::Acquire)
    }

    /// Kill this task. This is a one-way operation.
    ///
    /// This will try to wake up this task if it's sleeping.
    pub fn set_killed(self: &Arc<Self>) {
        self.killed.store(true, Ordering::Release);

        notify(self);
    }
}

// endregion: common field accessors

// region: core trait implementations

// Eq, PartialEq traits are a bit tricky. Since when dethreading, the non-leader
// thread will take the same Tid as the leader thread, so we cannot simply
// compare Tids. Maybe we should just compare the addresses of Arc?

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

// endregion: core trait implementations
