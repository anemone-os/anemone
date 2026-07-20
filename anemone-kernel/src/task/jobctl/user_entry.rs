//! Mandatory user-entry arbitration for dormant job control.

use crate::{prelude::*, task::cpu_usage::Privilege};

impl ThreadGroup {
    fn clear_user_exposure(&self, tid: Tid) {
        let mut inner = self.inner.write();
        assert!(
            inner.job_control.is_some(),
            "jobctl: user trap entry reached non-user ThreadGroup {}",
            self.tgid()
        );
        inner.members.clear_user_exposure(tid);
    }

    fn try_admit_user_entry(&self, tid: Tid) -> bool {
        let mut inner = self.inner.write();
        let running = inner
            .job_control
            .as_ref()
            .unwrap_or_else(|| {
                panic!(
                    "jobctl: user entry reached non-user ThreadGroup {}",
                    self.tgid()
                )
            })
            .is_running();
        if running {
            inner.members.expose_user(tid);
        }
        running
    }

    fn before_user_entry(&self, tid: Tid) {
        if self.try_admit_user_entry(tid) {
            return;
        }

        // Event only publishes a rescan opportunity. The predicate reacquires
        // the ThreadGroup owner and atomically registers exposure with the live
        // Running phase, closing both publication-before-wait and stale-permit
        // races without carrying a token across the park.
        self.jobctl_unblocked
            .listen_uninterruptible(false, || self.try_admit_user_entry(tid));
    }
}

impl Task {
    /// Record that a user task entered the kernel before processing the trap.
    #[track_caller]
    pub(crate) fn on_user_trap_entry(&self) {
        assert!(
            IntrArch::local_intr_disabled(),
            "jobctl: user trap entry must run with interrupts disabled"
        );
        self.get_thread_group().clear_user_exposure(self.tid());
        self.on_prv_change(Privilege::Kernel);
    }

    /// Perform the final ThreadGroup gate before executing user instructions.
    #[track_caller]
    pub(crate) fn before_user_entry(&self) {
        assert!(
            IntrArch::local_intr_disabled(),
            "jobctl: final user-entry gate must run with interrupts disabled"
        );
        self.get_thread_group().before_user_entry(self.tid());
    }
}
