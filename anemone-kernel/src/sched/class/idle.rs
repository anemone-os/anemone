//! Idle scheduler class. The last-resort scheduler that runs when there are no
//! other runnable tasks.

use crate::{
    prelude::*,
    sched::class::{OnTickAction, SchedClassPrv, Scheduler},
};

pub struct Idle;

impl Scheduler for Idle {
    fn enqueue(&mut self, task: Arc<Task>) {
        panic!("idle scheduler should not be enqueued with any task");
    }

    fn dequeue(&mut self, task: &Arc<Task>) -> bool {
        panic!("idle scheduler should not be dequeued with any task");
    }

    fn pick_next(&mut self) -> Option<Arc<Task>> {
        Some(IDLE_TASK.with(|task| (**task).clone()))
    }

    fn on_tick(&mut self, cur_task: &Arc<Task>) -> Option<OnTickAction> {
        debug_assert!(matches!(
            cur_task.sched_entity().class,
            SchedClassPrv::Idle(())
        ));
        Some(OnTickAction::Resched)
    }
}

mod idle_task {
    use core::hint::spin_loop;

    use super::*;

    extern "C" fn idle_loop() -> ! {
        loop {
            // if kernel preemption is disabled, this step must be taken to ensure other
            // tasks can run.
            unsafe {
                with_intr_disabled(|| {
                    if fetch_clear_need_resched() {
                        schedule();
                    }
                })
            }

            spin_loop();
        }
    }

    #[percpu]
    pub static IDLE_TASK: Lazy<Arc<Task>> = Lazy::new(|| unsafe {
        let (mut task, guard) = Task::new_idle(idle_loop as *const ())
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

/// Get a clone of local processor's idle task.
pub fn clone_local_idle_task() -> Arc<Task> {
    IDLE_TASK.with(|task| (**task).clone())
}
