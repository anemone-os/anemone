//! Topology of tasks. Parent-child relationships, thread groups, process
//! groups, session, and so on.
//!
//! TODO: write a chapter in book to state the difference between **Consistence
//! of Memory Object** and **Consistence of Topology**.

use crate::prelude::*;

use core::mem::ManuallyDrop;

use super::*;

/// Global task topology. Singleton instance.
///
/// One primary objective of this struct is to transfer all
/// task-destroying logic to a explicit and controllable point. If we simply
/// leave that in [Drop], then XXX (dead lock and performance fluctuations).
///
/// `xxx_irqsave` should be used when accessing task topology, because
/// accessing may happen in hwirq context (e.g. ipi handler).
///
/// **Task publishing/unpublishing, and topogily reading/updating, are all done
/// in the same lock, thus ensuring the atomicity of these transactions.**
///
/// We don't implement methods on this struct. We only want to gather related
/// data structures together, and the actual logic is implemented in free
/// functions.
///
/// TODO: fine-grained locking.
#[derive(Debug)]
struct TaskTopology {
    inner: RwLock<TaskTopologyInner>,
}

#[derive(Debug)]
struct TaskTopologyInner {
    /// [Vec] + [HashMap] would be better, but we don't have intrusive list,
    /// so removing tasks from [Vec] will be quite expensive.
    ///
    /// That's why [BTreeMap] is used here, which provides both efficient
    /// lookup (but still slower than [HashMap], generally) and ordered
    /// iteration.
    tasks: BTreeMap<Tid, TaskNode>,
}

#[derive(Debug)]
struct TaskNode {
    task: Arc<Task>,
    // TODO: tid and tgid are immutable during the lifetime of a task, so they should be stored
    // directly in Task. pgid and sid are mutable, so they should be stored here.
    parent: Option<Tid>,
    children: BTreeSet<Tid>,
}

static TOPOLOGY: TaskTopology = TaskTopology {
    inner: RwLock::new(TaskTopologyInner {
        tasks: BTreeMap::new(),
    }),
};

#[derive(Debug)]
pub struct TaskBinding {
    /// idle tasks don't register. init task uses [RegisterGuard::register_root]
    /// and thus has no parent. other tasks must have a parent.
    pub parent: Tid,
    // TODO: tgid, etc.
}

/// When creating a new task, this guard will be returned as well.
///
/// You must call either `register` or `forget` on the guard, otherwise a
/// panic will occur when the guard is dropped.
#[derive(Debug)]
pub struct RegisterGuard;

impl RegisterGuard {
    pub fn register(self, task: Task, binding: TaskBinding) -> Arc<Task> {
        let task = publish_task(task, binding);
        let _ = ManuallyDrop::new(self);
        task
    }

    /// Creating a task but not registering it to global registry? You'd
    /// better consider carefully...
    ///
    /// One example: idle tasks indeed need this method, because they are
    /// not registered to the global registry at all.
    pub unsafe fn forget(self) {
        let _ = ManuallyDrop::new(self);
    }

    /// Receiver type (self) is intentionally not used, you can only call this
    /// method as [RegisterGuard::register_root].
    pub unsafe fn register_root(guard: Self, task: Task) -> Arc<Task> {
        let mut topology = TOPOLOGY.inner.write_irqsave();
        debug_assert!(
            topology.tasks.is_empty(),
            "task topology: root task must be registered before any other task"
        );
        debug_assert!(
            task.tid() == Tid::INIT,
            "task topology: root task must have TID 1"
        );

        let task = Arc::new(task);
        let node = TaskNode {
            task: task.clone(),
            parent: None,
            children: BTreeSet::new(),
        };
        topology.tasks.insert(Tid::INIT, node);
        let _ = ManuallyDrop::new(guard);

        task
    }
}

impl Drop for RegisterGuard {
    fn drop(&mut self) {
        panic!("a task was dropped without being explicitly registered or forgotten");
    }
}

/// Publish a task to global-visible topology. Registration and topology update
/// are done atomically in this function.
fn publish_task(task: Task, binding: TaskBinding) -> Arc<Task> {
    let mut topology = TOPOLOGY.inner.write_irqsave();
    let tid = task.tid();

    let task = Arc::new(task);
    let node = TaskNode {
        task: task.clone(),
        parent: Some(binding.parent),
        children: BTreeSet::new(),
    };
    let parent_node = topology
        .tasks
        .get_mut(&binding.parent)
        .expect("task topology: parent task not found when publishing task");
    assert!(
        parent_node.children.insert(tid),
        "task topology: duplicate child TID {} when publishing task",
        tid
    );
    assert!(
        topology.tasks.insert(tid, node).is_none(),
        "task topology: duplicate TID {} when publishing task",
        tid
    );

    task
}

// /// Unpublish a task from global-visible topology.
// ///
// /// This functions required that the strong reference count of the task is 1,
// /// the one held by the topology itself. Otherwise we panic. Error-tolerant
// /// unpublishing may hide bugs and lead to inconsistent topology, so we want
// to /// be strict about this.
// ///
// /// TODO: we should differentiate between remove and reap?
// pub unsafe fn remove_task(tid: &Tid) -> Task {
//     let mut topology = TOPOLOGY.inner.write_irqsave();
//
//     let node = topology
//         .tasks
//         .remove(tid)
//         .unwrap_or_else(|| panic!("task topology: task not found when
// removing task: {}", tid));
//
//     todo!()
// }

/// Get a task by its [Tid].
pub fn get_task(tid: &Tid) -> Option<Arc<Task>> {
    let topology = TOPOLOGY.inner.read_irqsave();
    topology.tasks.get(tid).map(|node| node.task.clone())
}

/// Get the init task, the ancestor of all other tasks.
///
/// TODO: cache to avoid lookup.
pub fn get_init_task() -> Arc<Task> {
    get_task(&Tid::INIT).expect("task topology: init task not found")
}

/// Iterate over all tasks in the registry.
///
/// This is an expensive operation. use with caution.
pub fn for_each_task<F: FnMut(&Arc<Task>)>(mut f: F) {
    let topology = TOPOLOGY.inner.read_irqsave();
    for node in topology.tasks.values() {
        f(&node.task);
    }
}

// region: topology operations

impl Task {
    /// Whether this task is the init task, the ancestor of all other tasks.
    pub fn is_init(&self) -> bool {
        self.tid() == Tid::INIT
    }

    /// For init or idle tasks, this will return [None].
    ///
    /// Otherwise unwrapping this is always safe.
    pub fn parent_tid(&self) -> Option<Tid> {
        let topology = TOPOLOGY.inner.read_irqsave();
        //topology.tasks.get(&self.tid()).and_then(|node| node.parent)
        topology
            .tasks
            .get(&self.tid())
            .expect("task topology: task not found")
            .parent
    }

    /// Run a closure with the parent task of this task.
    ///
    /// Internally this function locks the topology, so we can make sure that
    /// the parent task will stay stable during the execution of the closure,
    /// which is what we want.
    ///
    /// This method provides **Topology Consistency** at the cost of locking the
    /// topology for a longer time. Caution should be taken to avoid deadlocks.
    ///
    /// # Panics
    ///
    /// Panics if this task is root or idle, which should not happen.
    pub fn with_parent_task<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&Arc<Task>) -> R,
    {
        let mut topology = TOPOLOGY.inner.write_irqsave();

        let parent_tid = topology
            .tasks
            .get(&self.tid())
            .expect("task topology: task not found")
            .parent
            .expect("task topology: parent task not found");

        let parent = topology
            .tasks
            .get(&parent_tid)
            .expect("task topology: parent task not found")
            .task
            .clone();

        f(&parent)
    }

    /// Get the parent task of this task.
    ///
    /// Only **Object Consistency** is guaranteed.
    ///
    /// # Panics
    ///
    /// Panics if this task is root or idle, which should not happen.
    pub fn get_parent_task(&self) -> Arc<Task> {
        let topology = TOPOLOGY.inner.read_irqsave();

        let parent_tid = topology
            .tasks
            .get(&self.tid())
            .expect("task topology: task not found")
            .parent
            .expect("task topology: parent task not found");

        let parent = topology
            .tasks
            .get(&parent_tid)
            .expect("task topology: parent task not found")
            .task
            .clone();

        parent
    }

    /// Iterate over the children of this task.
    ///
    /// This method provides **Topology Consistency** at the cost of locking the
    /// topology for a longer time. Caution should be taken to avoid deadlocks.
    pub fn for_each_child<F: FnMut(&Arc<Task>)>(&self, mut f: F) {
        let topology = TOPOLOGY.inner.read_irqsave();
        let node = topology
            .tasks
            .get(&self.tid())
            .expect("task topology: task not found");
        for child_tid in &node.children {
            let child = topology
                .tasks
                .get(child_tid)
                .expect("task topology: child task not found");
            f(&child.task);
        }
    }

    /// Find first child (order unspecified) of this task that satisfies the
    /// given prediction. Returns [None] if no such child is found.
    ///
    /// Only **Object Consistency** is guaranteed.
    pub fn find_child<P: FnMut(&Arc<Task>) -> bool>(&self, mut prediction: P) -> Option<Arc<Task>> {
        let children = {
            let topology = TOPOLOGY.inner.read_irqsave();
            let node = topology
                .tasks
                .get(&self.tid())
                .expect("task topology: task not found");
            node.children
                .iter()
                .map(|tid| {
                    topology
                        .tasks
                        .get(tid)
                        .expect("task topology: child task not found")
                        .task
                        .clone()
                })
                .collect::<Vec<_>>()
        };

        for child in children {
            if prediction(&child) {
                return Some(child);
            }
        }
        None
    }

    /// Reparent all children of this task to init. This is called when a task
    /// is exiting.
    ///
    /// **Topology Consistency** is guaranteed natually due to the objective of
    /// this method.
    pub fn reparent_orphan_children(&self) {
        // we may need get_many_mut... but it's still a nightly feature.
        let mut topology = TOPOLOGY.inner.write_irqsave();

        let mut child_tids = vec![];

        let node = topology
            .tasks
            .get_mut(&self.tid())
            .expect("task topology: task not found");
        while let Some(child_tid) = node.children.pop_last() {
            knoticeln!(
                "reparenting orphan child task {} of parent task {}",
                child_tid,
                self.tid()
            );
            child_tids.push(child_tid);
        }

        for child_tid in &child_tids {
            let child_node = topology
                .tasks
                .get_mut(child_tid)
                .expect("task topology: child task not found when reparenting orphan children");
            child_node.parent = Some(Tid::INIT);
        }

        let init_node = topology
            .tasks
            .get_mut(&Tid::INIT)
            .expect("task topology: init task not found");

        for child_tid in child_tids {
            assert!(
                init_node.children.insert(child_tid),
                "task topology: duplicate child TID {} when reparenting orphan children",
                child_tid
            );
        }
    }

    /// Contrast to [publish_task], this is the end of the life cycle of a task
    /// in visible topology.
    ///
    /// **Caller must guarantee that after this operation, the reaped task won't
    /// be exposed to outside observers anymore.**
    ///
    /// Note that reaping only removes the task from global-visible topology.
    /// The task may still be temporarily referenced by scheduler-local state
    /// on its owner CPU until the switch boundary fully completes.
    ///
    /// TODO: deferred queue.
    pub fn reap_child(&self, child: Tid) -> Arc<Task> {
        let mut topology = TOPOLOGY.inner.write_irqsave();

        let parent_tid = self.tid();

        let parent_node = topology
            .tasks
            .get_mut(&parent_tid)
            .expect("task topology: parent task not found when unlinking child");
        assert!(
            parent_node.children.remove(&child),
            "task topology: failed to unlink child {} from parent {}",
            child,
            parent_tid
        );

        let child_node = topology
            .tasks
            .remove(&child)
            .expect("task topology: child task disappeared while reaping");
        assert!(
            child_node.parent == Some(parent_tid),
            "task topology: child {} has unexpected parent {:?} when reaping",
            child,
            child_node.parent
        );

        child_node.task
    }
}

// endregion: topology operations
