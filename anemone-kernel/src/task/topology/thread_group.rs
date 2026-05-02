//! Intra-thread group operations.

use crate::{
    prelude::*,
    task::topology::{TOPOLOGY, TaskNode, ThreadGroup},
};

impl ThreadGroup {
    /// Get the thread group ID.
    pub fn tgid(&self) -> Tid {
        self.tgid.get_typed()
    }

    /// How many members are in this thread group.
    pub fn ntasks(&self) -> usize {
        self.inner.read_irqsave().members.len()
    }

    /// Get the status of this thread group.
    pub fn status(&self) -> ThreadGroupStatus {
        self.inner.read_irqsave().status
    }

    pub fn update_life_cycle_with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&ThreadGroupLifeCycle) -> (ThreadGroupLifeCycle, R),
    {
        let mut inner = self.inner.write_irqsave();
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

    /// Iterate over all members of this thread group.
    ///
    /// **Topology Consistency** is guaranteed.
    ///
    /// ## Locks
    ///
    /// [TOPOLOGY] -> [ThreadGroup]
    pub fn for_each_member<F: FnMut(&Arc<Task>)>(&self, mut f: F) {
        let topology = TOPOLOGY.inner.read_irqsave();
        for member_tid in self.inner.read_irqsave().members.iter() {
            let member = &topology
                .tasks
                .get(member_tid)
                .expect("task topology: task not found")
                .task;
            f(member);
        }
    }

    /// Collect all members of this task's thread group into a vector.
    ///
    /// **Topology Consistency** is guaranteed.
    ///
    /// ## Locks
    /// [TOPOLOGY] -> [ThreadGroup]
    ///
    /// Useful when the lock chain is too deep.
    pub fn get_members(&self) -> Vec<Arc<Task>> {
        let topology = TOPOLOGY.inner.read_irqsave();
        let tg = topology
            .thread_groups
            .get(&self.tgid())
            .expect("task topology: thread group not found");
        tg.inner
            .read_irqsave()
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
    pub fn find_member<P: FnMut(&Arc<Task>) -> bool>(
        &self,
        mut prediction: P,
    ) -> Option<Arc<Task>> {
        let children = {
            let topology = TOPOLOGY.inner.read_irqsave();
            let tg = topology
                .thread_groups
                .get(&self.tgid())
                .expect("task topology: thread group not found");
            tg.inner
                .read_irqsave()
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
                .collect::<Vec<_>>()
        };
        children.into_iter().find(prediction)
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
        let topology = TOPOLOGY.inner.read_irqsave();
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
        let topology = TOPOLOGY.inner.read_irqsave();
        let tg = topology
            .thread_groups
            .get(&self.tgid())
            .expect("task topology: thread group not found");
        f(&tg)
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
        let mut topology = TOPOLOGY.inner.write_irqsave();

        let is_last = {
            let tg = topology
                .thread_groups
                .get(&self.tgid())
                .expect("task topology: thread group not found");

            let mut inner = tg.inner.write_irqsave();
            assert!(inner.members.remove(&self.tid()));
            inner.members.is_empty()
        };
        assert!(topology.tasks.remove(&self.tid()).is_some());

        is_last
    }

    /// Kill all other members in this task's thread group, who, if not the
    /// original leader, will become the new leader of the thread group.
    ///
    /// The thread group itself stays alive.
    pub fn dethread(self: &Arc<Self>) {
        let tg = {
            let topology = TOPOLOGY.inner.read_irqsave();

            // we can't call above encapsulated APIs here since that will cause deadlocks.

            let tg = topology
                .thread_groups
                .get(&self.tgid())
                .expect("task topology: thread group not found")
                .clone();

            // make sure no new thread can join this thread group.
            tg.inner.write_irqsave().status.is_dethreading = true;

            for member_tid in tg.inner.read_irqsave().members.iter() {
                if *member_tid != self.tid() {
                    let member = &topology
                        .tasks
                        .get(member_tid)
                        .expect("task topology: task not found")
                        .task;
                    member.set_killed();
                }
            }

            tg
        };

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
            let mut topology = TOPOLOGY.inner.write_irqsave();
            let mut tg_inner = tg.inner.write_irqsave();
            let mut tid = self.tid.write_irqsave();
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
        tg.inner.write_irqsave().status.is_dethreading = false;

        kdebugln!(
            "task {} (previously {}) has dethreaded its thread group {}",
            new_tid,
            old_tid,
            tg.tgid()
        );
    }
}
