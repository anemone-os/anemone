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
pub mod credentials;
pub use credentials::{
    CredentialSet, Credentials, Gid, Uid, UserId,
    cap::{Capability, CredCapabilities, FileCapabilities, SecureBits},
};
pub mod files;
pub mod kthread;
pub mod sig;
#[path = "fs.rs"]
pub mod task_fs;
#[path = "itimer.rs"]
pub mod task_itimer;
#[path = "resource/mod.rs"]
pub mod task_resource;
#[path = "sched.rs"]
pub mod task_sched;

mod api;
pub use api::*;

use core::fmt::{Debug, Display};

use crate::{
    mm::stack::KernelStack,
    prelude::*,
    sched::class::{SchedClassPrv, SchedEntity},
    sync::mono::MonoFlow,
    task::{
        cpu_usage::{TaskCpuUsage, ThreadGroupCpuUsage},
        files::FilesState,
        kthread::KThreadTaskLocal,
        sig::{
            PendingSignals, SigNo, TaskSigMaskState, altstack::SigAltStack,
            disposition::SignalDisposition,
        },
        task_itimer::ITimers,
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
/// - **`uspace`->`flags`->`name`**
/// - **`sig_pending`->`sig_mask`->`sig_disposition`**
///
/// TODO: full lock ordering chain.
pub struct Task {
    /// Task ID.
    tid: NoIrqRwLock<TidRef>,
    /// What linux folks call "real parent". Tid of the task who cloned/forked
    /// this task.
    ///
    /// For idle and init tasks, this field is set to [None].
    creator: Option<Tid>,
    /// Thread group ID.
    tgid: Tid,
    /// Task creation time on the kernel monotonic timeline.
    create_instant: Instant,

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
    name: NoIrqRwLock<Box<str>>,

    /// Task attribute flags.
    ///
    /// Memory of TCB is quite precious, so single bool field should not be used
    /// for each flag. Instead, use this bitfield to store all flags.
    flags: NoIrqRwLock<TaskFlags>,

    /// User address space, or [None] for pure kernel tasks.
    usp: RwLock<Option<Arc<UserSpaceHandle>>>,

    /// Which cpu this task is scheduled to run on.
    cpuid: CpuId,

    /// stub.
    nice: AtomicIsize,
    /// Scheduling context. Used for context switching.
    sched_ctx: MonoFlow<TaskContext>,
    /// Scheduling entity. Used for scheduling.
    sched_entity: SpinLock<SchedEntity>,

    /// Whether this task has used FPU. This is used to optimize FPU context
    /// switching.
    fpu_used: AtomicBool,

    /// Filesystem state shared by task-related FS operations.
    fs_state: Arc<RwLock<FsState>>,
    /// File descriptor table state.
    files_state: RwLock<Arc<RwLock<FilesState>>>,
    /// Identity, group, and capability state used for permission checks.
    cred: RwLock<CredentialSet>,
    /// Irreversible bit set by `PR_SET_NO_NEW_PRIVS`.
    no_new_privs: AtomicBool,
    /// Cpu usage information.
    cpu_usage: NoIrqRwLock<TaskCpuUsage>,

    /// Signal disposition table.
    sig_disposition: Arc<NoIrqRwLock<SignalDisposition>>,
    /// Current signal mask state.
    sig_mask: NoIrqSpinLock<TaskSigMaskState>,
    /// Current pending signals. Local to each task.
    sig_pending: NoIrqSpinLock<PendingSignals>,
    /// Alternative signal stack. Local to each task.
    sig_altstack: NoIrqSpinLock<Option<SigAltStack>>,

    /// Robust futex list head.
    robust_list: SpinLock<Option<VirtAddr>>,
    /// Exit code of this task. Meaningful only when this task is a zombie.
    exit_code: SpinLock<Option<ExitCode>>,
    /// Published by the child when a vfork parent may continue.
    vfork_done: Event,

    /// Internal scheduling state. [TaskStatus] is a compatibility projection.
    sched_state: NoIrqRwLock<TaskSchedState>,

    /// Optional user pointer updated during child clear-tid handling.
    ///
    /// This pointer is not guaranteed to be valid. It's user's responsibility.
    clear_child_tid: SpinLock<Option<VirtAddr>>,

    /// Ordinary kthread task-local attachment owned by this task.
    ///
    /// The attachment is installed before the task is published. It is not an
    /// external lifecycle owner; shared lifecycle state lives only in the
    /// `Arc<KThreadControl>` stored inside this attachment.
    kthread: SpinLock<Option<KThreadTaskLocal>>,
}

/// Observation-only compatibility state for status readers.
///
/// [TaskStatus] is a lossy snapshot projected from [TaskSchedState]. It hides
/// wait identity and park state, so scheduler, wait, wake, and enqueue paths
/// must use scheduler-state helpers or transactions instead of this projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Task is observable as schedulable.
    Runnable,
    /// Task is observable as exited and cannot run anymore.
    Zombie,
    /// Task is observable as waiting.
    Waiting { interruptible: bool },
}

bitflags! {
    /// TODO: Remove current flags.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct TaskFlags: u8{
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
    Signaled(SigNo),
}

/// **LOCK ORDERING**
///
/// TOPOLOGY -> Session.inner -> ProcessGroup.inner -> ThreadGroup.inner
#[derive(Debug)]
pub struct Session {
    sid: Tid,
    inner: NoIrqRwLock<SessionInner>,
}

#[derive(Debug)]
struct SessionInner {
    process_groups: BTreeSet<Tid>,
}

/// **LOCK ORDERING**
///
/// TOPOLOGY -> Session.inner -> ProcessGroup.inner -> ThreadGroup.inner
#[derive(Debug)]
pub struct ProcessGroup {
    pgid: Tid,
    sid: Tid,
    inner: NoIrqRwLock<ProcessGroupInner>,
}

#[derive(Debug)]
struct ProcessGroupInner {
    members: BTreeSet<Tid>,
}

/// **LOCK ORDERING**
///
/// TOPOLOGY -> Session.inner -> ProcessGroup.inner -> ThreadGroup.inner
#[derive(Debug)]
pub struct ThreadGroup {
    /// The thread group ID, which is the same as the leader thread's ID.
    tgid: TidHandle,
    ty: ThreadGroupType,
    /// Event that will be published when a child thread group exits.
    child_exited: Event,
    /// Signal to send to parent when this thread group exits.
    terminate_signal: Option<SigNo>,
    /// POSIX interval timers. Shared by all member threads.
    itimers: ITimers,
    inner: NoIrqRwLock<ThreadGroupInner>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadGroupType {
    User,
    KThread,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadGroupStatus {
    /// See [kernel_execve] for details.
    is_dethreading: bool,
    /// Whether this thread group has successfully executed a new image after
    /// fork. Used by setpgid's fork/exec race semantics.
    has_executed: bool,
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
            has_executed: false,
            life_cycle: ThreadGroupLifeCycle::Alive,
        }
    }

    pub fn new_alive_executed() -> Self {
        Self {
            has_executed: true,
            ..Self::new_alive()
        }
    }

    pub fn life_cycle(&self) -> ThreadGroupLifeCycle {
        self.life_cycle
    }

    pub fn is_dethreading(&self) -> bool {
        self.is_dethreading
    }

    pub fn has_executed(&self) -> bool {
        self.has_executed
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
    /// Process group ID cached on the process identity.
    pgid: Option<Tid>,
    /// Session ID cached on the process identity.
    sid: Option<Tid>,
    /// TIDs of all member threads, including the leader thread.
    members: BTreeSet<Tid>,
    /// Tgid of parent thread group. [None] for init/idle thread group.
    parent_tgid: Option<Tid>,
    /// Tgids of all child thread groups.
    children_tgids: BTreeSet<Tid>,
    /// CPU usage of this thread group.
    cpu_usage: ThreadGroupCpuUsage,
    /// Pending signals for this thread group. Shared by all member threads.
    sig_pending: NoIrqSpinLock<PendingSignals>,
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
    ///
    /// Signal-related structs are both initialized to an 'empty' state, which
    /// means no pending signals, no blocked signals and all signals have
    /// default handlers.
    ///
    /// Signal to send to parent when this task exits is set to [None] by
    /// default, which means no signal will be sent.
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

        unsafe {
            Self::new_kernel_with_tid_handle(
                name, entry, args, creator, tgid, sched, flags, cpu, tid,
            )
        }
    }

    /// Create a kernel task from a caller-owned TID handle.
    ///
    /// This exists for fixed kernel identities such as the boot init task and
    /// `kthreadd`. Ordinary callers must use [`Task::new_kernel`] so fixed TID
    /// ownership stays visible at the bootstrap/kthread call site.
    pub(crate) unsafe fn new_kernel_with_tid_handle(
        name: &str,
        entry: *const (),
        args: ParameterList,
        creator: Option<Tid>,
        tgid: Option<Tid>,
        sched: SchedEntity,
        flags: TaskFlags,
        cpu: Option<CpuId>,
        tid: TidHandle,
    ) -> Result<(Task, PublishGuard), SysError> {
        let tid_value = tid.get_typed().get();
        let tgid = tgid.unwrap_or(tid.get_typed());
        let stack = KernelStack::new()?;
        let stack_top = stack.stack_top();
        let create_instant = Instant::now();
        let task = Self {
            tid: NoIrqRwLock::new(TidRef::Owned(tid)),
            creator,
            tgid,
            create_instant,
            kstack: stack,
            name: NoIrqRwLock::new((String::from("@kernel/") + name).into_boxed_str()),
            flags: NoIrqRwLock::new(flags | TaskFlags::KERNEL),
            usp: RwLock::new(None),
            cpuid: if let Some(cpu) = cpu {
                cpu
            } else {
                let cpu = pick_next_cpu();
                kdebugln!(
                    "{}:{}: no cpu specified, picked cpu {}",
                    tid_value,
                    name,
                    cpu
                );

                cpu
            },
            nice: AtomicIsize::new(0),
            sched_ctx: unsafe {
                MonoFlow::new(TaskContext::from_kernel_fn(
                    VirtAddr::new(entry as u64),
                    stack_top,
                    args,
                ))
            },
            sched_entity: SpinLock::new(sched),
            fpu_used: AtomicBool::new(false),
            fs_state: Arc::new(RwLock::new(FsState::new_hanging())),
            files_state: RwLock::new(Arc::new(RwLock::new(FilesState::new()))),
            cred: RwLock::new(CredentialSet::new_root()),
            no_new_privs: AtomicBool::new(false),
            cpu_usage: NoIrqRwLock::new(TaskCpuUsage::ZERO),

            sig_disposition: Arc::new(NoIrqRwLock::new(SignalDisposition::new())),
            sig_mask: NoIrqSpinLock::new(TaskSigMaskState::new()),
            sig_pending: NoIrqSpinLock::new(PendingSignals::new()),
            sig_altstack: NoIrqSpinLock::new(None),

            robust_list: SpinLock::new(None),
            exit_code: SpinLock::new(None),
            vfork_done: Event::new(),
            sched_state: NoIrqRwLock::new(TaskSchedState::Runnable),
            clear_child_tid: SpinLock::new(None),
            kthread: SpinLock::new(None),
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
                tid: NoIrqRwLock::new(TidRef::Idle),
                creator: None,
                tgid: Tid::IDLE,
                create_instant: Instant::now(),
                kstack: stack,
                name: NoIrqRwLock::new(Box::from("@idle")),
                flags: NoIrqRwLock::new(TaskFlags::IDLE | TaskFlags::KERNEL),
                usp: RwLock::new(None),
                cpuid: cur_cpu_id(),
                nice: AtomicIsize::new(0),
                sched_ctx: unsafe {
                    MonoFlow::new(TaskContext::from_kernel_fn(
                        VirtAddr::new(entry as u64),
                        stack_top,
                        ParameterList::empty(),
                    ))
                },
                sched_entity: SpinLock::new(SchedEntity::new(SchedClassPrv::Idle(()))),
                fpu_used: AtomicBool::new(false),
                fs_state: Arc::new(RwLock::new(FsState::new_hanging())),
                files_state: RwLock::new(Arc::new(RwLock::new(FilesState::new()))),
                cred: RwLock::new(CredentialSet::new_root()),
                no_new_privs: AtomicBool::new(false),
                cpu_usage: NoIrqRwLock::new(TaskCpuUsage::ZERO),

                sig_disposition: Arc::new(NoIrqRwLock::new(SignalDisposition::new())),
                sig_mask: NoIrqSpinLock::new(TaskSigMaskState::new()),
                sig_pending: NoIrqSpinLock::new(PendingSignals::new()),
                sig_altstack: NoIrqSpinLock::new(None),

                robust_list: SpinLock::new(None),
                exit_code: SpinLock::new(None),
                vfork_done: Event::new(),
                sched_state: NoIrqRwLock::new(TaskSchedState::Runnable),
                clear_child_tid: SpinLock::new(None),
                kthread: SpinLock::new(None),
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

    /// Get the task creation time on the kernel monotonic timeline.
    pub fn create_instant(&self) -> Instant {
        self.create_instant
    }

    /// Get the task name. This introduces a heap allocation. Pay attention.
    pub fn name(&self) -> Box<str> {
        self.name.read().clone()
    }

    /// Get the task flags.
    ///
    /// We don't provide wrapers for each flag (something like `is_kernel`) on
    /// [Task]. Caller should always have the awareness that flag reading will
    /// involve a lock operation, which is actually a bit dangerous.
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
    pub fn clone_uspace_handle(&self) -> Arc<UserSpaceHandle> {
        self.usp
            .read()
            .as_ref()
            .expect("cannot access user space of a pure kernel task")
            .clone()
    }

    /// See [Self::clone_uspace_handle] for details.
    pub fn try_clone_uspace_handle(&self) -> Option<Arc<UserSpaceHandle>> {
        self.usp.read().as_ref().map(|usp| usp.clone())
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
                "{}: exit code is already set to {:?}, cannot set to {:?}",
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
    pub unsafe fn switch_exec_ctx(
        &self,
        name: Box<str>,
        uspace: Arc<UserSpaceHandle>,
        flags: TaskFlags,
        fpu_used: bool,
    ) {
        // NOTE THE LOCK ORDERING
        let mut usp_ptr = self.usp.write();
        let mut flags_ptr = self.flags.write();
        let mut name_ptr = self.name.write();
        *usp_ptr = Some(uspace);
        *flags_ptr = flags;
        *name_ptr = name;
        self.fpu_used.store(fpu_used, Ordering::Release);
    }

    /// Set `tid_ptr` as the clear-child-tid target pointer.
    pub fn set_clear_child_tid(&self, tid_ptr: Option<VirtAddr>) {
        *self.clear_child_tid.lock() = tid_ptr;
    }

    /// Get the current clear-child-tid target pointer.
    pub fn get_clear_child_tid(&self) -> Option<VirtAddr> {
        self.clear_child_tid.lock().clone()
    }

    /// Get the robust futex list head pointer.
    pub fn robust_list(&self) -> Option<VirtAddr> {
        self.robust_list.lock().clone()
    }

    /// Set the robust futex list head pointer.
    pub fn set_robust_list(&self, head: Option<VirtAddr>) {
        *self.robust_list.lock() = head;
    }

    pub fn fpu_used(&self) -> bool {
        self.fpu_used.load(Ordering::Acquire)
    }

    pub fn set_fpu_used(&self) {
        self.fpu_used.store(true, Ordering::Release);
    }

    pub fn nice(&self) -> isize {
        self.nice.load(Ordering::Acquire)
    }

    pub fn set_nice(&self, nice: isize) {
        self.nice.store(nice, Ordering::Release);
    }

    /// Return a credential snapshot.
    ///
    /// This accessor intentionally clones the credential set so callers do not
    /// hold the credential lock across scheduler, topology, VFS, or user memory
    /// operations.
    pub fn cred(&self) -> CredentialSet {
        self.cred.read().clone()
    }

    /// Replace the whole credential snapshot.
    ///
    /// This method only takes the credential lock. Callers must not hold a
    /// scheduler-state lock while replacing credentials.
    pub fn replace_cred(&self, cred: CredentialSet) {
        *self.cred.write() = cred;
    }

    /// Mutate credentials transactionally under the credential lock.
    ///
    /// The closure must stay local to credential state and must not acquire
    /// scheduler-state locks or perform operations that can block.
    pub fn update_cred_with<F>(&self, f: F) -> Result<(), SysError>
    where
        F: FnOnce(&mut CredentialSet) -> Result<(), SysError>,
    {
        let mut cred = self.cred.write();
        f(&mut cred)
    }

    pub fn has_cap(&self, cap: Capability) -> bool {
        self.cred.read().has_cap_effective(cap)
    }

    pub fn no_new_privs(&self) -> bool {
        self.no_new_privs.load(Ordering::Acquire)
    }

    pub fn set_no_new_privs(&self) {
        self.no_new_privs.store(true, Ordering::Release);
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
            .field("status", &self.status())
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
