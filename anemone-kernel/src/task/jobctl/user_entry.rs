//! Mandatory user-entry arbitration for dormant job control.

use crate::{
    prelude::*,
    task::{
        ThreadGroupInner,
        cpu_usage::Privilege,
        sig::{SigNo, set::SigSet},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::task) enum UserEntryOutcome {
    Admitted,
    Recheck,
    Park,
    Exit(ExitCode),
}

impl ThreadGroup {
    fn clear_user_exposure(&self, tid: Tid) {
        let needs_stop_completion = {
            let mut inner = self.inner.write();
            let life_cycle = inner.status.life_cycle();
            let ThreadGroupInner {
                members,
                job_control,
                ..
            } = &mut *inner;
            match life_cycle {
                ThreadGroupLifeCycle::Alive => members.clear_user_exposure(tid),
                ThreadGroupLifeCycle::Exiting(_) => members.assert_user_unexposed(tid),
                ThreadGroupLifeCycle::Exited(code) => {
                    panic!(
                        "jobctl: task {} trapped after ThreadGroup {} exited with {:?}",
                        tid,
                        self.tgid(),
                        code
                    )
                },
            }
            let job_control = job_control.as_ref().unwrap_or_else(|| {
                panic!(
                    "jobctl: user trap entry reached non-user ThreadGroup {}",
                    self.tgid()
                )
            });
            let stopping = matches!(
                job_control.phase,
                super::group::JobControlPhase::Stopping(_)
            );
            if stopping && members.exposed_count() != 0 {
                job_control.log_stopping_progress(members, self.tgid());
            }
            matches!(life_cycle, ThreadGroupLifeCycle::Alive)
                && stopping
                && members.exposed_count() == 0
        };
        if !needs_stop_completion {
            return;
        }

        // Only the last exposure closure enters topology. Recheck the live
        // phase and exposure set there so a concurrent SIGCONT or terminal
        // transition fails closed before any Stopped report is committed.
        let Some(((), transition)) = self.with_child_status_transaction(|_, inner| {
            let ThreadGroupInner {
                members,
                job_control,
                ..
            } = inner;
            let transition = job_control
                .as_mut()
                .expect("jobctl: user ThreadGroup lacks control state")
                .on_user_exposure_closed(members, self.tgid());
            ((), transition)
        }) else {
            return;
        };
        self.finish_job_control_transition(transition);
    }

    fn try_admit_user_entry(&self, task: &Task) -> UserEntryOutcome {
        let mut inner = self.inner.write();
        match inner.status.life_cycle() {
            ThreadGroupLifeCycle::Alive => {},
            ThreadGroupLifeCycle::Exiting(code) => return UserEntryOutcome::Exit(code),
            ThreadGroupLifeCycle::Exited(code) => {
                panic!(
                    "jobctl: task {} reached user entry after ThreadGroup {} exited with {:?}",
                    task.tid(),
                    self.tgid(),
                    code
                )
            },
        }

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
        if !running {
            return UserEntryOutcome::Park;
        }

        inner.members.expose_user(task.tid());
        UserEntryOutcome::Admitted
    }

    fn should_leave_entry_park(&self, task: &Task) -> bool {
        let phase_or_lifecycle_changed = {
            let inner = self.inner.read();
            !matches!(inner.status.life_cycle(), ThreadGroupLifeCycle::Alive)
                || inner
                    .job_control
                    .as_ref()
                    .expect("jobctl: user ThreadGroup lacks control state")
                    .is_running()
        };
        if phase_or_lifecycle_changed {
            return true;
        }

        // SIGKILL is the only force wake that must leave a stopped-phase park
        // before the phase changes. Ordinary asynchronous signals remain
        // pending and cannot release the mandatory gate.
        task.has_specific_signal(SigSet::new_with_signos(&[SigNo::SIGKILL]))
    }

    fn before_user_entry(&self, task: &Task) -> UserEntryOutcome {
        let decision = self.try_admit_user_entry(task);
        if decision != UserEntryOutcome::Park {
            return decision;
        }

        kdebugln!(
            "jobctl: tgid={} tid={} parked at user-entry gate",
            self.tgid(),
            task.tid(),
        );

        // Event and force wake only request a fresh outer arbitration pass;
        // neither carries phase truth or a user-entry permit.
        self.jobctl_unblocked
            .listen_uninterruptible(false, || self.should_leave_entry_park(task));
        kdebugln!(
            "jobctl: tgid={} tid={} left user-entry park for re-arbitration",
            self.tgid(),
            task.tid(),
        );
        UserEntryOutcome::Recheck
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
    pub(in crate::task) fn before_user_entry(&self) -> UserEntryOutcome {
        assert!(
            IntrArch::local_intr_disabled(),
            "jobctl: final user-entry gate must run with interrupts disabled"
        );
        let outcome = self.get_thread_group().before_user_entry(self);
        assert!(
            outcome != UserEntryOutcome::Park,
            "jobctl park must resolve before returning"
        );
        outcome
    }
}
