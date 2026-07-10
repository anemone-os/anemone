//! Classic round-robin scheduler.
//!
//! TODO: O(1) dequeue is not implemented yet.

use crate::{
    prelude::*,
    sched::class::{PendingResched, PreemptDecision, SchedClassKind, Scheduler, TickAction},
};

pub struct RoundRobin {
    // the core queue.
    ready_queue: VecDeque<Arc<Task>>,
    // TODO: auxiliary map for O(1) dequeue.
}

impl RoundRobin {
    pub const fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }
}

impl Scheduler for RoundRobin {
    const KIND: SchedClassKind = SchedClassKind::RoundRobin;

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
            .map(|idx| self.ready_queue.remove(idx).is_some())
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
        assert!(matches!(
            task.sched_class_kind(),
            SchedClassKind::RoundRobin
        ));
    }

    fn put_prev_exiting(&mut self, task: &Arc<Task>, _now: Instant) {
        assert!(matches!(
            task.sched_class_kind(),
            SchedClassKind::RoundRobin
        ));
    }

    fn pick_next_task(&mut self) -> Option<Arc<Task>> {
        self.ready_queue.pop_front()
    }

    fn set_next_task(&mut self, task: &Arc<Task>, _now: Instant) {
        assert!(matches!(
            task.sched_class_kind(),
            SchedClassKind::RoundRobin
        ));
    }

    fn task_tick(&mut self, cur_task: &Arc<Task>, _now: Instant) -> TickAction {
        // currently our round-robin scheduler does not support time-slicing.
        assert!(matches!(
            cur_task.sched_class_kind(),
            SchedClassKind::RoundRobin
        ));
        TickAction::RequestResched
    }

    fn decide_preempt_current(
        &mut self,
        current: &Arc<Task>,
        candidate: &Arc<Task>,
        _now: Instant,
    ) -> PreemptDecision {
        assert!(matches!(
            current.sched_class_kind(),
            SchedClassKind::RoundRobin | SchedClassKind::Idle
        ));
        assert!(matches!(
            candidate.sched_class_kind(),
            SchedClassKind::RoundRobin
        ));
        PreemptDecision::RequestResched
    }
}

impl RoundRobin {
    fn enqueue_back(&mut self, task: Arc<Task>) {
        assert!(
            self.ready_queue.iter().all(|t| !Arc::ptr_eq(t, &task)),
            "task is already in the ready queue"
        );

        self.ready_queue.push_back(task);
    }
}
