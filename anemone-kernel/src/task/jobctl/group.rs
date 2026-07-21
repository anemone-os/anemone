//! ThreadGroup-owned job-control phase and exposure state.

use crate::{
    prelude::*,
    task::{ThreadGroup, sig::SigNo},
};

use super::report::{ChildJobControlStatus, JobControlReport};

/// Whether a live user member may currently be executing user instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UserExposure {
    Unexposed,
    Exposed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThreadGroupMember {
    User(UserExposure),
    KThread,
}

/// ThreadGroup membership is the only owner of user-exposure state.
///
/// The wrapper intentionally preserves the narrow set-like operations used by
/// topology readers while storing an owner-local value for each live member.
#[derive(Debug)]
pub(in crate::task) struct ThreadGroupMembers {
    inner: BTreeMap<Tid, ThreadGroupMember>,
}

impl ThreadGroupMembers {
    pub(in crate::task) fn new_user(tid: Tid) -> Self {
        Self {
            inner: BTreeMap::from([(tid, ThreadGroupMember::User(UserExposure::Unexposed))]),
        }
    }

    pub(in crate::task) fn new_kthread(tid: Tid) -> Self {
        Self {
            inner: BTreeMap::from([(tid, ThreadGroupMember::KThread)]),
        }
    }

    pub(in crate::task) fn len(&self) -> usize {
        self.inner.len()
    }

    pub(in crate::task) fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub(in crate::task) fn contains(&self, tid: &Tid) -> bool {
        self.inner.contains_key(tid)
    }

    pub(in crate::task) fn iter(&self) -> impl Iterator<Item = &Tid> {
        self.inner.keys()
    }

    pub(in crate::task) fn insert_user(&mut self, tid: Tid) -> bool {
        if self.inner.contains_key(&tid) {
            return false;
        }
        self.inner
            .insert(tid, ThreadGroupMember::User(UserExposure::Unexposed))
            .is_none()
    }

    pub(in crate::task) fn remove(&mut self, tid: &Tid) -> bool {
        match self.inner.get(tid) {
            Some(ThreadGroupMember::KThread) => {},
            Some(ThreadGroupMember::User(_)) => {
                panic!("jobctl: user member {} used kthread removal", tid)
            },
            None => return false,
        }
        self.inner.remove(tid);
        true
    }

    pub(in crate::task) fn remove_unexposed_user(&mut self, tid: &Tid) -> bool {
        match self.inner.get(tid) {
            Some(ThreadGroupMember::User(UserExposure::Unexposed)) => {},
            Some(ThreadGroupMember::User(UserExposure::Exposed)) => {
                panic!(
                    "jobctl: exposed user member {} detached before trap entry",
                    tid
                )
            },
            Some(ThreadGroupMember::KThread) => {
                panic!("jobctl: kthread member {} used user detach", tid)
            },
            None => return false,
        }
        self.inner.remove(tid);
        true
    }

    pub(in crate::task) fn rekey_unexposed_user(&mut self, old_tid: Tid, new_tid: Tid) -> bool {
        if self.inner.contains_key(&new_tid) {
            return false;
        }
        match self.inner.get(&old_tid) {
            Some(ThreadGroupMember::User(UserExposure::Unexposed)) => {},
            Some(ThreadGroupMember::User(UserExposure::Exposed)) => {
                panic!(
                    "jobctl: exposed user member {} rekeyed during dethread",
                    old_tid
                )
            },
            Some(ThreadGroupMember::KThread) => {
                panic!("jobctl: kthread member {} used user dethread", old_tid)
            },
            None => return false,
        }
        self.inner.remove(&old_tid);

        self.inner
            .insert(new_tid, ThreadGroupMember::User(UserExposure::Unexposed))
            .is_none()
    }

    pub(super) fn clear_user_exposure(&mut self, tid: Tid) {
        let member = self
            .inner
            .get_mut(&tid)
            .unwrap_or_else(|| panic!("jobctl: user member {} not found at trap entry", tid));
        match member {
            ThreadGroupMember::User(exposure) => {
                assert!(
                    *exposure == UserExposure::Exposed,
                    "jobctl: user member {} entered the kernel while unexposed",
                    tid
                );
                *exposure = UserExposure::Unexposed;
            },
            ThreadGroupMember::KThread => {
                panic!("jobctl: kthread member {} reached user trap entry", tid)
            },
        }
    }

    pub(super) fn expose_user(&mut self, tid: Tid) {
        let member = self
            .inner
            .get_mut(&tid)
            .unwrap_or_else(|| panic!("jobctl: user member {} not found at user entry", tid));
        match member {
            ThreadGroupMember::User(exposure) => {
                assert!(
                    *exposure == UserExposure::Unexposed,
                    "jobctl: user member {} passed the entry gate while already exposed",
                    tid
                );
                *exposure = UserExposure::Exposed;
            },
            ThreadGroupMember::KThread => {
                panic!("jobctl: kthread member {} reached user entry", tid)
            },
        }
    }

    pub(super) fn exposed_count(&self) -> usize {
        self.inner
            .values()
            .filter(|member| matches!(member, ThreadGroupMember::User(UserExposure::Exposed)))
            .count()
    }

    pub(in crate::task) fn all_user(&self) -> bool {
        self.inner
            .values()
            .all(|member| matches!(member, ThreadGroupMember::User(_)))
    }

    pub(in crate::task) fn all_kthread(&self) -> bool {
        self.inner
            .values()
            .all(|member| matches!(member, ThreadGroupMember::KThread))
    }
}

/// Ordering identity advanced by each admitted `SIGCONT` generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub(in crate::task) struct ContinueEpoch(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct StopEpisode {
    pub(super) reason: SigNo,
    /// Diagnostic timestamp only; it never participates in phase decisions.
    started_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum JobControlPhase {
    Running {
        /// Diagnostic timestamp only; it never participates in phase decisions.
        started_at: Instant,
    },
    Stopping(StopEpisode),
    Stopped(StopEpisode),
}

#[derive(Debug)]
pub(in crate::task) struct UserJobControl {
    pub(super) phase: JobControlPhase,
    continue_epoch: ContinueEpoch,
    /// Coalesced parent-visible report truth. A stopped reason is deliberately
    /// not copied here; it is derived from the live `Stopped` phase.
    pub(super) report: Option<JobControlReport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum JobControlPhaseSnapshot {
    Running,
    Stopping,
    Stopped,
}

/// Immutable diagnostic projection. None of these fields drive behavior.
#[derive(Debug, Clone, Copy)]
pub(super) struct JobControlDiagnostic {
    pub(super) phase: JobControlPhaseSnapshot,
    pub(super) first_reason: Option<SigNo>,
    pub(super) exposed_count: usize,
    pub(super) phase_age: Duration,
}

#[derive(Debug, Clone)]
pub(in crate::task) struct JobControlTransition {
    pub(super) wake_entry_gate: bool,
    pub(in crate::task) parent_status: Option<ChildJobControlStatus>,
    /// Immutable current-parent snapshot for this guards-out effect only. It
    /// does not identify the report and never drives child-owned state.
    pub(in crate::task) parent: Option<Arc<ThreadGroup>>,
}

impl JobControlTransition {
    pub(in crate::task) const NONE: Self = Self {
        wake_entry_gate: false,
        parent_status: None,
        parent: None,
    };
}

impl UserJobControl {
    pub(in crate::task) fn new_running() -> Self {
        Self {
            phase: JobControlPhase::Running {
                started_at: Instant::now(),
            },
            continue_epoch: ContinueEpoch(0),
            report: None,
        }
    }

    pub(in crate::task) fn is_running(&self) -> bool {
        matches!(self.phase, JobControlPhase::Running { .. })
    }

    pub(in crate::task) fn continue_epoch(&self) -> ContinueEpoch {
        self.continue_epoch
    }

    pub(in crate::task) fn request_unconditional_stop(
        &mut self,
        members: &ThreadGroupMembers,
        tgid: Tid,
        reason: SigNo,
    ) -> JobControlTransition {
        match self.phase {
            JobControlPhase::Running { .. } => {
                let episode = StopEpisode {
                    reason,
                    started_at: Instant::now(),
                };
                if members.exposed_count() == 0 {
                    self.phase = JobControlPhase::Stopped(episode);
                    self.report = Some(JobControlReport::Stopped);
                    kdebugln!(
                        "jobctl: tgid={} phase Running -> Stopped reason={:?} exposed=0",
                        tgid,
                        reason
                    );
                    JobControlTransition {
                        wake_entry_gate: false,
                        parent_status: Some(ChildJobControlStatus::Stopped(reason)),
                        parent: None,
                    }
                } else {
                    self.phase = JobControlPhase::Stopping(episode);
                    kdebugln!(
                        "jobctl: tgid={} phase Running -> Stopping reason={:?} exposed={}",
                        tgid,
                        reason,
                        members.exposed_count()
                    );
                    JobControlTransition::NONE
                }
            },
            JobControlPhase::Stopping(_) | JobControlPhase::Stopped(_) => {
                // Later stop requests merge into the first live episode and
                // never replace its externally visible reason.
                JobControlTransition::NONE
            },
        }
    }

    pub(in crate::task) fn request_conditional_stop(
        &mut self,
        members: &ThreadGroupMembers,
        tgid: Tid,
        reason: SigNo,
        expected_continue_epoch: ContinueEpoch,
    ) -> JobControlTransition {
        if tgid == Tid::INIT {
            // Global init consumes a live default-stop occurrence but can never
            // obtain job-control stop authority.
            return JobControlTransition::NONE;
        }
        if self.continue_epoch != expected_continue_epoch {
            kdebugln!(
                "jobctl: tgid={} rejected stale {:?} candidate epoch={:?} current={:?}",
                tgid,
                reason,
                expected_continue_epoch,
                self.continue_epoch,
            );
            return JobControlTransition::NONE;
        }

        self.request_unconditional_stop(members, tgid, reason)
    }

    pub(super) fn on_user_exposure_closed(
        &mut self,
        members: &ThreadGroupMembers,
        tgid: Tid,
    ) -> JobControlTransition {
        let JobControlPhase::Stopping(episode) = self.phase else {
            return JobControlTransition::NONE;
        };
        if members.exposed_count() != 0 {
            return JobControlTransition::NONE;
        }

        self.phase = JobControlPhase::Stopped(episode);
        self.report = Some(JobControlReport::Stopped);
        kdebugln!(
            "jobctl: tgid={} phase Stopping -> Stopped reason={:?} exposed=0",
            tgid,
            episode.reason
        );
        JobControlTransition {
            wake_entry_gate: false,
            parent_status: Some(ChildJobControlStatus::Stopped(episode.reason)),
            parent: None,
        }
    }

    pub(in crate::task) fn continue_generation(&mut self, tgid: Tid) -> JobControlTransition {
        self.continue_epoch.0 = self
            .continue_epoch
            .0
            .checked_add(1)
            .expect("jobctl: ContinueEpoch exhausted");

        match self.phase {
            JobControlPhase::Running { .. } => JobControlTransition::NONE,
            JobControlPhase::Stopping(episode) => {
                self.phase = JobControlPhase::Running {
                    started_at: Instant::now(),
                };
                kdebugln!(
                    "jobctl: tgid={} phase Stopping -> Running reason={:?} without report",
                    tgid,
                    episode.reason
                );
                JobControlTransition {
                    wake_entry_gate: true,
                    parent_status: None,
                    parent: None,
                }
            },
            JobControlPhase::Stopped(episode) => {
                self.phase = JobControlPhase::Running {
                    started_at: Instant::now(),
                };
                self.report = Some(JobControlReport::Continued);
                kdebugln!(
                    "jobctl: tgid={} phase Stopped -> Running reason={:?} with Continued report",
                    tgid,
                    episode.reason
                );
                JobControlTransition {
                    wake_entry_gate: true,
                    parent_status: Some(ChildJobControlStatus::Continued),
                    parent: None,
                }
            },
        }
    }

    /// Invalidate job-control authority before terminal lifecycle becomes
    /// externally visible. Terminal status remains owned by lifecycle code.
    pub(in crate::task) fn prepare_terminal(&mut self) -> JobControlTransition {
        let wake_entry_gate = !self.is_running();
        self.phase = JobControlPhase::Running {
            started_at: Instant::now(),
        };
        self.report = None;
        JobControlTransition {
            wake_entry_gate,
            parent_status: None,
            parent: None,
        }
    }

    pub(super) fn diagnostic(&self, members: &ThreadGroupMembers) -> JobControlDiagnostic {
        let (phase, first_reason, started_at) = match self.phase {
            JobControlPhase::Running { started_at } => {
                (JobControlPhaseSnapshot::Running, None, started_at)
            },
            JobControlPhase::Stopping(episode) => (
                JobControlPhaseSnapshot::Stopping,
                Some(episode.reason),
                episode.started_at,
            ),
            JobControlPhase::Stopped(episode) => (
                JobControlPhaseSnapshot::Stopped,
                Some(episode.reason),
                episode.started_at,
            ),
        };
        JobControlDiagnostic {
            phase,
            first_reason,
            exposed_count: members.exposed_count(),
            phase_age: episode_age(started_at),
        }
    }
}

fn episode_age(started_at: Instant) -> Duration {
    started_at.elapsed()
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn test_conditional_stop_rejects_stale_continue_epoch() {
        let tgid = Tid::new(2);
        let members = ThreadGroupMembers::new_user(tgid);
        let mut job_control = UserJobControl::new_running();
        let stale_epoch = job_control.continue_epoch();

        assert!(
            job_control
                .continue_generation(tgid)
                .parent_status
                .is_none()
        );
        let rejected =
            job_control.request_conditional_stop(&members, tgid, SigNo::SIGTSTP, stale_epoch);
        assert!(rejected.parent_status.is_none());
        assert!(job_control.is_running());
        assert!(job_control.report.is_none());

        let current_epoch = job_control.continue_epoch();
        let accepted =
            job_control.request_conditional_stop(&members, tgid, SigNo::SIGTSTP, current_epoch);
        assert_eq!(
            accepted.parent_status,
            Some(ChildJobControlStatus::Stopped(SigNo::SIGTSTP))
        );
        assert!(matches!(job_control.phase, JobControlPhase::Stopped(_)));
    }
}
