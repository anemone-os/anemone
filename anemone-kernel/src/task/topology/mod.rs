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
pub mod process_group;
pub use process_group::{get_process_group, get_session};
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
    inner: NoIrqRwLock<TaskTopologyInner>,
}

/// [Vec] + [HashMap] would be better, but we don't have intrusive list,
/// so removing tasks from [Vec] will be quite expensive.
///
/// That's why [BTreeMap] is used here, which provides both efficient
/// lookup (but still slower than [HashMap], generally) and ordered
/// iteration.
#[derive(Debug)]
struct TaskTopologyInner {
    /// this one for lookup.
    tasks: BTreeMap<Tid, TaskNode>,

    thread_groups: BTreeMap<Tid, Arc<ThreadGroup>>,
    process_groups: BTreeMap<Tid, Arc<ProcessGroup>>,
    sessions: BTreeMap<Tid, Arc<Session>>,
}

#[derive(Debug)]
struct TaskNode {
    task: Arc<Task>,
}

static TOPOLOGY: TaskTopology = TaskTopology {
    inner: NoIrqRwLock::new(TaskTopologyInner {
        tasks: BTreeMap::new(),
        thread_groups: BTreeMap::new(),
        process_groups: BTreeMap::new(),
        sessions: BTreeMap::new(),
    }),
};

/// idle tasks don't register. init task uses [PublishGuard::register_root]
/// and thus has no parent. other tasks must have a parent.
#[derive(Debug)]
pub enum TaskBinding {
    UserLeader {
        parent_tgid: Tid,
        pgid: Tid,
        sid: Tid,
        /// To put this field here is a bit weird, but [publish_task] is where a
        /// [ThreadGroup] is constructed.
        ///
        /// We do need refactor this later.
        terminate_signal: Option<SigNo>,
    },
    KThread,
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
        let mut topology = TOPOLOGY.inner.write();
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
        task.tid = NoIrqRwLock::new(TidRef::Leader);

        let task = Arc::new(task);
        let node = TaskNode { task: task.clone() };
        let session = Arc::new(Session {
            sid: Tid::INIT,
            inner: NoIrqRwLock::new(SessionInner {
                process_groups: BTreeSet::from([Tid::INIT]),
            }),
        });
        let pg = Arc::new(ProcessGroup {
            pgid: Tid::INIT,
            sid: Tid::INIT,
            inner: NoIrqRwLock::new(ProcessGroupInner {
                members: BTreeSet::from([Tid::INIT]),
            }),
        });
        let tg = ThreadGroup {
            tgid: handle,
            ty: ThreadGroupType::User,
            child_exited: Event::new(),
            terminate_signal: None,
            itimers: ITimers::new(),
            inner: NoIrqRwLock::new(ThreadGroupInner {
                status: ThreadGroupStatus::new_alive_executed(),
                pgid: Some(Tid::INIT),
                sid: Some(Tid::INIT),
                members: BTreeSet::from([Tid::INIT]),
                parent_tgid: None,
                children_tgids: BTreeSet::new(),
                cpu_usage: ThreadGroupCpuUsage::ZERO,
                sig_pending: NoIrqSpinLock::new(PendingSignals::new()),
            }),
        };
        assert_thread_group_shape(Tid::INIT, &tg);

        topology.tasks.insert(Tid::INIT, node);
        topology.sessions.insert(Tid::INIT, session);
        topology.process_groups.insert(Tid::INIT, pg);
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
    let mut topology = TOPOLOGY.inner.write();
    match binding {
        TaskBinding::UserLeader {
            parent_tgid,
            pgid,
            sid,
            terminate_signal,
        } => {
            let tidref = task.tid.into_inner();
            let handle = match tidref {
                TidRef::Owned(h) => h,
                _ => panic!("task topology: leader task must have an owned TID handle"),
            };
            task.tid = NoIrqRwLock::new(TidRef::Leader);

            let task = Arc::new(task);

            let node = TaskNode { task: task.clone() };

            let inner = ThreadGroupInner {
                status: ThreadGroupStatus::new_alive(),
                pgid: Some(pgid),
                sid: Some(sid),
                members: BTreeSet::from([node.task.tid()]),
                parent_tgid: Some(parent_tgid),
                children_tgids: BTreeSet::new(),
                cpu_usage: ThreadGroupCpuUsage::ZERO,
                sig_pending: NoIrqSpinLock::new(PendingSignals::new()),
            };

            let session = topology
                .sessions
                .get(&sid)
                .expect("task topology: inherited session not found when publishing task");
            let pg = topology
                .process_groups
                .get(&pgid)
                .expect("task topology: inherited process group not found when publishing task");
            assert!(
                pg.sid == sid,
                "task topology: process group {} belongs to session {}, not {}",
                pgid,
                pg.sid,
                sid
            );
            let session_inner = session.inner.read();
            assert!(
                session_inner.process_groups.contains(&pgid),
                "task topology: process group {} not linked from session {}",
                pgid,
                sid
            );
            assert!(
                pg.inner.write().members.insert(node.task.tgid()),
                "task topology: duplicate process TGID {} in process group {}",
                node.task.tgid(),
                pgid
            );
            drop(session_inner);

            let parent_tg = topology
                .thread_groups
                .get(&parent_tgid)
                .expect("task topology: parent thread group not found when publishing task");
            assert!(
                parent_tg.ty() == ThreadGroupType::User,
                "task topology: user thread group {} cannot use non-user parent {}",
                node.task.tgid(),
                parent_tgid
            );
            assert!(
                parent_tg
                    .inner
                    .write()
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
                        Arc::new({
                            let tg = ThreadGroup {
                                tgid: handle,
                                ty: ThreadGroupType::User,
                                child_exited: Event::new(),
                                terminate_signal,
                                itimers: ITimers::new(),
                                inner: NoIrqRwLock::new(inner),
                            };
                            assert_thread_group_shape(tgid, &tg);
                            tg
                        })
                    )
                    .is_none(),
                "task topology: duplicate TGID {} when publishing task",
                tgid,
            );

            Ok(task)
        },
        TaskBinding::KThread => {
            let tidref = task.tid.into_inner();
            let handle = match tidref {
                TidRef::Owned(h) => h,
                _ => panic!("task topology: kthread leader must have an owned TID handle"),
            };
            task.tid = NoIrqRwLock::new(TidRef::Leader);
            assert!(
                tid == tgid,
                "task topology: kthread leader must have tid == tgid, got tid={} tgid={}",
                tid,
                tgid
            );
            assert!(
                task.flags().is_kernel(),
                "task topology: TaskBinding::KThread requires a kernel task"
            );
            assert!(
                task.kthread.lock().is_some(),
                "task topology: TaskBinding::KThread requires task-local kthread state before publish"
            );

            let task = Arc::new(task);
            let node = TaskNode { task: task.clone() };
            let tg = ThreadGroup {
                tgid: handle,
                ty: ThreadGroupType::KThread,
                child_exited: Event::new(),
                terminate_signal: None,
                itimers: ITimers::new(),
                inner: NoIrqRwLock::new(ThreadGroupInner {
                    status: ThreadGroupStatus::new_alive(),
                    pgid: None,
                    sid: None,
                    members: BTreeSet::from([node.task.tid()]),
                    parent_tgid: None,
                    children_tgids: BTreeSet::new(),
                    cpu_usage: ThreadGroupCpuUsage::ZERO,
                    sig_pending: NoIrqSpinLock::new(PendingSignals::new()),
                }),
            };
            assert_thread_group_shape(tgid, &tg);

            assert!(
                topology.tasks.insert(node.task.tid(), node).is_none(),
                "task topology: duplicate TID {} when publishing task",
                tid
            );
            assert!(
                topology.thread_groups.insert(tgid, Arc::new(tg)).is_none(),
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
            assert!(
                tg.ty() == ThreadGroupType::User,
                "task topology: thread-group members can only join user thread groups"
            );
            if !tg.status().can_join() {
                knoticeln!(
                    "task topology: thread group {} is not in a state that can accept new members when publishing {}",
                    tg.tgid(),
                    task.tid()
                );
                // idk whether Again here is appropriate, but it looks not that bac... we'd
                // better consider this later.
                return Err((task, SysError::Again));
            }
            let task = Arc::new(task);

            assert!(
                tg.inner.write().members.insert(task.tid()),
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

fn assert_thread_group_shape(tgid: Tid, tg: &ThreadGroup) {
    let inner = tg.inner.read();
    match tg.ty {
        ThreadGroupType::User => {
            assert!(
                inner.pgid.is_some(),
                "task topology: user thread group {} missing process group",
                tgid
            );
            assert!(
                inner.sid.is_some(),
                "task topology: user thread group {} missing session",
                tgid
            );
        },
        ThreadGroupType::KThread => {
            assert!(
                inner.pgid.is_none() && inner.sid.is_none(),
                "task topology: kthread {} must not have process group/session",
                tgid
            );
            assert!(
                inner.children_tgids.is_empty(),
                "task topology: kthread {} must not own user child topology",
                tgid
            );
            assert!(
                inner.members.len() == 1 && inner.members.contains(&tgid),
                "task topology: kthread {} must be a singleton thread group",
                tgid
            );
        },
    }
}

/// Get a task by its [Tid].
///
/// ## Locks
///
/// [TOPOLOGY]
pub fn get_task(tid: &Tid) -> Option<Arc<Task>> {
    let topology = TOPOLOGY.inner.read();
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
    let topology = TOPOLOGY.inner.read();
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
    let topology = TOPOLOGY.inner.read();
    topology.thread_groups.get(tgid).cloned()
}

/// Run `f` while the requested thread group is still active in topology.
///
/// This intentionally holds the topology read lock across `f`. It exists for
/// procfs lazy `<tgid>` binding creation so lookup can take locks in topology
/// -> procfs-binding order and cannot rebuild a binding once kthread unpublish
/// has acquired the topology write lock.
pub(crate) fn with_active_thread_group<R>(
    tgid: &Tid,
    f: impl FnOnce(&Arc<ThreadGroup>) -> R,
) -> Option<R> {
    let topology = TOPOLOGY.inner.read();
    topology.thread_groups.get(tgid).map(f)
}

/// Return whether a TGID is currently an active kernel-thread thread group.
///
/// User-facing resolvers use this as a fail-closed policy hook before touching
/// User-only process-group/session accessors.
pub fn is_kthread_tgid(tgid: Tid) -> bool {
    get_thread_group(&tgid)
        .map(|tg| tg.ty() == ThreadGroupType::KThread)
        .unwrap_or(false)
}

/// Iterate over all thread groups in the registry.
///
/// Internally, thread groups are stored in a [BTreeMap], so the iteration order
/// is ascending order of TGID:
/// - If `from` is provided, then the iteration starts from the first TGID that
///   is greater than or equal to `from`.
/// - If `from` is [`None`], then the iteration starts from the smallest TGID.
///
/// # Locks
///
/// [TOPOLOGY]
pub fn for_each_thread_group_from<F: FnMut(&Arc<ThreadGroup>)>(mut f: F, from: Option<Tid>) {
    let topology = TOPOLOGY.inner.read();
    let iter = match from {
        Some(from) => topology.thread_groups.range(from..),
        None => topology.thread_groups.range(..),
    };
    for (_, tg) in iter {
        f(tg);
    }
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
    let topology = TOPOLOGY.inner.read();

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
