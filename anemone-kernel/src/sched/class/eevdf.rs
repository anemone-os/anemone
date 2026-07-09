//! EEVDF-lite scheduler class scaffold.
//!
//! Checkpoint 2A only introduces the class payload and queue shape. Fair
//! accounting, eligibility, yield penalty, and wake clamp semantics are closed
//! by later phase-2 gates before this class can become the default normal
//! scheduler.

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

    fn requeue_yielded_current(&mut self, task: Arc<Task>, _now: Instant) {
        self.enqueue_back(task);
    }

    fn requeue_preempted_current(
        &mut self,
        task: Arc<Task>,
        _now: Instant,
        _pending: PendingResched,
    ) {
        self.enqueue_back(task);
    }

    fn handoff_woken_current(&mut self, task: Arc<Task>, _now: Instant) {
        self.enqueue_back(task);
    }

    fn requeue_aborted_wait_current(&mut self, task: Arc<Task>, _now: Instant) {
        self.enqueue_back(task);
    }

    fn put_prev_blocked(&mut self, task: &Arc<Task>, _now: Instant) {
        Self::assert_entity(task);
    }

    fn put_prev_exiting(&mut self, task: &Arc<Task>, _now: Instant) {
        Self::assert_entity(task);
    }

    fn pick_next_task(&mut self) -> Option<Arc<Task>> {
        if self.ready_queue.is_empty() {
            None
        } else {
            Some(self.ready_queue.remove(0))
        }
    }

    fn set_next_task(&mut self, task: &Arc<Task>, _now: Instant) {
        Self::assert_entity(task);
    }

    fn task_tick(&mut self, cur_task: &Arc<Task>, _now: Instant) -> TickAction {
        Self::assert_entity(cur_task);
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
