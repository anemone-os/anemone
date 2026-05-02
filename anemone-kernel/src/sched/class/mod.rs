//! Scheduler class.

use crate::{
    prelude::*,
    sched::class::{idle::Idle, rr::RoundRobin},
};

/// TODO: list those system invariants that must be maintained by the scheduling
/// class. e.g. [TaskStatus].
pub trait Scheduler: Send + Sync {
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
    ///
    /// **This method should never access current processor's percpu variable,
    /// otherwise a reentrancy will occur and lead to panic.**
    ///
    /// The `cur_task` is guaranteed to have the same scheduling class as this
    /// scheduler.
    fn on_tick(&mut self, cur_task: &Arc<Task>) -> Option<OnTickAction>;
}

/// Action to be taken on certain timer tick.
pub enum OnTickAction {
    Resched,
}

pub mod idle;
pub mod rr;
// TODO: realtime, eevdf.

/// PerCpu run queue.
///
/// Priority (top-down):
/// - [RoundRobin]
/// - [Idle]
///
/// Reference:
/// - https://elixir.bootlin.com/linux/v6.6.32/source/kernel/sched/sched.h#L964
pub struct RunQueue {
    ntasks: usize,

    rr: RoundRobin,
    idle: Idle,
}

impl RunQueue {
    pub const fn new() -> Self {
        Self {
            ntasks: 0,
            rr: RoundRobin::new(),
            idle: Idle,
        }
    }

    pub fn enqueue(&mut self, task: Arc<Task>) {
        self.ntasks += 1;
        task.with_sched_entity_mut(|se| {
            match se.class {
                SchedClassPrv::RoundRobin(()) => self.rr.enqueue(task.clone()),
                SchedClassPrv::Idle(()) => panic!("idle task should not be enqueued"),
            }
            debug_assert!(!se.on_runq, "task is already on run queue");
            se.on_runq = true;
        });
    }

    pub fn dequeue(&mut self, task: &Arc<Task>) {
        task.with_sched_entity_mut(|se| {
            match se.class {
                SchedClassPrv::RoundRobin(()) => {
                    if self.rr.dequeue(task) {
                        self.ntasks -= 1;
                    } else {
                        panic!("task not found in round-robin scheduler");
                    }
                },
                SchedClassPrv::Idle(()) => panic!("idle task should not be dequeued"),
            }
            debug_assert!(se.on_runq, "task is not on run queue");
            se.on_runq = false;
        });
    }

    pub fn pick_next(&mut self) -> Arc<Task> {
        // rr
        if let Some(task) = self.rr.pick_next() {
            self.ntasks -= 1;
            task.with_sched_entity_mut(|se| {
                debug_assert!(se.on_runq, "task is not on run queue");
                se.on_runq = false;
            });
            return task;
        }

        // idle
        self.idle
            .pick_next()
            .expect("idle scheduler should always have a task to run")
    }

    pub fn on_tick(&mut self, task: &Arc<Task>) -> Option<OnTickAction> {
        match task.with_sched_entity_mut(|se| se.class) {
            SchedClassPrv::Idle(()) => self.idle.on_tick(task),
            SchedClassPrv::RoundRobin(()) => self.rr.on_tick(task),
        }
    }
}

/// [Copy] is implemented cz we expect this struct should be a POD type.
#[derive(Debug, Clone, Copy)]
pub struct SchedEntity {
    on_runq: bool,
    class: SchedClassPrv,
}

impl SchedEntity {
    /// Create a new scheduling entity with the given scheduling class.
    pub fn new(class: SchedClassPrv) -> Self {
        Self {
            on_runq: false,
            class,
        }
    }

    /// **`on_runq` should never be accessed on a cpu which does not own the
    /// task. Correctness of scheduling system relies on this invariant.**
    pub fn on_runq(&self) -> bool {
        self.on_runq
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SchedClassPrv {
    // TODO: time slice.
    RoundRobin(()),
    Idle(()),
}
