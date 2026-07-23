//! Narrow task-topology capabilities for the TTY relation owner.

use crate::{
    prelude::*,
    task::sig::{
        SigNo, Signal, TtySigttouDisposition,
        info::{SiCode, SigInfoFields, SigKill},
    },
};

#[derive(Clone)]
pub(crate) struct TtySession {
    session: Arc<Session>,
    leader: Arc<ThreadGroup>,
}

#[derive(Clone)]
pub(crate) struct TtyProcessGroup {
    group: Arc<ProcessGroup>,
}

pub(crate) struct TtyCaller {
    task: Arc<Task>,
    thread_group: Arc<ThreadGroup>,
    session: TtySession,
    process_group: TtyProcessGroup,
}

/// Opaque lifecycle handoff. It identifies the stable session and its leader;
/// it carries no relation truth and cannot mutate task topology.
pub(crate) struct TtySessionLeader(TtySession);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TtySigttouDecision {
    Continue,
    Signal,
}

impl TtySession {
    pub(crate) fn sid(&self) -> Tid {
        self.session.sid()
    }

    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.session, &other.session) && Arc::ptr_eq(&self.leader, &other.leader)
    }

    /// Re-resolve both stable objects so reused numeric IDs cannot revive a
    /// relation whose original session leader has started exiting.
    pub(crate) fn is_live(&self) -> bool {
        get_session(&self.sid()).is_some_and(|session| Arc::ptr_eq(&session, &self.session))
            && get_thread_group(&self.sid())
                .is_some_and(|leader| Arc::ptr_eq(&leader, &self.leader))
            && matches!(
                self.leader.status().life_cycle(),
                ThreadGroupLifeCycle::Alive
            )
    }
}

impl TtyProcessGroup {
    pub(crate) fn pgid(&self) -> Tid {
        self.group.pgid()
    }

    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.group, &other.group)
    }

    pub(crate) fn is_live_in(&self, session: &TtySession) -> bool {
        self.group.sid() == session.sid()
            && get_process_group(&self.pgid()).is_some_and(|group| Arc::ptr_eq(&group, &self.group))
    }
}

impl TtyCaller {
    pub(crate) fn current() -> Result<Self, SysError> {
        let task = get_current_task();
        let thread_group = task.get_thread_group();
        if thread_group.ty() != ThreadGroupType::User
            || !matches!(
                thread_group.status().life_cycle(),
                ThreadGroupLifeCycle::Alive
            )
        {
            return Err(SysError::NoSuchProcess);
        }
        let sid = thread_group.sid();
        let pgid = thread_group.pgid();
        let session = get_session(&sid).ok_or(SysError::NoSuchProcess)?;
        let leader = get_thread_group(&sid).ok_or(SysError::NoSuchProcess)?;
        let group = get_process_group(&pgid).ok_or(SysError::NoSuchProcess)?;
        let caller = Self {
            task,
            thread_group,
            session: TtySession { session, leader },
            process_group: TtyProcessGroup { group },
        };
        if caller.revalidate() {
            Ok(caller)
        } else {
            Err(SysError::NoSuchProcess)
        }
    }

    pub(crate) fn session(&self) -> &TtySession {
        &self.session
    }

    pub(crate) fn process_group(&self) -> &TtyProcessGroup {
        &self.process_group
    }

    pub(crate) fn is_session_leader(&self) -> bool {
        self.thread_group.tgid() == self.session.sid()
            && Arc::ptr_eq(&self.thread_group, &self.session.leader)
    }

    pub(crate) fn revalidate(&self) -> bool {
        self.session.is_live()
            && get_thread_group(&self.thread_group.tgid())
                .is_some_and(|group| Arc::ptr_eq(&group, &self.thread_group))
            && self.thread_group.sid() == self.session.sid()
            && self.process_group.is_live_in(&self.session)
            && self.thread_group.pgid() == self.process_group.pgid()
    }

    pub(crate) fn resolve_process_group(&self, pgid: Tid) -> Result<TtyProcessGroup, SysError> {
        let group = get_process_group(&pgid).ok_or(SysError::NoSuchProcess)?;
        if group.sid() != self.session.sid() {
            return Err(SysError::PermissionDenied);
        }
        let result = TtyProcessGroup { group };
        if result.is_live_in(&self.session) {
            Ok(result)
        } else {
            Err(SysError::NoSuchProcess)
        }
    }

    pub(crate) fn sigttou_decision(
        &self,
        foreground: Option<&TtyProcessGroup>,
    ) -> TtySigttouDecision {
        if foreground.is_none_or(|foreground| foreground.same_identity(&self.process_group)) {
            return TtySigttouDecision::Continue;
        }
        match self.task.tty_sigttou_disposition() {
            TtySigttouDisposition::BlockedOrIgnored => TtySigttouDecision::Continue,
            TtySigttouDisposition::Actionable => TtySigttouDecision::Signal,
        }
    }

    pub(crate) fn signal_process_group_sigttou(&self) -> bool {
        if !self.revalidate() {
            return false;
        }
        self.process_group.group.recv_signal(Signal::new(
            SigNo::SIGTTOU,
            SiCode::Kernel,
            SigInfoFields::Kill(SigKill {
                pid: self.thread_group.tgid(),
                uid: self.task.cred().uid.real,
            }),
        ));
        true
    }
}

impl TtySessionLeader {
    pub(crate) fn from_thread_group(group: &Arc<ThreadGroup>) -> Option<Self> {
        if group.ty() != ThreadGroupType::User || group.tgid() != group.sid() {
            return None;
        }
        let session = get_session(&group.sid())?;
        let leader = get_thread_group(&group.sid())?;
        if !Arc::ptr_eq(group, &leader) {
            return None;
        }
        Some(Self(TtySession { session, leader }))
    }

    pub(crate) fn session(&self) -> &TtySession {
        &self.0
    }
}
