//! Lightweight kernel thread creation and lifecycle core.
//!
//! Callers submit create requests to `kthreadd`; `kthreadd` creates and
//! publishes the `Task`, then the task is enqueued so the typed entry shim can
//! recover the leaked start object. Ordinary kthread stop/exited state is
//! owned by `KThreadControl`, not by `TaskSchedState` or `kthreadd`.

use core::ptr::NonNull;

use crate::prelude::*;

mod create;

pub use create::{KThreadBuilder, init_kthreadd};

/// The public entry signature accepted by kthread creation.
///
/// This is deliberately a plain static function pointer. The low-level task
/// entry is installed as a code address, while owned state is passed through
/// the typed argument `A`.
pub type KThreadEntry<A> = fn(KThreadContext, A) -> i32;

/// The erased shim signature installed as the low-level kernel task entry.
///
/// The shim immediately casts the erased `NonNull<()>` back to
/// `KThreadStart<A>` and restores the `Box`.
pub type KThreadShimEntry = fn(NonNull<()>) -> !;

/// External handle for a created kthread.
///
/// This is only a weak reference. Stop and normal entry return clear the
/// task-owned `Arc<KThread>`, so later `upgrade()` returns `None`.
#[derive(Debug, Clone)]
pub struct KThreadRef {
    thread: Weak<KThread>,
}

impl KThreadRef {
    fn new(thread: &Arc<KThread>) -> Self {
        Self {
            thread: Arc::downgrade(thread),
        }
    }

    pub fn upgrade(&self) -> Option<Arc<KThread>> {
        self.thread.upgrade()
    }
}

#[derive(Debug)]
pub struct KThread {
    task: Weak<Task>,
    control: KThreadControl,
}

impl KThread {
    pub fn wake(&self) {
        self.control.wake();
    }

    pub fn stop(self: &Arc<Self>) -> i32 {
        let code = self.control.stop_and_wait();
        self.detach_from_task();
        code
    }

    fn finish_returned_entry(self: &Arc<Self>, code: i32) {
        self.control.finish_returned_entry(code);
        self.detach_from_task();
    }

    fn detach_from_task(self: &Arc<Self>) {
        if let Some(task) = self.task.upgrade() {
            task.clear_kthread(self);
        }
    }

    pub fn has_exited(&self) -> bool {
        matches!(self.control.state(), KThreadRunState::Exited)
    }

    pub fn state(&self) -> Option<KThreadSnapshot> {
        self.task.upgrade().map(|task| self.control.snapshot(&task))
    }

    pub fn tid(&self) -> Option<Tid> {
        self.task.upgrade().map(|task| task.tid())
    }

    pub fn name(&self) -> Option<Box<str>> {
        self.task.upgrade().map(|task| task.name())
    }
}

impl Drop for KThread {
    fn drop(&mut self) {
        if !self.has_exited() {
            if let Some(task) = self.task.upgrade() {
                panic!(
                    "kthread dropped while alive: tid={} name={}",
                    task.tid(),
                    task.name()
                );
            } else {
                panic!("kthread dropped while alive after task was dropped");
            }
        }
    }
}

impl Task {
    pub fn install_kthread(&self, kthread: Arc<KThread>) {
        let mut slot = self.kthread.lock();
        assert!(
            slot.is_none(),
            "kthread installed more than once for task {}",
            self.tid()
        );
        *slot = Some(kthread);
    }

    pub fn kthread(&self) -> Option<Arc<KThread>> {
        self.kthread.lock().clone()
    }

    pub fn clear_kthread(&self, kthread: &Arc<KThread>) {
        let mut slot = self.kthread.lock();
        if slot
            .as_ref()
            .map(|installed| Arc::ptr_eq(installed, kthread))
            .unwrap_or(false)
        {
            *slot = None;
        }
    }
}

/// Context passed to kthread entries.
///
/// The context lets the running kthread observe lifecycle requests at explicit
/// safe points. It does not expose the underlying `Task`.
#[derive(Debug, Clone)]
pub struct KThreadContext {
    thread: Arc<KThread>,
}

impl KThreadContext {
    pub fn should_stop(&self) -> bool {
        matches!(
            self.thread.control.state(),
            KThreadRunState::StopRequested | KThreadRunState::Exited
        )
    }

    /// Wait for a business predicate or a stop request.
    ///
    /// This keeps stop wakeups in the kthread lifecycle protocol instead of
    /// forcing each worker to duplicate stop-aware wait predicates.
    pub fn wait_until<P>(&self, event: &Event, predicate: P)
    where
        P: Fn() -> bool,
    {
        event.listen_uninterruptible(false, || self.should_stop() || predicate());
    }

    /// Wait on the kthread lifecycle wake event until the entry must recheck
    /// stop state or its own business predicate.
    ///
    /// `wake()` is only a wake capability. The consumer remains responsible for
    /// storing and checking any business request truth before and after waking.
    pub fn wait_until_woken<P>(&self, predicate: P)
    where
        P: Fn() -> bool,
    {
        self.thread
            .control
            .wake_event
            .listen_uninterruptible(false, || self.should_stop() || predicate());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KThreadRunState {
    Running,
    StopRequested,
    Exited,
}

#[derive(Debug, Clone)]
pub struct KThreadSnapshot {
    pub tid: Tid,
    pub name: Box<str>,
    pub state: KThreadRunState,
    pub exit_code: Option<i32>,
}

#[derive(Debug)]
struct KThreadInner {
    /// Lifecycle state. This is the single truth source for kthread lifecycle;
    /// scheduler state remains owned by `TaskSchedState`.
    state: KThreadRunState,
    /// Stable once the entry has returned. `KThread::stop()` returns this
    /// value.
    exit_code: i32,
}

/// Lifecycle owner for one ordinary kthread.
///
/// `KThreadControl` owns stop/exited state and the wake/state-change
/// events. It is not a scheduler entity and it does not own request queue
/// state.
#[derive(Debug)]
struct KThreadControl {
    inner: SpinLock<KThreadInner>,
    wake_event: Event,
    state_changed: Event,
}

impl KThreadControl {
    fn new() -> Self {
        Self {
            inner: SpinLock::new(KThreadInner {
                state: KThreadRunState::Running,
                exit_code: 0,
            }),
            wake_event: Event::new(),
            state_changed: Event::new(),
        }
    }

    fn wake(&self) {
        self.wake_event.publish(usize::MAX, true);
    }

    fn stop_and_wait(&self) -> i32 {
        self.request_stop();
        self.wake();
        self.wait_exited()
    }

    fn request_stop(&self) {
        let changed = {
            let mut inner = self.inner.lock();
            match inner.state {
                KThreadRunState::Exited | KThreadRunState::StopRequested => false,
                _ => {
                    inner.state = KThreadRunState::StopRequested;
                    true
                },
            }
        };
        if changed {
            self.state_changed.publish(usize::MAX, true);
        }
    }

    fn finish_returned_entry(&self, code: i32) {
        {
            let mut inner = self.inner.lock();
            inner.exit_code = code;
            inner.state = KThreadRunState::Exited;
        }
        self.state_changed.publish(usize::MAX, true);
        self.wake();
    }

    fn wait_exited(&self) -> i32 {
        self.state_changed.listen_uninterruptible(false, || {
            matches!(self.inner.lock().state, KThreadRunState::Exited)
        });
        self.inner.lock().exit_code
    }

    fn state(&self) -> KThreadRunState {
        self.inner.lock().state
    }

    fn snapshot(&self, task: &Task) -> KThreadSnapshot {
        let inner = self.inner.lock();
        KThreadSnapshot {
            tid: task.tid(),
            name: task.name(),
            state: inner.state,
            exit_code: if matches!(inner.state, KThreadRunState::Exited) {
                Some(inner.exit_code)
            } else {
                None
            },
        }
    }
}
