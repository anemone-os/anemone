//! Per processor scheduler state.
//!
//! **We don't support cross-core scheduling, so there is no reason that you
//! should access another processor's state!**
//!
//! TODO: use intr lock to refactor this module to make it more robust with type
//! system guarantees.

use crate::{
    prelude::*,
    sched::class::{PendingResched, PreemptDecision, ReschedCause, RunQueue, TickAction},
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
    /// Coalesced request latch for the next owner-CPU full pick. A completed
    /// pick acknowledges all prior causes; no-pick paths keep or restore them.
    pending_resched: PendingResched,
}

#[percpu]
static PROCESSOR: Processor = Processor {
    running_task: None,
    runq: RunQueue::new(),
    sched_ctx: TaskContext::ZEROED,
    pending_resched: PendingResched::empty(),
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
    assert!(IntrArch::local_intr_disabled());
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

fn is_local_current_task(task: &Arc<Task>) -> bool {
    assert!(IntrArch::local_intr_disabled());
    PROCESSOR.with(|proc| {
        proc.running_task
            .as_ref()
            .map(|current| Arc::ptr_eq(current, task))
            .unwrap_or(false)
    })
}

/// Add one typed scheduler-core reschedule request.
///
/// This function will disable interrupts, since pending requests are also
/// accessed by IPI and timer handlers.
pub fn request_resched(cause: ReschedCause) {
    with_intr_disabled(|| {
        PROCESSOR.with_mut(|proc| proc.pending_resched.insert(cause));
    })
}

/// Fetch and clear pending scheduler-core reschedule requests.
///
/// This function will disable interrupts, since pending requests are also
/// accessed by IPI and timer handlers.
pub fn take_pending_resched() -> PendingResched {
    with_intr_disabled(|| {
        PROCESSOR.with_mut(|proc| {
            let pending = proc.pending_resched;
            proc.pending_resched = PendingResched::empty();
            pending
        })
    })
}

/// Restore a deferred preemption request without losing concurrently added
/// causes.
pub fn restore_pending_resched(pending: PendingResched) {
    if pending.is_empty() {
        return;
    }
    with_intr_disabled(|| {
        PROCESSOR.with_mut(|proc| {
            proc.pending_resched = proc.pending_resched.union(pending);
        });
    });
}

/// Enqueue a newly published task to the run queue of this processor.
///
/// [class::SchedEntity] of the task will determine which queue it will be
/// enqueued to.
///
/// This entry point is for non-wait-tail placement of tasks that are already
/// known to be runnable, primarily new task publication. Wait completion tails
/// must use [wake_enqueue] so late or stale wake placement is revalidated
/// instead of asserted through this path.
///
/// This function will disable interrupts.
///
/// # Panics
///
/// This function will panic if the task is not internally runnable.
pub fn local_enqueue_new_task(task: Arc<Task>) {
    assert!(task.cpuid() == cur_cpu_id());
    assert!(task.is_sched_runnable());

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
                proc.runq.enqueue_new(task.clone());
                proc.request_runnable_arrival_if_needed(&task);
            } else {
                knoticeln!(
                    "{} is already on run queue, not enqueuing it again",
                    task.tid()
                );
            }
        });
    });
}

/// Stale-safe local physical placement for a task already logically woken by
/// the wait core.
///
/// Unlike [local_enqueue], this entry point never asserts on stale wake tails.
/// It only performs physical placement if the task is still runnable and not
/// already current or queued.
pub fn local_wake_enqueue(task: Arc<Task>, park: ParkState) -> WakeEnqueueResult {
    assert!(task.cpuid() == cur_cpu_id());

    with_intr_disabled(|| {
        let state = task.sched_state();
        if !matches!(state, TaskSchedState::Runnable) {
            kdebugln!(
                "wake_enqueue: task={} stale sched_state={:?}",
                task.tid(),
                state,
            );
            return WakeEnqueueResult::Stale;
        }

        if is_local_current_task(&task) {
            let result = match park {
                ParkState::PrePark => WakeEnqueueResult::AlreadyCurrent,
                ParkState::Parked => WakeEnqueueResult::ParkPending,
            };
            kdebugln!(
                "wake_enqueue: task={} local current park={:?} result={:?}",
                task.tid(),
                park,
                result
            );
            return result;
        }

        if task.with_sched_entity_mut(|se| se.on_runq()) {
            kdebugln!("wake_enqueue: task={} already queued", task.tid());
            return WakeEnqueueResult::AlreadyQueued;
        }

        PROCESSOR.with_mut(|proc| {
            proc.runq.enqueue_woken(task.clone());
            proc.request_runnable_arrival_if_needed(&task);
        });
        kdebugln!("wake_enqueue: task={} enqueued locally", task.tid());
        WakeEnqueueResult::Enqueued
    })
}

fn requeue_current_with<F>(task: Arc<Task>, f: F)
where
    F: FnOnce(&mut RunQueue, Arc<Task>, Instant),
{
    assert!(task.cpuid() == cur_cpu_id());
    assert!(task.is_sched_runnable());

    with_intr_disabled(|| {
        PROCESSOR.with_mut(|proc| {
            assert!(
                Arc::ptr_eq(
                    &task,
                    proc.running_task
                        .as_ref()
                        .expect("see comments on Processor::running_task for details"),
                ),
                "only the current running task can be requeued through this path"
            );
            assert!(
                !task.with_sched_entity_mut(|se| se.on_runq()),
                "current running task should not already be on run queue"
            );
            f(&mut proc.runq, task, Instant::now());
        });
    });
}

/// Requeue the current task after an explicit yield.
pub fn local_requeue_yielded_current(task: Arc<Task>) {
    requeue_current_with(task, |runq, task, now| {
        runq.requeue_yielded_current(task, now);
    });
}

/// Requeue the current task after involuntary preemption.
pub fn local_requeue_preempted_current(task: Arc<Task>, pending: PendingResched) {
    requeue_current_with(task, |runq, task, now| {
        runq.requeue_preempted_current(task, now, pending);
    });
}

/// Requeue the current task after a parked wait completed while current was
/// still running.
pub fn local_handoff_woken_current(task: Arc<Task>) {
    requeue_current_with(task, |runq, task, now| {
        runq.handoff_woken_current(task, now);
    });
}

/// Requeue the current task after wait parking aborted without wake reward.
pub fn local_requeue_aborted_wait_current(task: Arc<Task>) {
    requeue_current_with(task, |runq, task, now| {
        runq.requeue_aborted_wait_current(task, now);
    });
}

fn put_prev_current_with<F>(task: &Arc<Task>, f: F)
where
    F: FnOnce(&mut RunQueue, &Arc<Task>, Instant),
{
    assert!(task.cpuid() == cur_cpu_id());

    with_intr_disabled(|| {
        PROCESSOR.with_mut(|proc| {
            assert!(
                Arc::ptr_eq(
                    task,
                    proc.running_task
                        .as_ref()
                        .expect("see comments on Processor::running_task for details"),
                ),
                "only the current running task can be put through this path"
            );
            assert!(
                !task.with_sched_entity_mut(|se| se.on_runq()),
                "current running task should not already be on run queue"
            );
            f(&mut proc.runq, task, Instant::now());
        });
    });
}

/// Observe that current blocked and will not be requeued.
pub fn local_put_prev_blocked(task: &Arc<Task>) {
    put_prev_current_with(task, |runq, task, now| {
        runq.put_prev_blocked(task, now);
    });
}

/// Observe that current is exiting and will not be requeued.
pub fn local_put_prev_exiting(task: &Arc<Task>) {
    put_prev_current_with(task, |runq, task, now| {
        runq.put_prev_exiting(task, now);
    });
}

/// Pick the next task to run from the run queue of this processor.
///
/// This function never returns [None], since there is always an idle task in
/// the run queue.
///
/// ** Interrupts must be disabled when calling this function.**
pub fn local_pick_next() -> Arc<Task> {
    assert!(IntrArch::local_intr_disabled());
    let task = PROCESSOR.with_mut(|proc| {
        let task = proc.runq.pick_next_task();
        // A full owner-CPU pick satisfies every request pending before
        // selection. Interrupts are disabled here, so this clear cannot race
        // with a new local request; requests raised later remain pending.
        proc.pending_resched = PendingResched::empty();
        proc.runq.set_next_task(&task, Instant::now());
        task
    });
    assert!(task.is_sched_runnable());
    task
}

/// Called by timer interrupt handler to update the state of schedulers.
///
/// **Only timer interrupt handler should call this function.**
pub fn local_sched_tick() {
    assert!(IntrArch::local_intr_disabled());

    let action = PROCESSOR.with_mut(|proc| {
        proc.runq.task_tick(
            proc.running_task.as_ref().expect("no running task"),
            Instant::now(),
        )
    });
    if let TickAction::RequestResched = action {
        request_resched(ReschedCause::Tick);
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
///
/// This is a strict non-wait-tail placement path. Wait completion tails must
/// use [wake_enqueue].
pub fn remote_enqueue_new_task(task: Arc<Task>) {
    assert!(task.is_sched_runnable());
    send_ipi(
        task.cpuid().get(),
        IpiPayload::EnqueueNewTask { tid: task.tid() },
    )
    .expect("failed to enqueue task to another cpu");
}

pub fn remote_wake_enqueue(task: Arc<Task>, park: ParkState) -> WakeEnqueueResult {
    assert!(task.cpuid() != cur_cpu_id());
    let state = task.sched_state();
    if !matches!(state, TaskSchedState::Runnable) {
        kdebugln!(
            "wake_enqueue: task={} stale before remote placement sched_state={:?}",
            task.tid(),
            state,
        );
        return WakeEnqueueResult::Stale;
    }

    let tid = task.tid();
    let placement = send_ipi_wait_result(
        task.cpuid().get(),
        IpiPayload::WakeUpTaskStaleSafe { tid, park },
    )
    .expect("failed to enqueue task to another cpu");

    kdebugln!(
        "wake_enqueue: task={} remote placement requested park={:?}",
        tid,
        park
    );
    placement
}

/// Strict non-wait-tail placement wrapper around [local_enqueue_new_task] and
/// [remote_enqueue_new_task].
///
/// New task publication can use this path because the task is already known
/// runnable and has no late wake tail. Wait completion must use [wake_enqueue].
pub fn enqueue_new_task(task: Arc<Task>) {
    assert!(task.is_sched_runnable());
    if task.cpuid() == cur_cpu_id() {
        local_enqueue_new_task(task);
    } else {
        remote_enqueue_new_task(task);
    }
}

/// Stale-safe physical placement used only after wait-core logical wake
/// completion.
pub fn wake_enqueue(task: Arc<Task>, park: ParkState) -> WakeEnqueueResult {
    let state = task.sched_state();
    if !matches!(state, TaskSchedState::Runnable) {
        kdebugln!(
            "wake_enqueue: task={} stale before placement sched_state={:?}",
            task.tid(),
            state,
        );
        return WakeEnqueueResult::Stale;
    }

    if task.cpuid() == cur_cpu_id() {
        local_wake_enqueue(task, park)
    } else {
        remote_wake_enqueue(task, park)
    }
}

pub mod init_routines {
    use super::*;

    /// First task to be scheduled on each cpu must be treated specially, since
    /// there is no running task on the cpu at that time. But
    /// [local_enqueue_new_task] assumes that there is always a running task.
    ///
    /// This function should be called by bootstrap code path to spawn
    /// [bsp_kinit]/[ap_kinit] tasks.
    pub fn local_enqueue_first_new_task(task: Arc<Task>) {
        assert!(task.cpuid() == cur_cpu_id());
        assert!(task.is_sched_runnable());

        with_intr_disabled(|| {
            PROCESSOR.with_mut(|proc| {
                assert!(
                    !task.with_sched_entity_mut(|se| se.on_runq()),
                    "current running task should not already be on run queue"
                );
                proc.runq.enqueue_new(task);
            });
        });
    }
}

impl Processor {
    fn request_runnable_arrival_if_needed(&mut self, candidate: &Arc<Task>) {
        let Some(current) = self.running_task.as_ref() else {
            return;
        };
        let now = Instant::now();
        if self.runq.decide_preempt_current(current, candidate, now)
            == PreemptDecision::RequestResched
        {
            self.pending_resched.insert(ReschedCause::RunnableArrival);
        }
    }
}
