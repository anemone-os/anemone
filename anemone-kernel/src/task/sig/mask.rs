use crate::{prelude::*, task::Task};

use super::{SigNo, set::SigSet};

/// Per-task signal mask state.
///
/// The `restore` slot is the single delayed-restore owner.
/// `active_restore_slot` is identity metadata for linear temporary-mask tokens;
/// it is not a second mask source.
#[derive(Debug)]
pub struct TaskSigMaskState {
    current: SigSet,
    restore: Option<SigSet>,
    active_restore_slot: Option<TemporarySigMaskSlotId>,
    next_restore_slot: TemporarySigMaskSlotId,
}

impl TaskSigMaskState {
    pub const fn new() -> Self {
        Self {
            current: SigSet::new(),
            restore: None,
            active_restore_slot: None,
            next_restore_slot: TemporarySigMaskSlotId::new(1),
        }
    }

    pub(super) fn current(&self) -> SigSet {
        self.current
    }

    fn assert_valid_mask(mask: SigSet) {
        assert!(
            !mask.get(SigNo::SIGKILL) && !mask.get(SigNo::SIGSTOP),
            "SIGKILL and SIGSTOP cannot be masked"
        );
    }

    fn assert_restore_slot_invariant(&self, task_id: Tid, context: &'static str) {
        assert!(
            self.restore.is_some() == self.active_restore_slot.is_some(),
            "temporary signal mask restore slot invariant failed: task={} context={} restore_present={} active_slot={:?}",
            task_id,
            context,
            self.restore.is_some(),
            self.active_restore_slot,
        );
    }

    fn assert_no_pending_restore(&self, task_id: Tid, context: &'static str) {
        self.assert_restore_slot_invariant(task_id, context);
        assert!(
            self.restore.is_none(),
            "ordinary signal mask mutation while temporary restore is pending: task={} context={} active_slot={:?}",
            task_id,
            context,
            self.active_restore_slot,
        );
    }

    fn set_permanent_current(&mut self, task_id: Tid, new_mask: SigSet) {
        self.assert_no_pending_restore(task_id, "set_permanent_current");
        Self::assert_valid_mask(new_mask);
        self.current = new_mask;
    }

    fn mutate_current(
        &mut self,
        task_id: Tid,
        context: &'static str,
        f: impl FnOnce(&mut SigSet),
    ) -> SigSet {
        self.assert_no_pending_restore(task_id, context);
        self.mutate_current_allowing_pending_restore(f)
    }

    fn mutate_current_for_signal_delivery(&mut self, f: impl FnOnce(&mut SigSet)) -> SigSet {
        self.mutate_current_allowing_pending_restore(f)
    }

    fn mutate_current_allowing_pending_restore(&mut self, f: impl FnOnce(&mut SigSet)) -> SigSet {
        let old_mask = self.current;
        f(&mut self.current);
        Self::assert_valid_mask(self.current);
        old_mask
    }

    fn begin_temporary(&mut self, task_id: Tid, new_mask: SigSet) -> TemporarySigMaskSlotId {
        self.assert_no_pending_restore(task_id, "begin_temporary_sig_mask");
        Self::assert_valid_mask(new_mask);

        let old_mask = self.current;
        let slot = self.next_restore_slot;
        self.next_restore_slot = self.next_restore_slot.next();
        self.restore = Some(old_mask);
        self.active_restore_slot = Some(slot);
        self.current = new_mask;
        self.assert_restore_slot_invariant(task_id, "begin_temporary_sig_mask");
        slot
    }

    fn assert_active_restore_slot(
        &self,
        task_id: Tid,
        slot: TemporarySigMaskSlotId,
        context: &'static str,
    ) {
        self.assert_restore_slot_invariant(task_id, context);
        assert!(
            self.active_restore_slot == Some(slot),
            "temporary signal mask token slot mismatch: task={} context={} token_slot={:?} active_slot={:?}",
            task_id,
            context,
            slot,
            self.active_restore_slot,
        );
    }

    fn restore_temporary_now(&mut self, task_id: Tid, slot: TemporarySigMaskSlotId) {
        self.assert_active_restore_slot(task_id, slot, "restore_temporary_now");
        let old_mask = self
            .restore
            .take()
            .expect("active temporary mask slot must have a restore mask");
        self.active_restore_slot = None;
        Self::assert_valid_mask(old_mask);
        self.current = old_mask;
        self.assert_restore_slot_invariant(task_id, "restore_temporary_now");
    }

    fn assert_defer_slot(&self, task_id: Tid, slot: TemporarySigMaskSlotId) {
        self.assert_active_restore_slot(task_id, slot, "defer_temporary_to_signal_delivery");
    }

    fn sigmask_to_save_for_signal_frame(&self) -> SigSet {
        self.restore.unwrap_or(self.current)
    }

    fn signal_frame_committed_restore_mask(&mut self, task_id: Tid) {
        self.assert_restore_slot_invariant(task_id, "signal_frame_committed_restore_mask");
        self.restore = None;
        self.active_restore_slot = None;
        self.assert_restore_slot_invariant(task_id, "signal_frame_committed_restore_mask");
    }

    fn restore_temporary_if_pending(&mut self, task_id: Tid) {
        self.assert_restore_slot_invariant(task_id, "restore_temporary_if_pending");
        if let Some(old_mask) = self.restore.take() {
            self.active_restore_slot = None;
            Self::assert_valid_mask(old_mask);
            self.current = old_mask;
        }
        self.assert_restore_slot_invariant(task_id, "restore_temporary_if_pending");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TemporarySigMaskSlotId(u64);

impl TemporarySigMaskSlotId {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("temporary sigmask slot id overflow"),
        )
    }
}

/// Linear ownership token for a pending temporary signal-mask restore.
///
/// Dropping this token never restores the old mask and never clears the restore
/// slot. Callers must end it with exactly one terminal method.
#[must_use = "temporary signal mask tokens must be ended with restore_now() or defer_to_signal_delivery()"]
pub struct TemporarySigMaskToken {
    task: Arc<Task>,
    slot: TemporarySigMaskSlotId,
    active: bool,
}

impl TemporarySigMaskToken {
    fn new(task: Arc<Task>, slot: TemporarySigMaskSlotId) -> Self {
        Self {
            task,
            slot,
            active: true,
        }
    }

    fn assert_current_task(&self, context: &'static str) {
        let current = get_current_task();
        assert!(
            Arc::ptr_eq(&current, &self.task),
            "temporary signal mask token used on non-owner task: context={} owner={} current={} slot={:?}",
            context,
            self.task.tid(),
            current.tid(),
            self.slot,
        );
    }

    /// Restore the old mask immediately and clear the pending restore slot.
    pub fn restore_now(mut self) {
        self.assert_current_task("restore_now");
        self.task
            .sig_mask
            .lock()
            .restore_temporary_now(self.task.tid(), self.slot);
        self.active = false;
    }

    /// Leave restore responsibility with trap-return signal delivery.
    pub fn defer_to_signal_delivery(mut self) {
        self.assert_current_task("defer_to_signal_delivery");
        self.task
            .sig_mask
            .lock()
            .assert_defer_slot(self.task.tid(), self.slot);
        self.active = false;
    }
}

impl Drop for TemporarySigMaskToken {
    fn drop(&mut self) {
        if self.active {
            kwarningln!(
                "temporary signal mask token leaked without terminal method: task={} slot={:?}",
                self.task.tid(),
                self.slot,
            );
            assert!(
                !self.active,
                "temporary signal mask token leaked without terminal method"
            );
        }
    }
}

impl Task {
    /// Snapshot the current signal mask. Pending delayed-restore state is not
    /// exposed by this API.
    pub fn snapshot_current_sig_mask(&self) -> SigSet {
        self.sig_mask.lock().current()
    }

    /// Set the permanent current signal mask. Caller is responsible for
    /// ensuring the validity of `new_mask`, i.e. it should not have SIGKILL and
    /// SIGSTOP set.
    pub fn set_permanent_sig_mask(&self, new_mask: SigSet) {
        self.sig_mask
            .lock()
            .set_permanent_current(self.tid(), new_mask);
    }

    /// Mutate the current mask for ordinary current-mask operations and return
    /// the previous mask.
    pub fn mutate_current_sig_mask(&self, f: impl FnOnce(&mut SigSet)) -> SigSet {
        self.sig_mask
            .lock()
            .mutate_current(self.tid(), "mutate_current_sig_mask", f)
    }

    /// Restore the current mask from a committed signal frame context.
    ///
    /// This is intentionally distinct from delayed temporary-mask restore. It
    /// does not read, consume, or overwrite the pending restore slot.
    pub fn restore_sigframe_current_sig_mask(&self, new_mask: SigSet) {
        self.set_permanent_sig_mask(new_mask);
    }

    /// Temporarily mutate current signal mask inside a syscall body and return
    /// the previous mask. The caller must restore with
    /// [Task::restore_syscall_body_current_sig_mask].
    pub fn mutate_syscall_body_current_sig_mask(&self, f: impl FnOnce(&mut SigSet)) -> SigSet {
        self.mutate_current_sig_mask(f)
    }

    /// Restore a syscall-body-only temporary current mask.
    pub fn restore_syscall_body_current_sig_mask(&self, old_mask: SigSet) {
        self.set_permanent_sig_mask(old_mask);
    }

    /// Begin a delayed temporary signal mask window for the current task.
    ///
    /// This installs `new_mask` as current and records the old mask in the
    /// single restore slot before returning a linear token.
    pub fn begin_temporary_sig_mask(self: &Arc<Self>, new_mask: SigSet) -> TemporarySigMaskToken {
        let task_id = self.tid();
        let slot = self.sig_mask.lock().begin_temporary(task_id, new_mask);
        TemporarySigMaskToken::new(self.clone(), slot)
    }

    /// Mask value that should be encoded into a signal frame.
    ///
    /// During a pending temporary-mask window this returns the saved old mask;
    /// otherwise it returns the current mask.
    pub fn sigmask_to_save_for_signal_frame(&self) -> SigSet {
        self.sig_mask.lock().sigmask_to_save_for_signal_frame()
    }

    /// Consume the pending restore slot after a user signal frame has been
    /// committed and restore responsibility has moved to `rt_sigreturn()`.
    pub fn signal_frame_committed_restore_mask(&self) {
        self.sig_mask
            .lock()
            .signal_frame_committed_restore_mask(self.tid());
    }

    /// Restore a pending temporary mask before returning to user mode without a
    /// committed handler frame.
    pub fn restore_temporary_sig_mask_if_pending(&self) {
        self.sig_mask
            .lock()
            .restore_temporary_if_pending(self.tid());
    }

    /// Signal delivery may install handler masks while a temporary restore is
    /// pending. Ordinary mutation helpers intentionally reject that state.
    pub fn mutate_current_sig_mask_for_signal_delivery(
        &self,
        f: impl FnOnce(&mut SigSet),
    ) -> SigSet {
        self.sig_mask.lock().mutate_current_for_signal_delivery(f)
    }

    pub(super) fn is_current_sig_mask_blocking(&self, no: SigNo) -> bool {
        self.snapshot_current_sig_mask().get(no)
    }
}
