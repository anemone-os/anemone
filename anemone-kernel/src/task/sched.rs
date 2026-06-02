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

    /// Get the current task status.
    ///
    /// **Do not call this method unless you are those observers and your work
    /// dosn't rely on the accuracy of the status, or you can ensure that you
    /// just need a single read.**
    pub fn status(&self) -> TaskStatus {
        self.sched_state.read().as_task_status()
    }

    /// Get the internal scheduler state.
    ///
    /// This is primarily for scheduler/wait-core diagnostics while the wait
    /// refactor is in flight. Most users should keep using [Self::status].
    pub fn sched_state(&self) -> TaskSchedState {
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
