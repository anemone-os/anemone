use alloc::sync::Arc;
use kernel_macros::percpu;

use crate::{
    mm::kptable::KERNEL_MEMSPACE, prelude::*, sched::idle::clone_current_idle_task,
    sync::mono::MonoFlow, task::tid::Tid,
};

/// Per-CPU processor information
#[percpu]
static PROCESSOR: ProcessorInfo = ProcessorInfo::EMPTY;

pub struct ProcessorInfo {
    /// Scheduler is per-CPU
    sched: RwLock<Scheduler>,
    inner: MonoFlow<ProcessorInner>,
}

pub struct ProcessorInner {
    running_task: Option<Arc<Task>>,
    /// The context used for scheduling
    sched_context: TaskContext,
}

impl ProcessorInfo {
    pub const EMPTY: Self = Self {
        inner: unsafe {
            MonoFlow::new(ProcessorInner {
                running_task: None,
                sched_context: TaskContext::ZEROED,
            })
        },
        sched: RwLock::new(Scheduler::EMPTY),
    };
}

/// Add a task to the ready queue of the current processor.
pub fn add_to_ready(task: Arc<Task>) {
    PROCESSOR.with(|f| f.sched.write_irqsave().add_to_ready(task))
}

/// Fetch new task. This will remove the task from the ready queue
///
/// The old task should be manually added to the ready queue by calling
/// [add_to_ready] if it is still runnable.
pub fn fetch_new_task() -> Arc<Task> {
    PROCESSOR
        .with(|f| {
            f.sched.write_irqsave().fetch_next() // intr is already disabled in scheduler
        })
        .unwrap_or(clone_current_idle_task())
}

/// Get the current task id **without creating a copy of the current task**
///
/// Use this function instead of `clone_current_task().tid()`.
pub fn current_task_id() -> Tid {
    PROCESSOR
        .with(|f| {
            f.inner
                .with(|inner| Some((inner.running_task.as_ref())?.tid()))
        })
        .expect("Scheduler not initialized: no running task found")
}

/// Get a copy of the current task name **without creating a copy of the current
/// task**
///
/// Use this function instead of `clone_current_task().tid()`.
pub fn current_task_name() -> Box<str> {
    PROCESSOR
        .with(|f| {
            f.inner
                .with(|inner| Some(Box::from((inner.running_task.as_ref())?.name())))
        })
        .expect("Scheduler not initialized: no running task found")
}

/// Create a clone of the current task.
///
/// This will increase the reference count of the current task by 1, along with
/// memory allocation for the clone.
///
/// Use [get_current_task_context], [get_current_task_context_mut] or
/// [current_task_id] instead as much as possible.
pub fn clone_current_task() -> Arc<Task> {
    PROCESSOR
        .with(|f| {
            f.inner
                .with(|inner| Some(inner.running_task.as_ref()?.clone()))
        })
        .expect("Scheduler not initialized: no running task found")
}

/// Capture the current task with a reference to it.
/// 
/// This function will disable preemption during the execution of the closure.
pub fn with_current_task<F: Fn(&Arc<Task>) -> R, R>(f: F) -> R {
    PROCESSOR
        .with(|p| {
            p.inner.with(|inner| {
                let running_task = inner.running_task.as_ref()?;
                Some(f(running_task))
            })
        })
        .expect("Scheduler not initialized: no running task found")
}

/// Get a const [TaskContext] pointer **without creating a copy of the current
/// task**
///
/// Use this function instead of `clone_current_task().get_task_context()`.
pub unsafe fn get_current_task_context() -> *const TaskContext {
    PROCESSOR
        .with(|f| {
            f.inner
                .with(|inner| Some(unsafe { inner.running_task.as_ref()?.get_task_context() }))
        })
        .expect("Scheduler not initialized: no running task found")
}

/// Get a mutable [TaskContext] pointer **without creating a copy of the current
/// task**
///
/// Use this function instead of `clone_current_task().get_task_context_mut()`.
pub unsafe fn get_current_task_context_mut() -> *mut TaskContext {
    PROCESSOR
        .with(|f| {
            f.inner
                .with(|inner| Some(unsafe { inner.running_task.as_ref()?.get_task_context_mut() }))
        })
        .expect("Scheduler not initialized: no running task found")
}

/// Get a scheduler context pointer of the current processor.
pub unsafe fn get_sched_context() -> *const TaskContext {
    PROCESSOR.with(|f| {
        f.inner
            .with(|inner| &inner.sched_context as *const TaskContext)
    })
}

/// Get a mutable scheduler context pointer of the current processor.
pub unsafe fn get_sched_context_mut() -> *mut TaskContext {
    PROCESSOR.with(|f| {
        f.inner
            .with_mut(|inner| &mut inner.sched_context as *mut TaskContext)
    })
}

/// Switch out the current task and switch to the next task.
///
/// If `exit` is true, the current task will not be added back to the ready
/// queue and will be dropped instead. Then the [Task] struct of the current
/// task will be deallocated if no external references to it exist.
///
/// ***Make sure interrupts are disabled before calling this function***
pub unsafe fn switch_out(exit: bool) {
    let task = clone_current_task();
    let context = unsafe { task.get_task_context_mut() };
    if !exit && !task.flags().contains(TaskFlags::IDLE) {
        add_to_ready(task);
    } else {
        drop(task);
    }
    let sched_context = unsafe { get_sched_context() };
    unsafe {
        SchedArch::switch(context, sched_context);
    }
}

unsafe fn switch_memspace(cur_task: &Arc<Task>, next_task: &Arc<Task>) {
    unsafe {
        if next_task.memspace().eq(&cur_task.memspace()) {
            // same addr
            return;
        }
        if let Some(next_memsp) = next_task.memspace() {
            // user task
            PagingArch::activate_addr_space(next_memsp.as_ref());
        } else {
            // kernel task
            PagingArch::activate_addr_space(KERNEL_MEMSPACE.memspace());
        }
        if let Some(cur_memsp) = cur_task.memspace() {
            // clear tlb
            PagingArch::tlb_shootdown_all();
        }
    }
}

/// Switch to the given task.
///
/// **This function should only be used by the scheduler**
///
/// ***Make sure interrupts are disabled before calling this function***
pub unsafe fn switch_to(task: Arc<Task>) {
    let cur_context = unsafe { get_sched_context_mut() };
    let next_task = task;
    let next_context = unsafe { next_task.get_task_context() };
    unsafe {
        switch_memspace(&clone_current_task(), &next_task);
        set_running_task(next_task);
        SchedArch::switch(cur_context, next_context);
    }
}

/// Set the current running task
///
/// **This function should only be used by the scheduler**
///
/// ***Make sure interrupts are disabled before calling this function***
pub unsafe fn set_running_task(task: Arc<Task>) {
    PROCESSOR.with(|proc| proc.inner.with_mut(|inner| inner.running_task = Some(task)));
}
