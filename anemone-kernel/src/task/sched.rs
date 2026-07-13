use crate::{
    prelude::*,
    sched::{
        AtomicNice, Nice,
        class::{SchedClassKind, SchedEntity, SchedEntityMutToken},
    },
};

impl Task {
    /// Return this task's validated nice value.
    pub fn nice(&self) -> Nice {
        self.nice.load()
    }

    /// Inherit nice while a child task is still caller-owned and unpublished.
    pub(crate) fn inherit_nice_before_publish(&mut self, nice: Nice) {
        self.nice = AtomicNice::new(nice);
    }

    /// Set the nice value of a published task.
    ///
    /// The first version deliberately does not update the owner CPU runqueue or
    /// split a current execution segment at this call. A later owner-CPU
    /// accounting or placement transaction consumes the new value. Replace
    /// this direct update when dynamic renice is routed as a `RunQueue`
    /// command/IPI.
    pub(crate) fn set_nice(&self, nice: Nice) {
        self.nice.store(nice);
    }

    /// Run a scheduler-class transaction under this task's entity lock.
    ///
    /// The token is constructible only inside the scheduler-class owner. In
    /// particular, ordinary crate callers cannot use this lock bridge to
    /// replace the class, policy, priority, or run-queue membership of a
    /// published task.
    pub(crate) fn with_sched_entity_mut<F: FnOnce(&mut SchedEntity) -> R, R>(
        &self,
        _token: SchedEntityMutToken,
        f: F,
    ) -> R {
        let mut guard = self.sched_entity.lock_irqsave();
        f(&mut guard)
    }

    /// Return an owner-CPU observation of physical run-queue membership.
    ///
    /// This snapshot is only for scheduler-core placement checks. Membership
    /// transitions remain owned by `RunQueue` transactions.
    pub(crate) fn sched_on_runq(&self) -> bool {
        self.sched_entity.lock_irqsave().on_runq()
    }

    /// Return an observation-only scheduler class snapshot.
    ///
    /// This may be used for assertions and diagnostics. Code that changes queue
    /// membership or class-owned state must use scheduler transactions instead
    /// of driving behavior from this lossy class identity.
    pub fn sched_class_kind(&self) -> SchedClassKind {
        self.sched_entity.lock_irqsave().class_kind()
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
