//! Scheduler class.

use crate::prelude::*;

pub mod idle;
pub mod rr;

mod entity;
mod runqueue;

pub use entity::{SchedClassPrv, SchedEntity};
pub use runqueue::RunQueue;

/// Scheduler-class local transaction surface.
///
/// Each method is one class-owned lifecycle transaction. Scheduler core and
/// [`RunQueue`] choose which transaction happens and maintain global owner CPU
/// state; class implementations keep their own queue/accounting details behind
/// these path-specific methods.
pub trait Scheduler: Send + Sync {
    /// Place a freshly published runnable task.
    fn enqueue_new(&mut self, task: Arc<Task>);

    /// Place a task after stale-safe wake completion produced an enqueue.
    fn enqueue_woken(&mut self, task: Arc<Task>);

    /// Remove a queued task from this class.
    fn dequeue(&mut self, task: &Arc<Task>) -> bool;

    /// Requeue the current task after an explicit yield.
    fn requeue_yielded_current(&mut self, task: Arc<Task>, now: Instant);

    /// Requeue the current task after involuntary preemption.
    fn requeue_preempted_current(&mut self, task: Arc<Task>, now: Instant, pending: PendingResched);

    /// Requeue the current task after a parked wait was woken in place.
    fn handoff_woken_current(&mut self, task: Arc<Task>, now: Instant);

    /// Requeue the current task after wait parking aborted without a wake
    /// reward.
    fn requeue_aborted_wait_current(&mut self, task: Arc<Task>, now: Instant);

    /// Observe that the previous current task blocked and will not be requeued.
    fn put_prev_blocked(&mut self, task: &Arc<Task>, now: Instant);

    /// Observe that the previous current task is exiting and will not be
    /// requeued.
    fn put_prev_exiting(&mut self, task: &Arc<Task>, now: Instant);

    /// Pick and remove the next runnable task from this class.
    fn pick_next_task(&mut self) -> Option<Arc<Task>>;

    /// Mark a picked task as the next execution segment.
    fn set_next_task(&mut self, task: &Arc<Task>, now: Instant);

    /// Timer-tick lifecycle transaction for the running task.
    fn task_tick(&mut self, task: &Arc<Task>, now: Instant) -> TickAction;

    /// Decide whether a newly placed candidate should preempt current.
    fn decide_preempt_current(
        &mut self,
        current: &Arc<Task>,
        candidate: &Arc<Task>,
        now: Instant,
    ) -> PreemptDecision;
}

/// Action requested by a scheduler class on timer tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickAction {
    None,
    RequestResched,
}

/// Preemption decision requested by a scheduler class after placement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreemptDecision {
    KeepCurrent,
    RequestResched,
}

/// Source of a pending scheduler-core reschedule request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReschedCause {
    Tick,
    RunnableArrival,
}

impl ReschedCause {
    const fn bit(self) -> u8 {
        match self {
            Self::Tick => 1 << 0,
            Self::RunnableArrival => 1 << 1,
        }
    }
}

/// Value flags passed into class-local preempted-current transactions.
///
/// This is not a processor-state capability. Scheduler core owns request
/// restore and merging; scheduler classes only read a copied value while
/// handling a preempted-current transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingResched {
    bits: u8,
}

impl PendingResched {
    pub const fn empty() -> Self {
        Self { bits: 0 }
    }

    pub const fn from_cause(cause: ReschedCause) -> Self {
        Self { bits: cause.bit() }
    }

    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }

    pub const fn contains(self, cause: ReschedCause) -> bool {
        self.bits & cause.bit() != 0
    }

    pub fn insert(&mut self, cause: ReschedCause) {
        self.bits |= cause.bit();
    }

    pub const fn union(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }
}

impl Default for PendingResched {
    fn default() -> Self {
        Self::empty()
    }
}
