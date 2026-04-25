//! Idle scheduler class. The last-resort scheduler that runs when there are no
//! other runnable tasks.

use crate::{prelude::*, sched::class::SchedClass};

pub struct Idle;

impl SchedClass for Idle {
    fn enqueue(&mut self, task: Arc<Task>) {
        panic!("idle scheduler should not be enqueued with any task");
    }

    fn dequeue(&mut self, task: &Arc<Task>) -> bool {
        panic!("idle scheduler should not be dequeued with any task");
    }

    fn pick_next(&mut self) -> Option<Arc<Task>> {
        Some(IDLE_TASK.with(|task| (**task).clone()))
    }

    fn on_tick(&mut self) {
        // nothing to do for idle scheduler on each tick
    }

    fn empty() -> Self
    where
        Self: Sized,
    {
        Idle
    }
}

mod idle_task {
    use core::hint::spin_loop;

    use super::*;

    extern "C" fn idle_loop() -> ! {
        loop {
            spin_loop();
        }
    }

    #[percpu]
    pub static IDLE_TASK: Lazy<Arc<Task>> = Lazy::new(|| unsafe {
        let (task, guard) = Task::new_idle(idle_loop as *const ())
            .unwrap_or_else(|e| panic!("failed to create idle tasks: {:?}", e));
        // SAFETY:
        // idle task should not be registered to global task registry.
        unsafe {
            guard.forget();
        }
        Arc::new(task)
    });
}
use idle_task::*;
