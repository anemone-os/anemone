//! Topology of tasks. Parent-child relationships, thread groups, process
//! groups, session, and so on.
//!
//! TODO: write a chapter in book to state the difference between **Memory
//! Object Consistency** and **Topology Consistency**.

use super::*;
use crate::{prelude::*, task::cpu_usage::ThreadGroupCpuUsage};
use core::mem::ManuallyDrop;

// infras
mod deferred;
pub use deferred::*;

// concrete topology
pub mod parent_child;
pub mod thread_group;

/// Global task topology. Singleton instance.
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

/// [Vec] + [HashMap] would be better, but we don't have intrusive list,
/// so removing tasks from [Vec] will be quite expensive.
///
/// That's why [BTreeMap] is used here, which provides both efficient
/// lookup (but still slower than [HashMap], generally) and ordered
/// iteration.
#[derive(Debug)]
struct TaskTopologyInner {
    tasks: BTreeMap<Tid, TaskNode>,
    thread_groups: BTreeMap<Tid, Arc<ThreadGroup>>,
}

#[derive(Debug)]
struct TaskNode {
    task: Arc<Task>,
}

static TOPOLOGY: TaskTopology = TaskTopology {
    inner: RwLock::new(TaskTopologyInner {
        tasks: BTreeMap::new(),
        thread_groups: BTreeMap::new(),
    }),
};

/// idle tasks don't register. init task uses [PublishGuard::register_root]
/// and thus has no parent. other tasks must have a parent.
#[derive(Debug)]
pub enum TaskBinding {
    Leader {
        parent_tgid: Tid,
        /// To put this field here is a bit weird, but [publish_task] is where a
        /// [ThreadGroup] is constructed.
        ///
        /// We do need refactor this later.
        terminate_signal: Option<SigNo>,
    },
    Member,
}

/// When creating a new task, this guard will be returned as well.
///
/// You must call either `publish` or `forget` on the guard, otherwise a
/// panic will occur when the guard is dropped.
#[derive(Debug)]
pub struct PublishGuard;

impl PublishGuard {
    /// This operation might fail if the linked topology (e.g. parent task,
    /// thread group) is accidentally dropped right before registration. Or some
    /// other unexpected edge cases.
    ///
    /// For most cases, currently we just panic for simplicity. When we have
    /// time we should make this more robust.
    pub fn publish(self, task: Task, binding: TaskBinding) -> Result<Arc<Task>, (Task, SysError)> {
        let ret = publish_task(task, binding);
        let _ = ManuallyDrop::new(self);
        ret
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
    /// method as [PublishGuard::register_root].
    pub unsafe fn register_root(guard: Self, mut task: Task) -> Arc<Task> {
        let mut topology = TOPOLOGY.inner.write_irqsave();
        debug_assert!(
            topology.tasks.is_empty(),
            "task topology: root task must be registered before any other task"
        );
        debug_assert!(
            task.tid() == Tid::INIT,
            "task topology: root task must have TID 1"
        );
        debug_assert!(
            task.tgid() == Tid::INIT,
            "task topology: root task must have TGID 1"
        );

        let tid_ref = task.tid.into_inner();
        let handle = match tid_ref {
            TidRef::Owned(h) => h,
            _ => panic!("task topology: leader task must have an owned TID handle"),
        };
        task.tid = RwLock::new(TidRef::Leader);

        let task = Arc::new(task);
        let node = TaskNode { task: task.clone() };
        let tg = ThreadGroup {
            tgid: handle,
            child_exited: Event::new(),
            terminate_signal: None,
            inner: RwLock::new(ThreadGroupInner {
                status: ThreadGroupStatus::new_alive(),
                members: BTreeSet::from([Tid::INIT]),
                parent_tgid: None,
                children_tgids: BTreeSet::new(),
                cpu_usage: ThreadGroupCpuUsage::ZERO,
                sig_pending: SpinLock::new(PendingSignals::new()),
            }),
        };

        topology.tasks.insert(Tid::INIT, node);
        topology.thread_groups.insert(Tid::INIT, Arc::new(tg));
        let _ = ManuallyDrop::new(guard);

        task
    }
}

impl Drop for PublishGuard {
    fn drop(&mut self) {
        panic!("a task was dropped without being explicitly published or forgotten");
    }
}

/// Publish a task to global-visible topology. Registration and topology update
/// are done atomically in this function.
fn publish_task(mut task: Task, binding: TaskBinding) -> Result<Arc<Task>, (Task, SysError)> {
    let tid = task.tid();
    let tgid = task.tgid();
    let mut topology = TOPOLOGY.inner.write_irqsave();
    match binding {
        TaskBinding::Leader {
            parent_tgid,
            terminate_signal,
        } => {
            let tidref = task.tid.into_inner();
            let handle = match tidref {
                TidRef::Owned(h) => h,
                _ => panic!("task topology: leader task must have an owned TID handle"),
            };
            task.tid = RwLock::new(TidRef::Leader);

            let task = Arc::new(task);

            let node = TaskNode { task: task.clone() };

            let mut inner = ThreadGroupInner {
                status: ThreadGroupStatus::new_alive(),
                members: BTreeSet::from([node.task.tid()]),
                parent_tgid: Some(parent_tgid),
                children_tgids: BTreeSet::new(),
                cpu_usage: ThreadGroupCpuUsage::ZERO,
                sig_pending: SpinLock::new(PendingSignals::new()),
            };

            inner.parent_tgid = Some(parent_tgid);

            let parent_tg = topology
                .thread_groups
                .get(&parent_tgid)
                .expect("task topology: parent thread group not found when publishing task");
            assert!(
                parent_tg
                    .inner
                    .write_irqsave()
                    .children_tgids
                    .insert(node.task.tgid()),
                "task topology: duplicate child TGID {} when publishing task",
                node.task.tgid()
            );
            assert!(
                topology.tasks.insert(node.task.tid(), node).is_none(),
                "task topology: duplicate TID {} when publishing task",
                tid
            );
            assert!(
                topology
                    .thread_groups
                    .insert(
                        tgid,
                        Arc::new(ThreadGroup {
                            tgid: handle,
                            child_exited: Event::new(),
                            terminate_signal,
                            inner: RwLock::new(inner),
                        })
                    )
                    .is_none(),
                "task topology: duplicate TGID {} when publishing task",
                tgid,
            );

            Ok(task)
        },
        TaskBinding::Member => {
            let tg = topology
                .thread_groups
                .get(&task.tgid())
                .expect("task topology: thread group not found when publishing task");
            if !tg.status().can_join() {
                knoticeln!(
                    "task topology: thread group {} is not in a state that can accept new members when publishing task {}",
                    tg.tgid(),
                    task.tid()
                );
                // idk whether Again here is appropriate, but it looks not that bac... we'd
                // better consider this later.
                return Err((task, SysError::Again));
            }
            let task = Arc::new(task);

            assert!(
                tg.inner.write_irqsave().members.insert(task.tid()),
                "task topology: duplicate member TID {} when publishing task",
                tid
            );
            let node = TaskNode { task: task.clone() };
            assert!(
                topology.tasks.insert(tid, node).is_none(),
                "task topology: duplicate TID {} when publishing task",
                tid
            );

            Ok(task)
        },
    }
}

/// Get a task by its [Tid].
///
/// ## Locks
///
/// [TOPOLOGY]
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
/// This is an very very expensive operation. use with caution.
///
/// ## Locks
///
/// [TOPOLOGY]
pub fn for_each_task<F: FnMut(&Arc<Task>)>(mut f: F) {
    let topology = TOPOLOGY.inner.read_irqsave();
    for node in topology.tasks.values() {
        f(&node.task);
    }
}

/// Get a thread group by its TGID.
///
/// # Locks
///
/// [TOPOLOGY]
pub fn get_thread_group(tgid: &Tid) -> Option<Arc<ThreadGroup>> {
    let topology = TOPOLOGY.inner.read_irqsave();
    topology.thread_groups.get(tgid).cloned()
}

#[derive(Debug, Clone, Copy)]
pub struct TopologyStat {
    pub ntasks: usize,
    pub nthread_groups: usize,
    // TODO: more statistics.
}

/// Get the topology statistics.
///
/// ## Locks
///
/// [TOPOLOGY]
pub fn topology_stat() -> TopologyStat {
    let topology = TOPOLOGY.inner.read_irqsave();

    TopologyStat {
        ntasks: topology.tasks.len(),
        nthread_groups: topology.thread_groups.len(),
    }
}

// impl Task {
//     /// When you want to lock multiple tasks, a consistent lock ordering must
// be     /// followed to avoid deadlocks.
//     ///
//     /// We adopt a simple lock ordering strategy based on TID: when locking
//     /// multiple tasks, always lock the task with smaller TID first.
//     pub fn lock_ordering<'a>(x: &'a Task, y: &'a Task) -> (&'a Task, &'a
// Task) {         if x.tid() < y.tid() { (x, y) } else { (y, x) }
//     }
// }
