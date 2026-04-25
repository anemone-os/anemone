//! Scheduler class.

use crate::prelude::*;

/// TODO: list those system invariants that must be maintained by the scheduling
/// class. e.g. [TaskStatus].
pub trait SchedClass: Send + Sync {
    /// Enqueue a task to the ready queue of this scheduling class.
    fn enqueue(&mut self, task: Arc<Task>);

    /// Dequeue a task from the ready queue of this scheduling class.
    ///
    /// Used when:
    /// - a task is killed, so it should be removed from the ready queue,
    /// - a task changed its scheduling policy, so it should be removed from the
    ///   old scheduling class's ready queue.
    ///
    /// etc.
    fn dequeue(&mut self, task: &Arc<Task>) -> bool;

    /// Pick the next task to run from the ready queue of this scheduling class.
    fn pick_next(&mut self) -> Option<Arc<Task>>;

    /// Called on each timer tick. This may be used to update the scheduling
    /// class's internal state, e.g. for time-slice based scheduling classes.
    fn on_tick(&mut self);

    /// Create an empty instance of this scheduling class.
    fn empty() -> Self
    where
        Self: Sized;
}

pub mod idle;
pub mod rr;

pub static SCHED_CLASSES: [Lazy<Box<dyn SchedClass>>; 2] = [
    Lazy::new(|| Box::new(idle::Idle::empty())),
    Lazy::new(|| Box::new(rr::RoundRobin::empty())),
];
