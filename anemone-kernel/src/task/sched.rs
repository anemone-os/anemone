use crate::{prelude::*, sched::class::SchedEntity};

impl Task {
    /// Get a copy of the scheduling entity of this task.
    ///
    /// If you just want to read some fields of the scheduling entity, consider
    /// using [Self::with_sched_entity_mut] instead to avoid unnecessary bytes
    /// copying.
    pub fn sched_entity(&self) -> SchedEntity {
        *self.sched_entity.lock()
    }

    /// Run a closure with a mutable reference to the scheduling entity of this
    /// task.
    pub fn with_sched_entity_mut<F: FnOnce(&mut SchedEntity) -> R, R>(&self, f: F) -> R {
        let mut guard = self.sched_entity.lock();
        f(&mut guard)
    }

    /// Get the current task status.
    ///
    /// **Do not call this method unless you are those observers and your work
    /// dosn't rely on the accuracy of the status, or you can ensure that you
    /// just need a single read.**
    pub fn status(&self) -> TaskStatus {
        self.status.read().clone()
    }

    /// Run a closure with current task status, and update the status with the
    /// returned one.
    ///
    /// Note that we don't provide a method that just updates the status. Status
    /// updating is always tied to certain transaction, and this method can
    /// ensure the atomicity of the transaction and the status update.
    ///
    /// **Only scheduling primitives have the right to call this method.**
    pub fn update_status_with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(TaskStatus) -> (TaskStatus, R),
    {
        let mut guard = self.status.write();
        let (status, r) = f(*guard);
        *guard = status;
        r
    }

    /// Get the cpu id this task is scheduled to run on.
    pub fn cpuid(&self) -> CpuId {
        self.cpuid
    }
}
