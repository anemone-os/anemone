//! Intra-thread group operations.

use crate::{
    prelude::*,
    task::{
        ThreadGroupType,
        sig::{
            SigNo, Signal,
            info::{SiCode, SigInfoFields, SigKill},
        },
        topology::{TOPOLOGY, TaskNode, ThreadGroup},
    },
};

impl ThreadGroup {
    /// Get the thread group ID.
    pub fn tgid(&self) -> Tid {
        self.tgid.get_typed()
    }

    pub fn ty(&self) -> ThreadGroupType {
        self.ty
    }

    /// How many members are in this thread group.
    pub fn ntasks(&self) -> usize {
        self.inner.read().members.len()
    }

    /// Get the status of this thread group.
    pub fn status(&self) -> ThreadGroupStatus {
        self.inner.read().status
    }

    pub fn update_life_cycle_with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&ThreadGroupLifeCycle) -> (ThreadGroupLifeCycle, R),
    {
        let mut inner = self.inner.write();
        let (new_life_cycle, ret) = f(&inner.status.life_cycle);
        inner.status.life_cycle = new_life_cycle;
        ret
    }

    /// Try to get the exit code of this thread group. Returns [None] if this
    /// thread group has not exited yet.
    ///
    /// Note: If the status is currently [ThreadGroupLifeCycle::Exiting], this
    /// method returns [None] as well, since the thread group has not finished
    /// exiting.
    pub fn exit_code(&self) -> Option<ExitCode> {
        if let ThreadGroupLifeCycle::Exited(code) = self.status().life_cycle() {
            Some(code)
        } else {
            None
        }
    }

    /// Get the leader task of this thread group.
    ///
    /// # Locks
    ///
    /// [TOPOLOGY]
    pub fn leader(&self) -> Option<Arc<Task>> {
        let topology = TOPOLOGY.inner.read();
        topology
            .tasks
            .get(&self.tgid())
            .map(|node| node.task.clone())
    }

    /// Iterate over all members of this thread group while topology membership
    /// stays stable.
    ///
    /// **Topology Consistency** is guaranteed.
    ///
    /// The closure must be short, non-blocking, and must not perform real work
    /// such as signal delivery, wait-core wakeup, event publish, scheduling, or
    /// any operation that can reacquire topology/thread-group locks. Use
    /// [ThreadGroup::get_members] first when the caller needs to run work on
    /// each member.
    ///
    /// ## Locks
    ///
    /// [TOPOLOGY] -> [ThreadGroup]
    pub fn for_each_member<F: FnMut(&Arc<Task>)>(&self, mut f: F) {
        let topology = TOPOLOGY.inner.read();
        for member_tid in self.inner.read().members.iter() {
            let member = &topology
                .tasks
                .get(member_tid)
                .expect("task topology: task not found")
                .task;
            f(member);
        }
    }

    /// Collect all members of this thread group into a vector.
    ///
    /// Only **Object Consistency** is guaranteed.
    ///
    /// ## Locks
    /// [TOPOLOGY] -> [ThreadGroup]
    ///
    /// This intentionally does not re-resolve `self` from the global topology.
    /// Callers may hold a stale `Arc<ThreadGroup>` after lookup/reap races; in
    /// that case an already-empty group simply snapshots to an empty member
    /// list. Signal delivery, wakeups, and other real work must run after this
    /// short lock window returns.
    pub fn get_members(&self) -> Vec<Arc<Task>> {
        let topology = TOPOLOGY.inner.read();
        self.inner
            .read()
            .members
            .iter()
            .map(|member_tid| {
                topology
                    .tasks
                    .get(member_tid)
                    .expect("task topology: task not found")
                    .task
                    .clone()
            })
            .collect()
    }

    /// Short-circuit find a member of this task's thread group that satisfies
    /// the given predicate.
    ///
    /// Only **Object Consistency** is guaranteed.
    pub fn find_member<P: FnMut(&Arc<Task>) -> bool>(&self, prediction: P) -> Option<Arc<Task>> {
        self.get_members().into_iter().find(prediction)
    }

    /// Get the [SigNo] that should be sent to parent thread group when this
    /// thread group exits.
    pub fn terminate_signal(&self) -> Option<SigNo> {
        self.terminate_signal
    }
}

impl Drop for ThreadGroup {
    fn drop(&mut self) {
        // TODO: should we support deferred cleanup of thread groups?
        kdebugln!("thread group {} is dropped", self.tgid.get_typed());
    }
}

impl Task {
    /// Get the thread group ID of this task.
    pub fn tgid(&self) -> Tid {
        self.tgid
    }

    /// Whether this task is the leader of its thread group.
    pub fn is_tg_leader(&self) -> bool {
        self.tid() == self.tgid()
    }

    /// Get the thread group of this task.
    ///
    /// # Panics
    ///
    /// Panics if the thread group of this task is not found, which should not
    /// happen if this [Task] is still alive.
    ///
    /// ## Locks
    ///
    /// [TOPOLOGY]
    pub fn get_thread_group(&self) -> Arc<ThreadGroup> {
        let topology = TOPOLOGY.inner.read();
        let tg = topology
            .thread_groups
            .get(&self.tgid())
            .expect("task topology: thread group not found");
        tg.clone()
    }

    /// Run a closure with this task's thread group.
    ///
    /// Internally this function locks the topology, so we can make sure that
    /// the thread group will stay stable during the execution of the closure,
    /// which is what we want.
    ///
    /// This method provides **Topology Consistency** at the cost of locking the
    /// topology for a longer time. Caution should be taken to avoid deadlocks.
    ///
    /// # Panics
    ///
    /// Panics if the thread group of this task is not found, which should not
    /// happen if this [Task] is still alive.
    ///
    /// ## Locks
    ///
    /// [TOPOLOGY]
    pub fn with_thread_group<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Arc<ThreadGroup>) -> R,
    {
        let topology = TOPOLOGY.inner.read();
        let tg = topology
            .thread_groups
            .get(&self.tgid())
            .expect("task topology: thread group not found");
        f(&tg)
    }

    /// Whether this task is the only topology-visible task using `uspace`.
    ///
    /// This is used by address-space scoped cleanup paths. It deliberately
    /// counts tasks, not transient `Arc` holders, so syscall-local references
    /// do not postpone process cleanup.
    pub fn is_last_user_of_uspace(&self, uspace: &Arc<UserSpaceHandle>) -> bool {
        let topology = TOPOLOGY.inner.read();
        !topology.tasks.values().any(|node| {
            if node.task.tid() == self.tid() {
                return false;
            }

            node.task
                .try_clone_uspace_handle()
                .map(|other| Arc::ptr_eq(&other, uspace))
                .unwrap_or(false)
        })
    }

    /// Detach this task from the topology, which includes:
    /// - removing this task from its thread group members.
    /// - removing this task from the topology registry.
    ///
    /// Returns true if this task is the last member of its thread group, who
    /// should handle the cleanup work of the thread group.
    ///
    /// Called when this task is exiting.
    ///
    /// After this operation, the task is unreachable from the topology (i.e.
    /// defunct), and should be disposed soon.
    pub fn detach_from_topology(&self) -> bool {
        let mut topology = TOPOLOGY.inner.write();

        let tg = topology
            .thread_groups
            .get(&self.tgid())
            .expect("task topology: thread group not found")
            .clone();
        assert!(
            tg.ty() == ThreadGroupType::User,
            "task topology: kthread {} requires dedicated kthread unpublish",
            self.tgid()
        );

        // The topology write lock keeps the process-group/session links stable
        // while we collect the lock keys, then we take the canonical
        // Session -> ProcessGroup -> ThreadGroup chain for the mutation.
        let tg_snapshot = tg.inner.read();
        let pgid = tg_snapshot
            .pgid
            .expect("task topology: user thread group missing process group");
        let sid = tg_snapshot
            .sid
            .expect("task topology: user thread group missing session");
        drop(tg_snapshot);

        let session = topology
            .sessions
            .get(&sid)
            .expect("task topology: session not found when detaching task")
            .clone();
        let pg = topology
            .process_groups
            .get(&pgid)
            .expect("task topology: process group not found when detaching task")
            .clone();

        let mut session_inner = session.inner.write();
        let mut pg_inner = pg.inner.write();
        let mut tg_inner = tg.inner.write();
        assert!(tg_inner.members.remove(&self.tid()));
        let is_last = tg_inner.members.is_empty();
        let pg_empty = if is_last {
            assert!(
                pg_inner.members.remove(&self.tgid()),
                "task topology: thread group {} not found in process group {}",
                self.tgid(),
                pgid
            );
            pg_inner.members.is_empty()
        } else {
            false
        };

        drop(tg_inner);
        drop(pg_inner);

        if pg_empty {
            assert!(
                session_inner.process_groups.remove(&pgid),
                "task topology: process group {} not found in session {}",
                pgid,
                sid
            );
            assert!(
                topology.process_groups.remove(&pgid).is_some(),
                "task topology: process group {} not found",
                pgid
            );
            if session_inner.process_groups.is_empty() {
                assert!(
                    topology.sessions.remove(&sid).is_some(),
                    "task topology: session {} not found",
                    sid
                );
            }
        }

        drop(session_inner);
        assert!(topology.tasks.remove(&self.tid()).is_some());

        is_last
    }

    /// Kill all other members in this task's thread group, who, if not the
    /// original leader, will become the new leader of the thread group.
    ///
    /// The thread group itself stays alive.
    pub fn dethread(self: &Arc<Self>) {
        let (tg, members_to_kill) = {
            let topology = TOPOLOGY.inner.read();

            // we can't call above encapsulated APIs here since that will cause deadlocks.

            let tg = topology
                .thread_groups
                .get(&self.tgid())
                .expect("task topology: thread group not found")
                .clone();
            assert!(
                tg.ty() == ThreadGroupType::User,
                "task topology: kthread {} cannot dethread",
                self.tgid()
            );

            // make sure no new thread can join this thread group.
            tg.inner.write().status.is_dethreading = true;

            let members_to_kill = tg
                .inner
                .read()
                .members
                .iter()
                .filter(|member_tid| **member_tid != self.tid())
                .map(|member_tid| {
                    topology
                        .tasks
                        .get(member_tid)
                        .expect("task topology: task not found")
                        .task
                        .clone()
                })
                .collect::<Vec<_>>();

            (tg, members_to_kill)
        };

        for member in members_to_kill {
            member.recv_signal(Signal::new(
                SigNo::SIGKILL,
                SiCode::Kernel,
                SigInfoFields::Kill(SigKill {
                    pid: tg.tgid(),
                    uid: self.cred().uid.real,
                }),
            ));
        }

        // now busy wait until all other members have exited.
        loop {
            if tg.ntasks() == 1 {
                break;
            }

            // such a simple operation does not deserve a event or something like that. just
            // busy wait.
            yield_now();
        }

        // ok. one last thing: if we are not the original leader, we should
        // become the new leader of the thread group.

        let old_tid = self.tid();
        let new_tid = self.tgid();
        if !self.is_tg_leader() {
            // this transition must be a topology transaction.
            // whoa! what a big lock chain... but i do think it's necessary.
            let mut topology = TOPOLOGY.inner.write();
            let mut tg_inner = tg.inner.write();
            let mut tid = self.tid.write();
            debug_assert!(matches!(*tid, TidRef::Owned(_)));
            *tid = TidRef::Leader;
            assert!(
                topology.tasks.remove(&old_tid).is_some(),
                "task topology: task not found when dethreading"
            );
            assert!(
                topology
                    .tasks
                    .insert(new_tid, TaskNode { task: self.clone() })
                    .is_none(),
                "task topology: duplicate task ID when dethreading"
            );
            // tg now has only one member, which is this task. but its key in
            // member map is still the old TID, and we should update it to
            // leader TID.
            assert!(
                tg_inner.members.remove(&old_tid),
                "task topology: old TID not found in thread group when dethreading"
            );
            assert!(
                tg_inner.members.insert(new_tid),
                "task topology: duplicate TID in thread group when dethreading"
            );
        }

        // dethreading done.
        tg.inner.write().status.is_dethreading = false;

        kdebugln!(
            "{} (previously {}) has dethreaded its thread group {}",
            new_tid,
            old_tid,
            tg.tgid()
        );
    }
}
