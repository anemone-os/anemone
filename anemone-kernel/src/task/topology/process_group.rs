//! Process-group and session topology operations.

use crate::{
    prelude::*,
    task::{
        ProcessGroupInner, SessionInner, ThreadGroupInner, sig::Signal, topology::TOPOLOGY,
    },
};

impl Session {
    pub fn sid(&self) -> Tid {
        self.sid
    }
}

impl ProcessGroup {
    pub fn pgid(&self) -> Tid {
        self.pgid
    }

    pub fn sid(&self) -> Tid {
        self.sid
    }

    pub fn nthread_groups(&self) -> usize {
        self.inner.read().members.len()
    }

    /// Collect thread groups currently in this process group.
    ///
    /// This is intentionally a snapshot: callers like signal broadcast should
    /// not run arbitrary signal handling work while holding topology and group
    /// locks.
    ///
    /// # Locks
    ///
    /// [TOPOLOGY] -> [ProcessGroup]
    pub fn get_members(&self) -> Vec<Arc<ThreadGroup>> {
        let topology = TOPOLOGY.inner.read();
        self.inner
            .read()
            .members
            .iter()
            .map(|tgid| {
                topology
                    .thread_groups
                    .get(tgid)
                    .expect("task topology: thread group not found in process group")
                    .clone()
            })
            .collect()
    }

    /// Send a signal to every member thread group.
    ///
    /// Process groups are only membership selectors; delivery stays on
    /// [ThreadGroup::recv_signal].
    pub fn recv_signal(&self, signal: Signal) {
        for tg in self.get_members() {
            tg.recv_signal(signal.clone());
        }
    }
}

impl ThreadGroup {
    pub fn pgid(&self) -> Tid {
        self.inner.read().pgid
    }

    pub fn sid(&self) -> Tid {
        self.inner.read().sid
    }

    pub fn has_executed(&self) -> bool {
        self.inner.read().status.has_executed()
    }

    pub fn mark_executed(&self) {
        self.inner.write().status.has_executed = true;
    }

    /// Move this thread group into a process group.
    ///
    /// This only performs the topology transaction. Policy like "is the target
    /// a child of the caller" or "has the child execed" belongs to the caller.
    ///
    /// # Locks
    ///
    /// [TOPOLOGY] -> [Session] -> [ProcessGroup] -> [ThreadGroup]
    pub fn move_to_process_group(self: &Arc<Self>, new_pgid: Tid) -> Result<(), SysError> {
        let target_tgid = self.tgid();
        let mut topology = TOPOLOGY.inner.write();
        if !topology.thread_groups.contains_key(&target_tgid) {
            return Err(SysError::NoSuchProcess);
        }

        let target_snapshot = self.inner.read();
        let old_pgid = target_snapshot.pgid;
        let target_sid = target_snapshot.sid;
        drop(target_snapshot);

        if old_pgid == new_pgid {
            return Ok(());
        }

        let session = topology
            .sessions
            .get(&target_sid)
            .expect("task topology: target session not found")
            .clone();
        let old_pg = topology
            .process_groups
            .get(&old_pgid)
            .expect("task topology: old process group not found")
            .clone();

        let (new_pg, creating_new_pg) =
            if let Some(existing) = topology.process_groups.get(&new_pgid) {
                if existing.sid != target_sid {
                    return Err(SysError::PermissionDenied);
                }
                (existing.clone(), false)
            } else if new_pgid == target_tgid {
                (
                    Arc::new(ProcessGroup {
                        pgid: new_pgid,
                        sid: target_sid,
                        inner: NoIrqRwLock::new(ProcessGroupInner {
                            members: BTreeSet::new(),
                        }),
                    }),
                    true,
                )
            } else {
                return Err(SysError::PermissionDenied);
            };

        let mut session_inner = session.inner.write();

        let old_pg_empty = if old_pgid < new_pgid {
            let mut old_pg_inner = old_pg.inner.write();
            let mut new_pg_inner = new_pg.inner.write();
            let mut target_inner = self.inner.write();

            move_thread_group_between_process_groups(
                target_tgid,
                new_pgid,
                target_sid,
                &mut old_pg_inner,
                &mut new_pg_inner,
                &mut target_inner,
            )
        } else {
            let mut new_pg_inner = new_pg.inner.write();
            let mut old_pg_inner = old_pg.inner.write();
            let mut target_inner = self.inner.write();

            move_thread_group_between_process_groups(
                target_tgid,
                new_pgid,
                target_sid,
                &mut old_pg_inner,
                &mut new_pg_inner,
                &mut target_inner,
            )
        };

        if creating_new_pg {
            assert!(
                session_inner.process_groups.insert(new_pgid),
                "task topology: duplicate process group {} in session {}",
                new_pgid,
                target_sid
            );
            assert!(
                topology.process_groups.insert(new_pgid, new_pg).is_none(),
                "task topology: duplicate process group {}",
                new_pgid
            );
        }

        if old_pg_empty {
            assert!(
                session_inner.process_groups.remove(&old_pgid),
                "task topology: old process group {} not found in session {}",
                old_pgid,
                target_sid
            );
            assert!(
                topology.process_groups.remove(&old_pgid).is_some(),
                "task topology: old process group {} not found",
                old_pgid
            );
            if session_inner.process_groups.is_empty() {
                assert!(
                    topology.sessions.remove(&target_sid).is_some(),
                    "task topology: empty session {} not found",
                    target_sid
                );
            }
        }

        Ok(())
    }

    /// Create a new session and process group containing only this thread group.
    ///
    /// # Locks
    ///
    /// [TOPOLOGY] -> [Session] -> [ProcessGroup] -> [ThreadGroup]
    pub fn create_session(self: &Arc<Self>) -> Result<Tid, SysError> {
        let target_tgid = self.tgid();
        let mut topology = TOPOLOGY.inner.write();
        if !topology.thread_groups.contains_key(&target_tgid) {
            return Err(SysError::NoSuchProcess);
        }

        let target_snapshot = self.inner.read();
        let old_pgid = target_snapshot.pgid;
        let old_sid = target_snapshot.sid;
        drop(target_snapshot);

        if old_pgid == target_tgid || topology.process_groups.contains_key(&target_tgid) {
            return Err(SysError::PermissionDenied);
        }
        if topology.sessions.contains_key(&target_tgid) {
            return Err(SysError::PermissionDenied);
        }

        let old_session = topology
            .sessions
            .get(&old_sid)
            .expect("task topology: old session not found")
            .clone();
        let old_pg = topology
            .process_groups
            .get(&old_pgid)
            .expect("task topology: old process group not found")
            .clone();
        let new_session = Arc::new(Session {
            sid: target_tgid,
            inner: NoIrqRwLock::new(SessionInner {
                process_groups: BTreeSet::from([target_tgid]),
            }),
        });
        let new_pg = Arc::new(ProcessGroup {
            pgid: target_tgid,
            sid: target_tgid,
            inner: NoIrqRwLock::new(ProcessGroupInner {
                members: BTreeSet::from([target_tgid]),
            }),
        });

        let mut old_session_inner = old_session.inner.write();
        let mut old_pg_inner = old_pg.inner.write();
        let mut target_inner = self.inner.write();

        assert!(
            old_pg_inner.members.remove(&target_tgid),
            "task topology: target {} not found in old process group {}",
            target_tgid,
            old_pgid
        );
        target_inner.pgid = target_tgid;
        target_inner.sid = target_tgid;
        let old_pg_empty = old_pg_inner.members.is_empty();

        drop(target_inner);
        drop(old_pg_inner);

        if old_pg_empty {
            assert!(
                old_session_inner.process_groups.remove(&old_pgid),
                "task topology: old process group {} not found in old session {}",
                old_pgid,
                old_sid
            );
        } else {
            assert!(
                old_session_inner.process_groups.contains(&old_pgid),
                "task topology: old process group {} not found in old session {}",
                old_pgid,
                old_sid
            );
        }

        assert!(
            topology.sessions.insert(target_tgid, new_session).is_none(),
            "task topology: duplicate session {}",
            target_tgid
        );
        assert!(
            topology.process_groups.insert(target_tgid, new_pg).is_none(),
            "task topology: duplicate process group {}",
            target_tgid
        );

        if old_pg_empty {
            assert!(
                topology.process_groups.remove(&old_pgid).is_some(),
                "task topology: old process group {} not found",
                old_pgid
            );
            if old_session_inner.process_groups.is_empty() {
                assert!(
                    topology.sessions.remove(&old_sid).is_some(),
                    "task topology: empty session {} not found",
                    old_sid
                );
            }
        }

        Ok(target_tgid)
    }
}

/// Get a process group by its PGID.
///
/// # Locks
///
/// [TOPOLOGY]
pub fn get_process_group(pgid: &Tid) -> Option<Arc<ProcessGroup>> {
    let topology = TOPOLOGY.inner.read();
    topology.process_groups.get(pgid).cloned()
}

/// Get a session by its SID.
///
/// # Locks
///
/// [TOPOLOGY]
pub fn get_session(sid: &Tid) -> Option<Arc<Session>> {
    let topology = TOPOLOGY.inner.read();
    topology.sessions.get(sid).cloned()
}

fn move_thread_group_between_process_groups(
    tgid: Tid,
    new_pgid: Tid,
    new_sid: Tid,
    old_pg_inner: &mut ProcessGroupInner,
    new_pg_inner: &mut ProcessGroupInner,
    target_inner: &mut ThreadGroupInner,
) -> bool {
    assert!(
        old_pg_inner.members.remove(&tgid),
        "task topology: target {} not found in old process group {}",
        tgid,
        target_inner.pgid
    );
    assert!(
        new_pg_inner.members.insert(tgid),
        "task topology: duplicate target {} in process group {}",
        tgid,
        new_pgid
    );
    target_inner.pgid = new_pgid;
    target_inner.sid = new_sid;
    old_pg_inner.members.is_empty()
}
