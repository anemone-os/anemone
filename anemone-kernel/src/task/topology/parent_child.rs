//! Inter-thread group operations.

use crate::{
    prelude::*,
    task::{tid::Tid, topology::TOPOLOGY},
};

impl ThreadGroup {
    /// Get the parent thread group ID of this thread group.
    ///
    /// For init and idle thread groups, this will return [None].
    pub fn parent_tgid(&self) -> Option<Tid> {
        self.inner.read_irqsave().parent_tgid
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
        let parent_tgid = self
            .inner
            .read_irqsave()
            .parent_tgid
            .expect("task topology: parent thread group not found");

        let topology = TOPOLOGY.inner.read_irqsave();

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
        let mut topology = TOPOLOGY.inner.write_irqsave();

        let parent_tgid = self
            .inner
            .read_irqsave()
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
    /// **To iterate over all tasks in the child thread groups, you should not
    /// call [ThreadGroup::for_each_member]** in the closure, which will cause a
    /// dead lock. Instead call [ThreadGroup::for_each_child_task]. **
    ///
    /// ## Locks
    /// - [TOPOLOGY] -> [ThreadGroup]
    /// - [TOPOLOGY] -> ‵f‵
    pub fn for_each_child<F: FnMut(&Arc<ThreadGroup>)>(&self, mut f: F) {
        let topology = TOPOLOGY.inner.read_irqsave();
        for child_tgid in self.inner.read_irqsave().children_tgids.iter() {
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
        let topology = TOPOLOGY.inner.read_irqsave();
        for child_tgid in self.inner.read_irqsave().children_tgids.iter() {
            let child_tg = topology
                .thread_groups
                .get(child_tgid)
                .expect("task topology: child thread group not found");
            for member_tid in child_tg.inner.read_irqsave().members.iter() {
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
        let child_tgids = self.inner.read_irqsave().children_tgids.clone();
        let topology = TOPOLOGY.inner.read_irqsave();
        child_tgids
            .iter()
            // filter_map must be used here cz there is a window between the read and the clone.
            .filter_map(|child_tgid| topology.thread_groups.get(child_tgid).cloned())
            .collect()
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
        let topology = TOPOLOGY.inner.read_irqsave();

        for child_tgid in self.inner.read_irqsave().children_tgids.iter() {
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
        // we may need get_many_mut... but it's still a nightly feature.
        let mut topology = TOPOLOGY.inner.read_irqsave();
        let mut child_tgids = vec![];
        while let Some(child_tgid) = self.inner.write_irqsave().children_tgids.pop_last() {
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
            child_tg.inner.write_irqsave().parent_tgid = Some(Tid::INIT);
        }

        let init_tg = topology
            .thread_groups
            .get(&Tid::INIT)
            .expect("task topology: init thread group not found");

        for child_tgid in child_tgids {
            assert!(
                init_tg
                    .inner
                    .write_irqsave()
                    .children_tgids
                    .insert(child_tgid),
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
    /// group at the same time.
    pub fn try_reap_child(&self, child_tgid: Tid) -> Option<Arc<ThreadGroup>> {
        let mut topology = TOPOLOGY.inner.write_irqsave();

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
            self.inner
                .write_irqsave()
                .children_tgids
                .remove(&child_tgid),
            "task topology: child thread group {} disappeared from parent {} when reaping",
            child_tgid,
            self.tgid()
        );

        Some(child_tg)
    }
}

impl Task {
    /// Whether this task is the init task, the ancestor of all other tasks.
    pub fn is_init(&self) -> bool {
        self.tid() == Tid::INIT
    }
}
