//! Deferred task disposal.
//!
//! Allows kernel to free tasks at well-defined points, thus improving the
//! observability of the system and avoiding unexpected performance
//! fluctuations or deadlocks.
//!
//! TODO: should we also defer the disposal of thread groups?

use crate::prelude::*;

use core::{mem::ManuallyDrop, ops::Deref};

/// lock_irqsave should always be used since this may be accessed in trap
/// handlers.
static TASKS_TO_DISPOSE: SpinLock<VecDeque<Arc<Task>>> = SpinLock::new(VecDeque::new());

#[derive(Debug)]
pub struct ReapedTask {
    task: Option<Arc<Task>>,
}

impl ReapedTask {
    /// We don't just let the [Arc] to be dropped naturally when its strong
    /// reference count reaches zero, because we want to control when the
    /// actual destruction happens. Otherwise, the destruction may happen at
    /// unexpected points, which may cause deadlocks or performance
    /// fluctuations. **More importantly, the observability of the system
    /// will be lost.**
    ///
    /// This function put the task into a global deferred queue, and the
    /// actual disposal will be done when [dispose_deferred_tasks] is
    /// called, at some well-defined points in kernel.
    pub fn defer_to_dispose(mut self) {
        TASKS_TO_DISPOSE
            .lock_irqsave()
            .push_back(self.task.take().unwrap());
        let _ = ManuallyDrop::new(self);
    }

    // is there any necessity to provide a method to cancel the defer? i think not.
}

impl Drop for ReapedTask {
    fn drop(&mut self) {
        panic!("a reaped task was dropped without being explicitly deferred");
    }
}

impl Deref for ReapedTask {
    type Target = Task;

    fn deref(&self) -> &Self::Target {
        self.task.as_ref().expect("reaped task is already taken")
    }
}

/// Defer a task to be disposed later. The actual disposal will happen when
/// [dispose_deferred_tasks] is called.
pub fn defer_to_dispose(task: Arc<Task>) {
    TASKS_TO_DISPOSE.lock_irqsave().push_back(task);
}

/// Explicitly dispose those tasks without any other strong reference.
///
/// Only certain points in the code are allowed to call this function. e.g.
/// the end of the scheduler loop, return point of hwirq handlers, etc.
///
/// TODO: if there are some tasks that have been deferred for a long time
/// but still cannot be disposed, report them in some way, because that may
/// indicate some bugs.
pub fn dispose_deferred_tasks() {
    // TODO: make these kconfig items
    const DISPOSE_BATCH_LIMIT: usize = 16;
    const SCAN_BATCH_LIMIT: usize = 64;

    let mut can_be_disposed = vec![];
    {
        let mut tasks = TASKS_TO_DISPOSE.lock_irqsave();

        let scan_budget = tasks.len().min(SCAN_BATCH_LIMIT);

        for _ in 0..scan_budget {
            let Some(task) = tasks.pop_front() else {
                break;
            };

            if Arc::strong_count(&task) == 1 {
                can_be_disposed.push(task);
                if can_be_disposed.len() >= DISPOSE_BATCH_LIMIT {
                    break;
                }
            } else {
                // still alive somewhere else, defer to next round.
                tasks.push_back(task);
            }
        }
    }
    while let Some(task) = can_be_disposed.pop() {
        kdebugln!("disposing {} with tid {}", task.name(), task.tid());
    }
}
