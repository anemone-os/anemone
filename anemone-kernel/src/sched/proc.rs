use alloc::sync::Arc;
use kernel_macros::percpu;

use crate::{
    mm::kptable::activate_kernel_mapping, prelude::*, sched::idle::clone_current_idle_task,
    sync::mono::MonoFlow, task::tid::Tid,
};

/// Per-CPU processor information
#[percpu]
static PROCESSOR: ProcessorInfo = ProcessorInfo::EMPTY;

pub struct ProcessorInfo {
    /// Scheduler is per-CPU
    sched: RwLock<Scheduler>,
    inner: MonoFlow<ProcessorInner>,
    need_resched: AtomicBool,
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
        need_resched: AtomicBool::new(false),
    };
}

pub fn set_resched_flag() {
    unsafe {
        PROCESSOR.unsafe_with(|proc| {
            proc.need_resched.store(true, Ordering::SeqCst);
        })
    }
}

pub fn fetch_clear_resched_flag() -> bool {
    unsafe { PROCESSOR.unsafe_with(|proc| proc.need_resched.fetch_and(false, Ordering::SeqCst)) }
}

/// Add a task to the ready queue of the current processor.
pub fn add_to_ready(task: Arc<Task>) {
    task.set_status(TaskStatus::Ready);
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
        .unwrap_or_else(|| clone_current_idle_task())
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
pub fn current_task_cmdline() -> Box<str> {
    PROCESSOR
        .with(|f| {
            f.inner
                .with(|inner| Some(Box::from((inner.running_task.as_ref())?.cmdline())))
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
pub fn with_current_task<F: FnOnce(&Arc<Task>) -> R, R>(f: F) -> R {
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
///
/// # Safety
/// * **Make sure interrupts are disabled before calling this function,
/// otherwise undefined behavior or unexpected panics may occur.**
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
///
/// # Safety
/// * **Make sure interrupts are disabled before calling this function,
/// otherwise undefined behavior or unexpected panics may occur.**
pub unsafe fn get_current_task_context_mut() -> *mut TaskContext {
    PROCESSOR
        .with(|f| {
            f.inner
                .with(|inner| Some(unsafe { inner.running_task.as_ref()?.get_task_context_mut() }))
        })
        .expect("Scheduler not initialized: no running task found")
}

/// Get a scheduler context pointer of the current processor.
///
/// # Safety
/// * **Make sure interrupts are disabled before calling this function,
/// otherwise undefined behavior or unexpected panics may occur.**
///
/// * **This function may only be called within a single execution flow,
/// typically the task's own execution context.
/// Parallel access will lead to data races.**
pub unsafe fn get_sched_context() -> *const TaskContext {
    PROCESSOR.with(|f| {
        f.inner
            .with(|inner| &inner.sched_context as *const TaskContext)
    })
}

/// Get a mutable scheduler context pointer of the current processor.
///
/// # Safety
/// * **Make sure interrupts are disabled before calling this function,
/// otherwise undefined behavior or unexpected panics may occur.**
///
/// * **This function may only be called within a single execution flow,
/// typically the task's own execution context.
/// Parallel access will lead to data races.**
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

unsafe fn switch_uspace(cur_task: &Arc<Task>, next_task: &Arc<Task>) {
    unsafe {
        let next_uspace = next_task.clone_uspace();
        let cur_uspace = cur_task.clone_uspace();
        if next_uspace.eq(&cur_uspace) {
            // same addr
            return;
        }
        if let Some(next_usersp) = next_uspace {
            // user task
            next_usersp.activate();
        } else {
            // kernel task
            activate_kernel_mapping();
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
        switch_uspace(&clone_current_task(), &next_task);
        next_task.set_status(TaskStatus::Running);
        let prev = exchange_running_task(next_task);
        drop(prev);
        SchedArch::switch(cur_context, next_context);
    }
}

/// Switch to the given task.
///
/// **This function should only be used by the scheduler**
///
/// ***Make sure interrupts are disabled before calling this function***
pub unsafe fn load_context(new: TaskContext) -> ! {
    unsafe {
        kinfoln!("load new context for {}", current_task_id());
        let mut _wasted = TaskContext::ZEROED;
        SchedArch::switch(&mut _wasted, &new);
        unreachable!("should never return to a wasted context");
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

/// Get the current running task
///
/// **This function should only be used by the scheduler**
///
/// ***Make sure interrupts are disabled before calling this function***
pub unsafe fn exchange_running_task(task: Arc<Task>) -> Arc<Task> {
    PROCESSOR.with(|proc| {
        proc.inner.with_mut(|inner| {
            let prev_task = inner
                .running_task
                .replace(task)
                .expect("No running task found");
            prev_task
        })
    })
}
