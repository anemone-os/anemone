//! EEVDF-lite scheduler class scaffold.
//!
//! Checkpoint 2A introduced the class payload and queue shape. Checkpoint 2B
//! closes the single EEVDF runtime-accounting boundary; eligibility, yield
//! penalty, and wake clamp semantics are closed by later phase-2 gates before
//! this class can become the default normal scheduler.

use crate::{
    prelude::*,
    sched::class::{PendingResched, PreemptDecision, SchedClassPrv, Scheduler, TickAction},
};

pub type Vruntime = u64;
pub type Deadline = u64;

#[derive(Debug, Clone)]
pub struct EevdfEntity {
    vruntime: Vruntime,
    deadline: Deadline,
    slice: Duration,
    exec_start: Option<Instant>,
    initialized: bool,
    fallback_anomalies: u64,
    last_fallback: Option<EevdfFallback>,
}

impl EevdfEntity {
    pub const fn new() -> Self {
        Self {
            vruntime: 0,
            deadline: 0,
            slice: Duration::from_micros(EEVDF_BASE_SLICE_US),
            exec_start: None,
            initialized: false,
            fallback_anomalies: 0,
            last_fallback: None,
        }
    }

    pub const fn vruntime(&self) -> Vruntime {
        self.vruntime
    }

    pub const fn deadline(&self) -> Deadline {
        self.deadline
    }

    pub const fn slice(&self) -> Duration {
        self.slice
    }

    pub const fn exec_start(&self) -> Option<Instant> {
        self.exec_start
    }

    pub const fn initialized(&self) -> bool {
        self.initialized
    }

    pub const fn fallback_anomalies(&self) -> u64 {
        self.fallback_anomalies
    }

    pub const fn last_fallback(&self) -> Option<EevdfFallback> {
        self.last_fallback
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EevdfFallback {
    NoEligibleTask,
}

pub struct Eevdf {
    ready_queue: Vec<Arc<Task>>,
    rq_vtime: Vruntime,
    fallback_anomalies: u64,
}

impl Eevdf {
    pub const fn new() -> Self {
        Self {
            ready_queue: Vec::new(),
            rq_vtime: 0,
            fallback_anomalies: 0,
        }
    }

    // Keep the entity lock out of the Scheduler trait: class transactions own
    // when they need a short typed payload access, while RunQueue still owns
    // queue membership and global scheduler linearization.
    fn with_entity_mut<R>(task: &Arc<Task>, f: impl FnOnce(&mut EevdfEntity) -> R) -> R {
        task.with_sched_entity_mut(|se| {
            let SchedClassPrv::Eevdf(entity) = &mut se.class else {
                panic!("expected EEVDF entity for task {}", task.tid());
            };
            f(entity)
        })
    }

    fn assert_entity(task: &Arc<Task>) {
        Self::with_entity_mut(task, |_| {});
    }

    fn enqueue_back(&mut self, task: Arc<Task>) {
        Self::assert_entity(&task);
        assert!(
            self.ready_queue.iter().all(|t| !Arc::ptr_eq(t, &task)),
            "task is already in the EEVDF ready queue"
        );

        self.ready_queue.push(task);
    }

    fn set_exec_start(task: &Arc<Task>, now: Instant) {
        Self::with_entity_mut(task, |entity| {
            entity.exec_start = Some(now);
        });
    }

    fn account_current(&mut self, task: &Arc<Task>, now: Instant) {
        Self::with_entity_mut(task, |entity| {
            let Some(exec_start) = entity.exec_start else {
                entity.exec_start = Some(now);
                return;
            };
            let Some(delta_exec) = now.checked_duration_since(exec_start) else {
                return;
            };
            if delta_exec == Duration::ZERO {
                return;
            }

            let delta_vruntime = Self::runtime_delta_to_vruntime(delta_exec);
            entity.vruntime = entity.vruntime.saturating_add(delta_vruntime);
            if entity.deadline <= entity.vruntime {
                entity.deadline = entity
                    .vruntime
                    .saturating_add(Self::slice_to_vruntime(entity));
            }
            entity.exec_start = Some(now);
        });
    }

    fn runtime_delta_to_vruntime(delta: Duration) -> Vruntime {
        // Checkpoint 2B only needs a monotonic runtime scalar to prove the
        // accounting boundary and `exec_start` refresh discipline. Checkpoint
        // 2C replaces this with the accepted weighted virtual-time arithmetic
        // before EEVDF can become the default normal class.
        delta.as_nanos().min(u64::MAX as u128) as Vruntime
    }

    fn slice_to_vruntime(entity: &EevdfEntity) -> Vruntime {
        Self::runtime_delta_to_vruntime(entity.slice)
    }
}

impl Scheduler for Eevdf {
    fn enqueue_new(&mut self, task: Arc<Task>) {
        self.enqueue_back(task);
    }

    fn enqueue_woken(&mut self, task: Arc<Task>) {
        self.enqueue_back(task);
    }

    fn dequeue(&mut self, task: &Arc<Task>) -> bool {
        self.ready_queue
            .iter()
            .position(|t| Arc::ptr_eq(t, task))
            .map(|idx| {
                self.ready_queue.remove(idx);
                true
            })
            .unwrap_or(false)
    }

    fn requeue_yielded_current(&mut self, task: Arc<Task>, now: Instant) {
        self.account_current(&task, now);
        self.enqueue_back(task);
    }

    fn requeue_preempted_current(
        &mut self,
        task: Arc<Task>,
        now: Instant,
        _pending: PendingResched,
    ) {
        self.account_current(&task, now);
        self.enqueue_back(task);
    }

    fn handoff_woken_current(&mut self, task: Arc<Task>, now: Instant) {
        self.account_current(&task, now);
        self.enqueue_back(task);
    }

    fn requeue_aborted_wait_current(&mut self, task: Arc<Task>, now: Instant) {
        self.account_current(&task, now);
        self.enqueue_back(task);
    }

    fn put_prev_blocked(&mut self, task: &Arc<Task>, now: Instant) {
        self.account_current(task, now);
    }

    fn put_prev_exiting(&mut self, task: &Arc<Task>, now: Instant) {
        self.account_current(task, now);
    }

    fn pick_next_task(&mut self) -> Option<Arc<Task>> {
        if self.ready_queue.is_empty() {
            None
        } else {
            Some(self.ready_queue.remove(0))
        }
    }

    fn set_next_task(&mut self, task: &Arc<Task>, now: Instant) {
        Self::set_exec_start(task, now);
    }

    fn task_tick(&mut self, cur_task: &Arc<Task>, now: Instant) -> TickAction {
        self.account_current(cur_task, now);
        // Checkpoint 2A has no EEVDF slice/eligibility decision yet. Keep the
        // directed scaffold preemptible with the same conservative behavior as
        // RR; 2C replaces this with virtual-time policy.
        TickAction::RequestResched
    }

    fn decide_preempt_current(
        &mut self,
        _current: &Arc<Task>,
        candidate: &Arc<Task>,
        _now: Instant,
    ) -> PreemptDecision {
        Self::assert_entity(candidate);
        // This is a scaffold-only conservative request, not the final EEVDF
        // current-vs-candidate policy. Gate 2C owns the real decision.
        PreemptDecision::RequestResched
    }
}
