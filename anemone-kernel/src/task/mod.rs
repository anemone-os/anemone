// infras
mod task;
pub use task::*;
mod topology;
pub use topology::*;

pub mod tid;

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

use core::ptr::NonNull;

use crate::{
    mm::stack::KernelStack,
    prelude::{dt::UserWritePtr, *},
    sched::class::SchedEntity,
    sync::mono::MonoFlow,
    task::{
        cpu_usage::CpuUsage,
        files::FilesState,
        tid::{Tid, TidHandle},
    },
};

/// What we call Task Control Block in kernel terminology.
///
/// **LOCK ORDERING**
///
/// **`uspace`->`flags`->`name`**
///
/// TODO: full lock ordering chain.
pub struct Task {
    /// Task identifier handle.
    tid: TidHandle,
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
    cpu_usage: RwLock<CpuUsage>,

    /// Exit status code written when the task terminates.
    exit_code: AtomicI8,

    /// for parent to listen to child exit event.
    child_exited: Event,

    /// See [TaskStatus] for a precise definition.
    status: RwLock<TaskStatus>,

    /// Optional user pointer updated during child clear-tid handling.
    clear_child_tid: RwLock<Option<UserWritePtr<Tid>>>,
}

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
