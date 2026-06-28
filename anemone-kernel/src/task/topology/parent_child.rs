//! Inter-thread group operations.

use crate::{
    fs,
    prelude::*,
    task::{ThreadGroupType, tid::Tid, topology::TOPOLOGY},
};

#[derive(Debug, Clone, Copy)]
pub struct ProcThreadGroupDisplay {
    pub ppid: Tid,
    pub pgrp: Tid,
    pub session: Tid,
}

impl ThreadGroup {
    /// Get the parent thread group ID of this thread group.
    ///
    /// For init and idle thread groups, this will return [None].
    pub fn parent_tgid(&self) -> Option<Tid> {
        assert!(
            self.ty() == ThreadGroupType::User,
            "task topology: kthread {} has no user parent",
            self.tgid()
        );
        self.inner.read().parent_tgid
    }

    /// Display-only parent/process-group/session values for procfs.
    ///
    /// These fields are not topology truth. User-process APIs must use
    /// User-only accessors (`parent_tgid`, `pgid`, `sid`) after checking
    /// `ThreadGroupType`; procfs is the only consumer allowed to show inert
    /// kthread values.
    pub fn proc_display_parentage(&self) -> ProcThreadGroupDisplay {
        match self.ty() {
            ThreadGroupType::User => ProcThreadGroupDisplay {
                ppid: self.parent_tgid().unwrap_or(Tid::new(0)),
                pgrp: self.pgid(),
                session: self.sid(),
            },
            ThreadGroupType::KThread => ProcThreadGroupDisplay {
                ppid: if self.tgid() == Tid::KTHREADD {
                    Tid::new(0)
                } else {
                    Tid::KTHREADD
                },
                pgrp: Tid::new(0),
                session: Tid::new(0),
            },
        }
    }

    /// Get the parent thread group.
    ///
    /// Only **Object Consistency** is guaranteed.
    ///
    /// # Panics
    ///
    /// Panics if the thread group is init or idle, which should not happen.
    ///
    /// # Locks
    ///
    /// - [TOPOLOGY]
    /// - self [ThreadGroup]
    pub fn get_parent(&self) -> Arc<ThreadGroup> {
        assert!(
            self.ty() == ThreadGroupType::User,
            "task topology: kthread {} has no user parent",
            self.tgid()
        );
        let parent_tgid = self
            .inner
            .read()
            .parent_tgid
            .expect("task topology: parent thread group not found");

        let topology = TOPOLOGY.inner.read();

        let parent = topology
            .thread_groups
            .get(&parent_tgid)
            .expect("task topology: parent thread group not found")
            .clone();

        parent
    }

    /// Run a closure with the parent thread group of this thread group.
    ///
    /// Internally this function locks the topology, so we can make sure that
    /// the parent thread group will stay stable during the execution of the
    /// closure, which is what we want.
    ///
    /// This method provides **Topology Consistency** at the cost of locking the
    /// topology for a longer time. Caution should be taken to avoid deadlocks.
    ///
    /// # Panics
    ///
    /// Panics if this thread group is init or idle, which should not happen.
    ///
    /// ## Locks
    ///
    /// - [TOPOLOGY] -> [ThreadGroup]
    /// - [TOPOLOGY] -> ‵f‵
    pub fn with_parent<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Arc<ThreadGroup>) -> R,
    {
        assert!(
            self.ty() == ThreadGroupType::User,
            "task topology: kthread {} has no user parent",
            self.tgid()
        );
        let topology = TOPOLOGY.inner.write();

        let parent_tgid = self
            .inner
            .read()
            .parent_tgid
            .expect("task topology: parent thread group not found");

        let parent_tg = topology
            .thread_groups
            .get(&parent_tgid)
            .expect("task topology: parent thread group not found");

        f(parent_tg)
    }

    /// Iterate over the children thread groups of this thread group.
    ///
    /// This method provides **Topology Consistency** at the cost of locking the
    /// topology for a longer time. Caution should be taken to avoid deadlocks.
    ///
    /// **To iterate over all tasks in the child thread groups, do not call
    /// [ThreadGroup::for_each_member] or take a member snapshot inside the
    /// closure**, because that would try to reacquire topology locks. Instead call
    /// [ThreadGroup::for_each_child_task].
    ///
    /// ## Locks
    /// - [TOPOLOGY] -> [ThreadGroup]
    /// - [TOPOLOGY] -> ‵f‵
    pub fn for_each_child<F: FnMut(&Arc<ThreadGroup>)>(&self, mut f: F) {
        assert!(
            self.ty() == ThreadGroupType::User,
            "task topology: kthread {} has no user children",
            self.tgid()
        );
        let topology = TOPOLOGY.inner.read();
        for child_tgid in self.inner.read().children_tgids.iter() {
            let child_tg = topology
                .thread_groups
                .get(child_tgid)
                .expect("task topology: child thread group not found");
            f(child_tg);
        }
    }

    /// Iterate over all tasks in the child thread groups of this thread group.
    ///
    /// This method provides **Topology Consistency** at the cost of locking the
    /// topology for a longer time. Caution should be taken to avoid deadlocks.
    ///
    /// ## Locks
    /// [TOPOLOGY] -> self [ThreadGroup] -> child [ThreadGroup]
    ///
    /// This suffers a **very very big hazard** of deadlock!!! You'd better call
    /// those collectors first, and then apply the closure to them. Consider if
    /// you really need such strong consistency guarantee before using this
    /// method.
    pub fn for_each_child_task<F: FnMut(&Arc<Task>)>(&self, mut f: F) {
        assert!(
            self.ty() == ThreadGroupType::User,
            "task topology: kthread {} has no user children",
            self.tgid()
        );
        let topology = TOPOLOGY.inner.read();
        for child_tgid in self.inner.read().children_tgids.iter() {
            let child_tg = topology
                .thread_groups
                .get(child_tgid)
                .expect("task topology: child thread group not found");
            for member_tid in child_tg.inner.read().members.iter() {
                let member = topology
                    .tasks
                    .get(member_tid)
                    .expect("task topology: child task not found")
                    .task
                    .clone();
                f(&member);
            }
        }
    }

    /// Collect all children thread groups of this thread group into a vector.
    ///
    /// Only **Object Consistency** is guaranteed.
    ///
    /// ## Locks
    /// - [TOPOLOGY]
    /// - self [ThreadGroup]
    ///
    /// Useful when the lock chain is too deep.
    pub fn get_children(&self) -> Vec<Arc<ThreadGroup>> {
        assert!(
            self.ty() == ThreadGroupType::User,
            "task topology: kthread {} has no user children",
            self.tgid()
        );
        let child_tgids = self.inner.read().children_tgids.clone();
        let topology = TOPOLOGY.inner.read();
        child_tgids
            .iter()
            // filter_map must be used here cz there is a window between the read and the clone.
            .filter_map(|child_tgid| topology.thread_groups.get(child_tgid).cloned())
            .collect()
    }

    /// Get the number of children thread groups of this thread group.
    pub fn nchildren(&self) -> usize {
        assert!(
            self.ty() == ThreadGroupType::User,
            "task topology: kthread {} has no user children",
            self.tgid()
        );
        self.inner.read().children_tgids.len()
    }

    /// Find a child thread group of the thread group of this task by its TGID.
    ///
    /// ## Locks
    ///
    /// [TOPOLOGY] -> [ThreadGroup]
    pub fn find_child<P: FnMut(&Arc<ThreadGroup>) -> bool>(
        &self,
        mut prediction: P,
    ) -> Option<Arc<ThreadGroup>> {
        assert!(
            self.ty() == ThreadGroupType::User,
            "task topology: kthread {} has no user children",
            self.tgid()
        );
        let topology = TOPOLOGY.inner.read();

        for child_tgid in self.inner.read().children_tgids.iter() {
            let child_tg = topology
                .thread_groups
                .get(child_tgid)
                .expect("task topology: child thread group not found");
            if prediction(child_tg) {
                return Some(child_tg.clone());
            }
        }
        None
    }

    /// Reparent all children of this thread group to init. This is called when
    /// a thread group is exiting.
    ///
    /// **Topology Consistency** is guaranteed natually due to the objective of
    /// this method.
    pub fn reparent_orphan_children(&self) {
        assert!(
            self.ty() == ThreadGroupType::User,
            "task topology: kthread {} cannot reparent user children",
            self.tgid()
        );
        // we may need get_many_mut... but it's still a nightly feature.
        let topology = TOPOLOGY.inner.read();
        let mut child_tgids = vec![];
        while let Some(child_tgid) = self.inner.write().children_tgids.pop_last() {
            knoticeln!(
                "reparenting orphan child thread group {} of parent thread group {}",
                child_tgid,
                self.tgid()
            );
            child_tgids.push(child_tgid);
        }

        for child_tgid in &child_tgids {
            let child_tg = topology.thread_groups.get(child_tgid).expect(
                "task topology: child thread group not found when reparenting orphan children",
            );
            assert!(
                child_tg.ty() == ThreadGroupType::User,
                "task topology: non-user child thread group {} in user child topology",
                child_tgid
            );
            child_tg.inner.write().parent_tgid = Some(Tid::INIT);
        }

        let init_tg = topology
            .thread_groups
            .get(&Tid::INIT)
            .expect("task topology: init thread group not found");

        for child_tgid in child_tgids {
            assert!(
                init_tg.inner.write().children_tgids.insert(child_tgid),
                "task topology: duplicate child TGID {} when reparenting orphan children",
                child_tgid
            );
        }
    }

    /// Reap a child thread group of this thread group. This is called when all
    /// tasks in a child thread group have exited, and the thread group itself
    /// is ready to be reaped.
    ///
    /// After this operation, the child thread group will be removed from the
    /// topology.
    ///
    /// Returns [None] if child thread group with the given TGID is not found,
    /// which may happen if multiple threads tries to reap the same child thread
    /// group at the same time. Or if the child thread group is not actually a
    /// child of this thread group, which may happen if the child thread group
    /// is reparented to init at some point.
    ///
    /// TODO: maybe we should make this method return a
    /// Result<Option<Arc<ThreadGroup>>, SomeError> to distinguish those two
    /// cases?
    pub fn try_reap_child(&self, child_tgid: Tid) -> Option<Arc<ThreadGroup>> {
        assert!(
            self.ty() == ThreadGroupType::User,
            "task topology: kthread {} cannot reap user children",
            self.tgid()
        );
        let mut topology = TOPOLOGY.inner.write();

        // make sure this is indeed a child thread group of us.
        if !self.inner.read().children_tgids.contains(&child_tgid) {
            return None;
        }

        let child_tg = topology.thread_groups.remove(&child_tgid)?;

        assert!(
            matches!(
                child_tg.status().life_cycle(),
                ThreadGroupLifeCycle::Exited(_)
            ),
            "task topology: child thread group {} is not exited yet when reaping",
            child_tgid
        );
        assert!(
            child_tg.parent_tgid() == Some(self.tgid()),
            "task topology: child thread group {} has unexpected parent {:?} when reaping",
            child_tgid,
            child_tg.parent_tgid()
        );
        assert!(
            child_tg.ntasks() == 0,
            "task topology: child thread group {} is not empty when reaping",
            child_tgid
        );

        assert!(
            self.inner.write().children_tgids.remove(&child_tgid),
            "task topology: child thread group {} disappeared from parent {} when reaping",
            child_tgid,
            self.tgid()
        );

        Some(child_tg)
    }

    /// Unpublish a singleton kthread from active topology and procfs.
    ///
    /// Kthreads do not enter ordinary children, process-group/session, or
    /// wait/reap topology. This transaction is therefore the only lifecycle
    /// owner for their procfs-visible identity, and procfs only receives a
    /// narrow binding invalidation hook.
    pub fn unpublish_kthread_topology(&self) {
        assert!(
            self.ty() == ThreadGroupType::KThread,
            "task topology: user thread group {} cannot use kthread unpublish",
            self.tgid()
        );
        let tgid = self.tgid();
        let mut topology = TOPOLOGY.inner.write();
        let tg = topology
            .thread_groups
            .get(&tgid)
            .expect("task topology: kthread thread group not found during unpublish")
            .clone();
        assert!(
            tg.ty() == ThreadGroupType::KThread,
            "task topology: kthread unpublish found non-kthread thread group {}",
            tgid
        );

        let mut inner = tg.inner.write();
        assert!(
            inner.members.len() == 1 && inner.members.contains(&tgid),
            "task topology: kthread {} must be singleton during unpublish",
            tgid
        );
        assert!(
            inner.children_tgids.is_empty(),
            "task topology: kthread {} must not own children during unpublish",
            tgid
        );
        assert!(inner.members.remove(&tgid));
        drop(inner);

        // Topology owns the unpublish transaction and takes the locks in the
        // documented order: topology first, procfs binding second. Marking the
        // procfs binding dead before removing topology membership prevents
        // operation-time revalidation from accepting a kthread that is already
        // exiting, while lookup cannot rebuild a binding until this topology
        // write lock is released.
        fs::proc::invalidate_thread_group_binding(tgid);
        assert!(
            topology.tasks.remove(&tgid).is_some(),
            "task topology: kthread task {} not found during unpublish",
            tgid
        );
        assert!(
            topology.thread_groups.remove(&tgid).is_some(),
            "task topology: kthread thread group {} not found during unpublish",
            tgid
        );
    }
}

impl Task {
    /// Whether this task is the init task, the ancestor of all other tasks.
    pub fn is_init(&self) -> bool {
        self.tid() == Tid::INIT
    }
}
