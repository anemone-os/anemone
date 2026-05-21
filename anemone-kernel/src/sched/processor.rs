//! Per processor scheduler state.
//!
//! **We don't support cross-core scheduling, so there is no reason that you
//! should access another processor's state!**
//!
//! TODO: use intr lock to refactor this module to make it more robust with type
//! system guarantees.

use crate::{
    prelude::*,
    sched::class::{OnTickAction, RunQueue},
};

/// This struct must be accessed with interrupts disabled. it's a bit overkill,
/// but it's the simplest way to guarantee that there won't be any race
/// conditions.
struct Processor {
    /// [None] only when the system is not fully initialized. After
    /// initialization, this field is always [Some].
    ///
    /// TODO: We should abstract scheduler itself into a task.
    running_task: Option<Arc<Task>>,
    // TODO: ipi wakeup list.
    runq: RunQueue,
    sched_ctx: TaskContext,
    need_resched: bool,
}

#[percpu]
static PROCESSOR: Processor = Processor {
    running_task: None,
    runq: RunQueue::new(),
    sched_ctx: TaskContext::ZEROED,
    need_resched: false,
};

/// Get the scheduler context of this processor.
///
/// **You should never convert this raw pointer back to a reference.**
///
/// This function automatically disables interrupts.
pub unsafe fn get_local_sched_ctx() -> *const TaskContext {
    with_intr_disabled(|| PROCESSOR.with(|proc| &proc.sched_ctx as *const _))
}

/// Get the mutable scheduler context of this processor.
///
/// **You should never convert this raw pointer back to a reference.**
///
/// This function automatically disables interrupts.
pub unsafe fn get_local_sched_ctx_mut() -> *mut TaskContext {
    with_intr_disabled(|| PROCESSOR.with_mut(|proc| &mut proc.sched_ctx as *mut _))
}

/// Set the current running task of this processor.
///
/// **This function can only be called by [scheduler], which should ensure that
/// interrupts are disabled.**
pub fn set_current_task(option: Option<Arc<Task>>) {
    debug_assert!(IntrArch::local_intr_disabled());
    PROCESSOR.with_mut(|proc| {
        proc.running_task = option;
    });
}

/// Get current running task of this processor. Internally, this function does a
/// clone of the [Arc].
///
/// Something like `with_current_task` is intentionally not provided. If it
/// exists, then it must disable preemption, which is not ideal. What't
/// more, if a trap happens in the closure, the kernel will panic due to
/// reentrency of [MonoFlow] if the trap handler also tries to access the
/// current task (which is almost always the case).
///
/// Actually cloning an [Arc] is quite a cheap operation, and this function has
/// the effect of "pinning" the current task to current context, which is good.
///
/// This function automatically disables interrupts.
///
/// # Panics
///
/// This function will panic if there is no running task, which should never
/// happen in a properly working system.
pub fn get_current_task() -> Arc<Task> {
    with_intr_disabled(|| {
        PROCESSOR.with(|proc| proc.running_task.as_ref().expect("no running task").clone())
    })
}

/// Mark the current running task of this processor as needing a reschedule.
///
/// This function will disable interrupts, since need_resched flag will also be
/// accessed by ipi handler.
pub fn mark_need_resched() {
    with_intr_disabled(|| {
        PROCESSOR.with_mut(|proc| proc.need_resched = true);
    })
}

/// Fetch whether the current running task of this processor needs a reschedule,
/// and clear the flag.
///
/// This function will disable interrupts, since need_resched flag will also be
/// accessed by ipi handler.
pub fn fetch_clear_need_resched() -> bool {
    with_intr_disabled(|| {
        PROCESSOR.with_mut(|proc| {
            let need = proc.need_resched;
            proc.need_resched = false;
            need
        })
    })
}

/// Enqueue a task to the run queue of this processor.
///
/// [class::SchedEntity] of the task will determine which queue it will be
/// enqueued to.
///
/// This entry point is for wakeup and newly-created runnable tasks. The
/// current running task should use [local_requeue_current] instead.
///
/// This function will disable interrupts.
///
/// # Panics
///
/// This function will panic if the task is not in [TaskStatus::Runnable]
/// status.
pub fn local_enqueue(task: Arc<Task>) {
    assert!(task.cpuid() == cur_cpu_id());
    assert!(task.status() == TaskStatus::Runnable);

    with_intr_disabled(|| {
        PROCESSOR.with_mut(|proc| {
            // TODO: when Task::eq supports idle task, we can remove Arc::ptr_eq and just
            // use == here.
            if Arc::ptr_eq(
                &task,
                proc.running_task
                    .as_ref()
                    .expect("see comments on Processor::running_task for details"),
            ) {
                knoticeln!(
                    "{} is already running on cpu {}, not enqueuing it to run queue",
                    task.tid(),
                    cur_cpu_id(),
                );
                return;
            }

            if !task.with_sched_entity_mut(|se| se.on_runq()) {
                proc.runq.enqueue(task);
            } else {
                knoticeln!(
                    "{} is already on run queue, not enqueuing it again",
                    task.tid()
                );
            }
        });
    });
}

/// Requeue the current running task back to its local run queue.
///
/// This is used by [schedule] when the current task remains runnable across a
/// context switch.
///
/// This function will disable interrupts.
///
/// # Panics
///
/// This function will panic if the task is not in [TaskStatus::Runnable]
/// status.
pub fn local_requeue_current(task: Arc<Task>) {
    assert!(task.cpuid() == cur_cpu_id());
    assert!(task.status() == TaskStatus::Runnable);

    with_intr_disabled(|| {
        PROCESSOR.with_mut(|proc| {
            debug_assert!(
                Arc::ptr_eq(
                    &task,
                    proc.running_task
                        .as_ref()
                        .expect("see comments on Processor::running_task for details"),
                ),
                "only the current running task can be requeued through this path"
            );
            debug_assert!(
                !task.with_sched_entity_mut(|se| se.on_runq()),
                "current running task should not already be on run queue"
            );
            proc.runq.enqueue(task);
        });
    });
}

/// Pick the next task to run from the run queue of this processor.
///
/// This function never returns [None], since there is always an idle task in
/// the run queue.
///
/// ** Interrupts must be disabled when calling this function.**
pub fn local_pick_next() -> Arc<Task> {
    debug_assert!(IntrArch::local_intr_disabled());
    let task = PROCESSOR.with_mut(|proc| proc.runq.pick_next());
    debug_assert!(task.status() == TaskStatus::Runnable);
    task
}

/// Called by timer interrupt handler to update the state of schedulers.
///
/// **Only timer interrupt handler should call this function.**
pub fn local_sched_tick() {
    debug_assert!(IntrArch::local_intr_disabled());

    let action = PROCESSOR.with_mut(|proc| {
        proc.runq
            .on_tick(proc.running_task.as_ref().expect("no running task"))
    });
    if let Some(action) = action {
        match action {
            OnTickAction::Resched => mark_need_resched(),
        }
    }
}

/// Pick the next cpu to schedule a new task on. This is used when creating a
/// new task.
///
/// TODO: better strategy for load balancing.
pub fn pick_next_cpu() -> CpuId {
    // currently a simple round-robin strategy is used.
    static NEXT_CPU: AtomicUsize = AtomicUsize::new(0);
    let ncpus = ncpus();
    let cpu = NEXT_CPU.fetch_add(1, Ordering::Relaxed) % ncpus;
    CpuId::new(cpu)
}

/// Enqueue a task to the run queue of another processor.
///
/// Internally, this function sends an IPI to the target processor, which is a
/// bit expensive.
pub fn remote_enqueue(task: Arc<Task>) {
    assert!(task.status() == TaskStatus::Runnable);
    send_ipi(
        task.cpuid().get(),
        IpiPayload::WakeUpTask { tid: task.tid() },
    )
    .expect("failed to enqueue task to another cpu");
}

/// Thin wrapper around [local_enqueue] and [remote_enqueue].
pub fn task_enqueue(task: Arc<Task>) {
    assert!(task.status() == TaskStatus::Runnable);
    if task.cpuid() == cur_cpu_id() {
        local_enqueue(task);
    } else {
        remote_enqueue(task);
    }
}

pub mod init_routines {
    use super::*;

    /// First task to be scheduled on each cpu must be treated specially, since
    /// there is no running task on the cpu at that time. But [local_enqueue]
    /// assumes that there is always a running task.
    ///
    /// This function should be called by bootstrap code path to spawn
    /// [bsp_kinit]/[ap_kinit] tasks.
    pub fn local_enqueue_first(task: Arc<Task>) {
        assert!(task.cpuid() == cur_cpu_id());
        assert!(task.status() == TaskStatus::Runnable);

        with_intr_disabled(|| {
            PROCESSOR.with_mut(|proc| {
                debug_assert!(
                    !task.with_sched_entity_mut(|se| se.on_runq()),
                    "current running task should not already be on run queue"
                );
                proc.runq.enqueue(task);
            });
        });
    }
}
