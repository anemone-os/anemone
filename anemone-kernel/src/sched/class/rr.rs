//! Classic round-robin scheduler.
//!
//! TODO: O(1) dequeue is not implemented yet.

use crate::{
    prelude::*,
    sched::class::{OnTickAction, SchedClassPrv, Scheduler},
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
    fn enqueue(&mut self, task: Arc<Task>) {
        debug_assert!(
            self.ready_queue.iter().all(|t| !Arc::ptr_eq(t, &task)),
            "task is already in the ready queue"
        );

        self.ready_queue.push_back(task);
    }

    fn dequeue(&mut self, task: &Arc<Task>) -> bool {
        self.ready_queue
            .iter()
            .position(|t| Arc::ptr_eq(t, task))
            .map(|idx| self.ready_queue.remove(idx).is_some())
            .unwrap_or(false)
    }

    fn pick_next(&mut self) -> Option<Arc<Task>> {
        self.ready_queue.pop_front()
    }

    fn on_tick(&mut self, cur_task: &Arc<Task>) -> Option<OnTickAction> {
        // currently our round-robin scheduler does not support time-slicing.
        debug_assert!(matches!(
            cur_task.sched_entity().class,
            SchedClassPrv::RoundRobin(())
        ));
        Some(OnTickAction::Resched)
    }
}
