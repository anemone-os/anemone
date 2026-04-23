pub mod cpu_usage;
pub mod files;
pub mod sig;
#[path = "fs.rs"]
pub mod task_fs;
pub mod tid;
pub mod wait;

mod api;
pub use api::*;
mod task;
pub use task::*;

use crate::{
    mm::stack::KernelStack,
    prelude::{dt::UserWritePtr, *},
    sync::mono::MonoFlow,
    task::{
        clone::CloneFlags,
        cpu_usage::CpuUsage,
        files::FilesState,
        tid::{Tid, TidHandle},
        wait::WaitQueue,
    },
};

/// What we call Task Control Block in kernel terminology.
pub struct Task {
    /// Task identifier handle.
    tid: TidHandle,
    /// Kernel stack owned by this task.
    kstack: KernelStack,
    /// Scheduler-owned context and trap-frame pointer.
    sched_ctx: MonoFlow<TaskSchedCtx>,
    /// Clone behavior flags captured when this task was created.
    create_flags: CloneFlags,
    /// Filesystem state shared by task-related FS operations.
    fs_state: Arc<RwLock<FsState>>,
    /// File descriptor table state.
    files_state: Arc<RwLock<FilesState>>,
    /// Cpu usage information.
    cpu_usage: RwLock<CpuUsage>,

    // execution information
    /// Executable context such as `cmdline`, `flags` and user address space.
    exec_ctx: RwLock<TaskExecCtx>,
    /// Exit status code written when the task terminates.
    exit_code: AtomicI8,

    /// Wait queue used by parent tasks waiting for child exits.
    wait_childexit: WaitQueue<Arc<Task>>,

    // hierarchy information
    /// Parent/children links in the task hierarchy.
    hierarchy: RwLock<TaskHierarchy>,

    // running status
    /// Runtime status visible to scheduler and wait paths.
    status: RwLock<TaskStatus>,

    /// Optional user pointer updated during child clear-tid handling.
    clear_child_tid: RwLock<Option<UserWritePtr<Tid>>>,
}

/// Boot routines and root task.
pub mod boot {

    use spin::Once;

    use super::*;
    /// Global root task holder.
    ///
    /// It is initialized once through [register_root_task] and then used by
    /// hierarchy operations as the fallback ancestor.
    static TASK_ROOT: Once<Arc<Task>> = Once::new();

    /// Register the root task, which is the ancestor of all tasks in the
    /// system.
    ///
    /// **This function should only be called once during the kernel
    /// initialization, otherwise it will do nothing.**
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
}

mod registry {
    use core::{mem::ManuallyDrop, ops::Deref};

    use super::*;

    /// Global task registry. Singleton instance.
    ///
    /// One primary objective of this registry is to transfer all
    /// task-destroying logic to a explicit and controllable point. If we simply
    /// leave that in [Drop], then XXX (dead lock and performance fluctuations).
    #[derive(Debug)]
    struct TaskRegistry {
        /// [Vec] + [HashMap] would be better, but we don't have intrusive list,
        /// so removing tasks from [Vec] will be quite expensive.
        ///
        /// That's why [BTreeMap] is used here, which provides both efficient
        /// lookup (but still slower than [HashMap], generally) and ordered
        /// iteration.
        map: BTreeMap<Tid, Arc<Task>>,
    }

    impl TaskRegistry {
        fn new() -> Self {
            Self {
                map: BTreeMap::new(),
            }
        }
    }

    static REGISTRY: Lazy<RwLock<TaskRegistry>> = Lazy::new(|| RwLock::new(TaskRegistry::new()));

    /// When creating a new task, this guard will be returned as well.
    ///
    /// You must call either `register` or `forget` on the guard, otherwise a
    /// panic will occur when the guard is dropped.
    #[derive(Debug)]
    pub struct RegisterGuard;

    impl RegisterGuard {
        pub fn register(self, task: Arc<Task>) {
            register_task(task);
            let _ = ManuallyDrop::new(self);
        }

        /// Creating a task but not registering it to global registry? You'd
        /// better consider carefully...
        ///
        /// One example: idle tasks indeed need this method, because they are
        /// not registered to the global registry at all.
        pub unsafe fn forget(self) {
            let _ = ManuallyDrop::new(self);
        }
    }

    impl Drop for RegisterGuard {
        fn drop(&mut self) {
            panic!("a task was dropped without being explicitly registered or forgotten");
        }
    }

    /// Register a task into the global registry.
    ///
    /// **This operation must be done if a new task is ready to 'enter' the
    /// system. I.e. all fields/resources are properly initialized.**
    ///
    /// # Panics
    ///
    /// Panics if there is already a task with the same TID in the registry.
    pub fn register_task(task: Arc<Task>) {
        kdebugln!("registering task {} into registry", task.tid());
        let mut registry = REGISTRY.write_irqsave();
        if let Some(old) = registry.map.insert(task.tid(), task.clone()) {
            panic!(
                "task registry: duplicate TID {} (old: {:?}, new: {:?})",
                task.tid(),
                old,
                Some(task)
            );
        }
    }

    /// What [unregister_task] returns. It still holds the task, but you cannot
    /// transform it back to a normal [Task] and re-register it.
    ///
    /// [DerefMut] is intentionally not implemented.
    #[derive(Debug)]
    pub struct UnregisteredTask {
        task: Task,
    }

    impl Deref for UnregisteredTask {
        type Target = Task;

        fn deref(&self) -> &Self::Target {
            &self.task
        }
    }

    /// Unregister a task from the global registry. Returns the unregistered
    /// task if it exists, or `None` if there is no such task.
    ///
    /// This function is not marked as `unsafe`, but it's actually quite unsafe.
    ///
    /// TODO: add doc.
    pub fn unregister_task(tid: &Tid) -> Option<UnregisteredTask> {
        let mut registry = REGISTRY.write_irqsave();
        let task = registry.map.remove(tid)?;
        if Arc::strong_count(&task) > 1 {
            panic!(
                "task registry: unregistering task {} with strong count {}. task is still alive somewhere else.",
                tid,
                Arc::strong_count(&task)
            );
        }
        Arc::try_unwrap(task)
            .ok()
            .map(|task| UnregisteredTask { task })
    }

    /// Get a task by its [Tid].
    pub fn get_task(tid: &Tid) -> Option<Arc<Task>> {
        let registry = REGISTRY.read_irqsave();
        registry.map.get(tid).cloned()
    }

    /// Iterate over all tasks in the registry.
    pub fn for_each_task<F: FnMut(&Arc<Task>)>(mut f: F) {
        let registry = REGISTRY.read_irqsave();
        for task in registry.map.values() {
            f(task);
        }
    }
}
pub use registry::*;
