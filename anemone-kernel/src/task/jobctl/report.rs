//! Parent-visible job-control report and guards-out publication.

use crate::{
    prelude::*,
    task::{
        ThreadGroup,
        sig::{
            SigNo, Signal,
            disposition::SaFlags,
            info::{SiCode, SigChld, SigInfoFields},
        },
    },
};

use super::group::{JobControlPhase, JobControlTransition, UserJobControl};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum JobControlReport {
    Stopped,
    Continued,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::task) enum ChildJobControlStatus {
    Stopped(SigNo),
    Continued,
}

impl UserJobControl {
    /// Read or consume the current eligible report while the child
    /// `ThreadGroup.inner` guard and parent relation are both current.
    pub(in crate::task) fn select_report(
        &mut self,
        include_stopped: bool,
        include_continued: bool,
        consume: bool,
    ) -> Option<ChildJobControlStatus> {
        let status = match self.report? {
            JobControlReport::Stopped if include_stopped => {
                let JobControlPhase::Stopped(episode) = self.phase else {
                    return None;
                };
                ChildJobControlStatus::Stopped(episode.reason)
            },
            JobControlReport::Continued if include_continued => ChildJobControlStatus::Continued,
            JobControlReport::Stopped | JobControlReport::Continued => return None,
        };

        if consume {
            self.report = None;
            kdebugln!("jobctl: report {:?} consumed", status);
        }
        Some(status)
    }
}

impl ThreadGroup {
    pub(in crate::task) fn finish_job_control_transition(&self, transition: JobControlTransition) {
        if transition.wake_entry_gate {
            kdebugln!("jobctl: tgid={} publishing entry-gate rescan", self.tgid());
            self.jobctl_unblocked.publish(usize::MAX, true);
        }
        if let Some(status) = transition.parent_status {
            let parent = transition
                .parent
                .expect("jobctl: parent-visible transition lacks parent snapshot");
            kdebugln!(
                "jobctl: tgid={} publishing {:?} to parent_tgid={}",
                self.tgid(),
                status,
                parent.tgid(),
            );
            self.publish_job_control_status(&parent, status);
        }
    }

    pub(crate) fn is_job_control_stopped(&self) -> bool {
        let inner = self.inner.read();
        matches!(inner.status.life_cycle(), ThreadGroupLifeCycle::Alive)
            && inner
                .job_control
                .as_ref()
                .is_some_and(|job_control| matches!(job_control.phase, JobControlPhase::Stopped(_)))
    }

    fn publish_job_control_status(&self, parent: &Arc<ThreadGroup>, status: ChildJobControlStatus) {
        // The Event carries no report identity. Publishing it first lets an
        // already sleeping waiter observe the committed child-owned slot even
        // when the optional SIGCHLD occurrence is ignored or suppressed.
        parent.child_status_changed.publish(usize::MAX, false);

        let Some(disposition) = parent.signal_disposition() else {
            return;
        };
        let disposition = disposition.read().get_disposition(SigNo::SIGCHLD);
        if disposition.flags.contains(SaFlags::NOCLDSTOP) {
            return;
        }

        let (code, status_value) = match status {
            ChildJobControlStatus::Stopped(reason) => {
                (SiCode::ChldStopped, reason.as_usize() as i32)
            },
            ChildJobControlStatus::Continued => {
                (SiCode::ChldContinued, SigNo::SIGCONT.as_usize() as i32)
            },
        };
        // Standard SIGCHLD is an occurrence notification, not report truth.
        // Guards-out publications from successive job-control or terminal
        // transitions are not totally ordered; the durable child status and
        // Event predicate remain current.
        parent.recv_signal(Signal::new(
            SigNo::SIGCHLD,
            code,
            SigInfoFields::Chld(SigChld {
                pid: self.tgid(),
                // The accepted R0 boundary deliberately avoids a credential
                // cache whose lifetime would outlive the leader.
                uid: Uid::new(0),
                status: status_value,
                utime: 0,
                stime: 0,
            }),
        ));
    }
}
