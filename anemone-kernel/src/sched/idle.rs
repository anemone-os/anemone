// TODO: idle should be a scheduler.

use core::hint::spin_loop;

use alloc::sync::Arc;
use kernel_macros::percpu;
use spin::Lazy;

use crate::prelude::*;

/// The idle task that runs when there are no other runnable tasks.
pub extern "C" fn idle() -> ! {
    loop {
        spin_loop();
    }
}

#[percpu]
static IDLE_TASK: Lazy<Arc<Task>> = Lazy::new(|| unsafe {
    let (task, guard) = Task::new_idle(idle as *const ())
        .unwrap_or_else(|e| panic!("failed to create idle tasks: {:?}", e));
    // SAFETY:
    // idle task should not be registered to global task registry.
    unsafe {
        guard.forget();
    }
    Arc::new(task)
});

/// Get a clone of the idle task of the current processor.
pub fn clone_current_idle_task() -> Arc<Task> {
    IDLE_TASK.with(|task| (**task).clone())
}
