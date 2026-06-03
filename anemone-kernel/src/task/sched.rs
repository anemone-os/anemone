use crate::{prelude::*, sched::class::SchedEntity};

impl Task {
    /// Get a copy of the scheduling entity of this task.
    ///
    /// If you just want to read some fields of the scheduling entity, consider
    /// using [Self::with_sched_entity_mut] instead to avoid unnecessary bytes
    /// copying.
    pub fn sched_entity(&self) -> SchedEntity {
        *self.sched_entity.lock_irqsave()
    }

    /// Run a closure with a mutable reference to the scheduling entity of this
    /// task.
    pub fn with_sched_entity_mut<F: FnOnce(&mut SchedEntity) -> R, R>(&self, f: F) -> R {
        let mut guard = self.sched_entity.lock_irqsave();
        f(&mut guard)
    }

    /// Return an observation-only compatibility snapshot.
    ///
    /// This is for procfs, debug, and other status observers that need one
    /// lossy read. Scheduler, wait, wake, and enqueue code must use
    /// scheduler-state helpers or transactions instead.
    pub fn status(&self) -> TaskStatus {
        self.sched_state.read().as_task_status()
    }

    /// Return whether the internal scheduler state is runnable.
    ///
    /// This is for scheduler placement and assertion paths that need a one-shot
    /// state check without going through the [TaskStatus] observation
    /// projection.
    pub(crate) fn is_sched_runnable(&self) -> bool {
        self.sched_state.read().is_runnable()
    }

    /// Get an internal scheduler-state snapshot.
    ///
    /// This is for scheduler/wait-core diagnostics and assertions. State
    /// transitions must use [Self::update_sched_state_with] or a narrower
    /// wait-core capability.
    pub(crate) fn sched_state(&self) -> TaskSchedState {
        self.sched_state.read().clone()
    }

    /// Borrow the internal scheduler state for wait-core owned transactions
    /// that must keep the state lock live across a narrow capability handoff.
    pub(crate) fn sched_state_guard(
        &self,
    ) -> crate::sync::rwlock::WriteIrqSaveGuard<'_, TaskSchedState> {
        self.sched_state.write()
    }

    /// Run a closure with the internal scheduler state and update it with the
    /// returned state in the same NoIrq transaction.
    pub(crate) fn update_sched_state_with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(TaskSchedState) -> (TaskSchedState, R),
    {
        let mut guard = self.sched_state.write();
        let (state, r) = f(guard.clone());
        *guard = state;
        r
    }

    /// Get the cpu id this task is scheduled to run on.
    pub fn cpuid(&self) -> CpuId {
        self.cpuid
    }
}
