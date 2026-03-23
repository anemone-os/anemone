use alloc::{collections::vec_deque::VecDeque, sync::Arc};

use crate::{prelude::*, sched::SchedTrait};

pub struct RRScheduler {
    ready_queue: VecDeque<Arc<Task>>,
}

impl SchedTrait for RRScheduler {
    fn add_to_ready(&mut self, task: Arc<Task>) {
        self.ready_queue.push_back(task);
    }

    fn fetch_next(&mut self) -> Option<Arc<Task>> {
        self.ready_queue.pop_front()
    }

    const EMPTY: Self = Self {
        ready_queue: VecDeque::new(),
    };
}
