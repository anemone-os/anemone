//! ThreadGroup-owned dormant job-control state.

use crate::{prelude::*, task::sig::SigNo};

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

/// Ordering identity advanced by each future admitted `SIGCONT` generation.
/// Stage 1 constructs the identity but has no production control-signal
/// ingress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub(super) struct ContinueEpoch(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StopEpisode {
    reason: SigNo,
    /// Diagnostic timestamp only; it never participates in phase decisions.
    started_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JobControlPhase {
    Running {
        /// Diagnostic timestamp only; it never participates in phase decisions.
        started_at: Instant,
    },
    #[allow(dead_code)]
    Stopping(StopEpisode),
    #[allow(dead_code)]
    Stopped(StopEpisode),
}

#[derive(Debug)]
pub(in crate::task) struct UserJobControl {
    phase: JobControlPhase,
    #[allow(dead_code)]
    continue_epoch: ContinueEpoch,
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

impl UserJobControl {
    pub(in crate::task) fn new_running() -> Self {
        Self {
            phase: JobControlPhase::Running {
                started_at: Instant::now(),
            },
            continue_epoch: ContinueEpoch(0),
        }
    }

    pub(super) fn is_running(&self) -> bool {
        matches!(self.phase, JobControlPhase::Running { .. })
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
