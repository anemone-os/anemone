//! Event, Publisher, and Listener.
//!
//! Almost the same as linux's wait queue, but with a more appropriate name,
//! maybe?

use core::fmt::{Debug, Formatter};

use crate::{prelude::*, sched::wait};

/// An occurrence of an event does not guarantee that the event is still valid
/// when the listener wakes up, so the listener should always check the
/// condition after waking up, and if the condition is not satisfied, it should
/// wait again. This is the same as the "spurious wakeup" in linux's wait queue.
/// Google "lost wakeup" for more details.
///
/// [Event] does not transfer data. It just notifies.
///
/// Listening to an [Event] is always blocking.
///
/// Event naming convention: `xxxed` for events that have already happened,
/// `xxxing` for events that are happening.
///
/// **System Invariant: A [Task] can only listen to one [Event] at a time.**
#[derive(Debug)]
pub struct Event {
    /// This [NoIrqSpinLock] must be embedded into [Event] itself, cz the
    /// correctness of [Event] relies on certain lock ordering. If we put the
    /// [NoIrqSpinLock] outside of [Event], then the safety can't be guaranteed.
    inner: NoIrqSpinLock<EventInner>,
}

#[derive(Debug)]
struct EventInner {
    non_exclusive: VecDeque<Listener>,
    exclusive: VecDeque<Listener>,
}

impl Event {
    pub const fn new() -> Self {
        Self {
            inner: NoIrqSpinLock::new(EventInner {
                non_exclusive: VecDeque::new(),
                exclusive: VecDeque::new(),
            }),
        }
    }

    /// This method is intenionally designed not to return any error. I.e. it's
    /// a ***fire-and-forget*** operation. Caller is both not expected and
    /// unable to handle any error.
    pub fn publish(&self, n_exclusive: usize, wakeup_uninterruptible: bool) {
        let mode = if wakeup_uninterruptible {
            WakeMode::AnyWait
        } else {
            WakeMode::InterruptibleOnly
        };

        let non_exclusive_len = self.inner.lock().non_exclusive.len();
        kdebugln!(
            "event: publish begin event={:#x} non_exclusive={} exclusive_quota={} mode={:?}",
            self.debug_id(),
            non_exclusive_len,
            n_exclusive,
            mode,
        );

        for _ in 0..non_exclusive_len {
            let Some(listener) = self.pop_listener(ListenerQueueKind::NonExclusive) else {
                break;
            };
            self.wake_detached_listener(listener, ListenerQueueKind::NonExclusive, mode);
        }

        let exclusive_len = self.inner.lock().exclusive.len();
        let mut exclusive_success = 0;
        let mut exclusive_scanned = 0;

        while exclusive_success < n_exclusive && exclusive_scanned < exclusive_len {
            let Some(listener) = self.pop_listener(ListenerQueueKind::Exclusive) else {
                break;
            };
            exclusive_scanned += 1;

            if self.wake_detached_listener(listener, ListenerQueueKind::Exclusive, mode) {
                exclusive_success += 1;
                kdebugln!(
                    "event: exclusive quota consumed event={:#x} success={} quota={}",
                    self.debug_id(),
                    exclusive_success,
                    n_exclusive,
                );
            }
        }

        kdebugln!(
            "event: publish end event={:#x} exclusive_scanned={} exclusive_success={}",
            self.debug_id(),
            exclusive_scanned,
            exclusive_success,
        );
    }

    /// Blocking listen. Interruptible by signals.
    ///
    /// Return true if the listener wakes up because the prediction is
    /// satisfied, false if the listener wakes up because of a signal.
    ///
    /// **Ensure no lock or guard is held when calling this method.**
    pub fn listen<P>(&self, exclusive: bool, prediction: P) -> bool
    where
        P: Fn() -> bool,
    {
        let task = get_current_task();
        let mut guard = PreemptGuard::new();

        loop {
            let (wait_guard, listener) = self.prepare_listener(&task, exclusive, true);

            if prediction() {
                wait::cancel_wait(&wait_guard, WaitReason::PredicateReady);
                self.clean_listener(&listener, exclusive);
                wait::finish_wait(wait_guard);
                return true;
            }

            if task.has_unmasked_signal() {
                kdebugln!("{} has unmasked signal, breaking the wait loop", task.tid());
                wait::cancel_wait(&wait_guard, WaitReason::Signal);
                self.clean_listener(&listener, exclusive);
                wait::finish_wait(wait_guard);
                return false;
            }

            unsafe {
                drop(guard);
                with_intr_disabled(|| {
                    schedule();
                });
                guard = PreemptGuard::new();
            }

            self.clean_listener(&listener, exclusive);
            let outcome = wait::finish_wait(wait_guard);
            kdebugln!(
                "event: listen woke event={:#x} task={} listener={:?} outcome={:?}",
                self.debug_id(),
                task.tid(),
                listener,
                outcome,
            );
            if matches!(
                outcome,
                WaitOutcome::Completed(WaitReason::Signal | WaitReason::Force)
            ) {
                return false;
            }
        }
    }

    /// Block listening. Won't be woken up by signals.
    ///
    /// **Ensure no lock or guard is held when calling this method.**
    pub fn listen_uninterruptible<P>(&self, exclusive: bool, prediction: P)
    where
        P: Fn() -> bool,
    {
        let task = get_current_task();
        let mut guard = PreemptGuard::new();

        loop {
            let (wait_guard, listener) = self.prepare_listener(&task, exclusive, false);

            if prediction() {
                wait::cancel_wait(&wait_guard, WaitReason::PredicateReady);
                self.clean_listener(&listener, exclusive);
                wait::finish_wait(wait_guard);
                return;
            }

            unsafe {
                drop(guard);
                with_intr_disabled(|| {
                    schedule();
                });
                guard = PreemptGuard::new();
            }

            self.clean_listener(&listener, exclusive);
            let outcome = wait::finish_wait(wait_guard);
            kdebugln!(
                "event: listen_uninterruptible woke event={:#x} task={} listener={:?} outcome={:?}",
                self.debug_id(),
                task.tid(),
                listener,
                outcome,
            );
        }
    }

    /// Block listening with timeout.
    ///
    /// Return:
    /// - [None] if condition is satisfied.
    /// - [TimeoutListenException::Timeout] if timeout expires.
    /// - [TimeoutListenException::Signaled] if woken up by signal.
    pub fn listen_with_timeout<P>(
        &self,
        exclusive: bool,
        prediction: P,
        mut timeout: Duration,
    ) -> Option<TimeoutListenException>
    where
        P: Fn() -> bool,
    {
        let task = get_current_task();
        let mut guard = PreemptGuard::new();

        loop {
            let (wait_guard, listener) = self.prepare_listener(&task, exclusive, true);

            if prediction() {
                wait::cancel_wait(&wait_guard, WaitReason::PredicateReady);
                self.clean_listener(&listener, exclusive);
                wait::finish_wait(wait_guard);
                return None;
            }

            if task.has_unmasked_signal() {
                kdebugln!("{} has unmasked signal, breaking the wait loop", task.tid());
                wait::cancel_wait(&wait_guard, WaitReason::Signal);
                self.clean_listener(&listener, exclusive);
                wait::finish_wait(wait_guard);
                return Some(TimeoutListenException::Signaled);
            }

            if timeout == Duration::ZERO {
                kdebugln!("listen_with_timeout: timeout is zero, returning immediately");
                wait::cancel_wait(&wait_guard, WaitReason::Timeout);
                self.clean_listener(&listener, exclusive);
                wait::finish_wait(wait_guard);
                return Some(TimeoutListenException::Timeout);
            }

            let token = listener.token().clone();
            let remaining = {
                drop(guard);
                self.schedule_with_wait_token_timeout(&task, token, timeout)
            };
            guard = PreemptGuard::new();

            self.clean_listener(&listener, exclusive);
            let outcome = wait::finish_wait(wait_guard);
            if matches!(
                outcome,
                WaitOutcome::Completed(WaitReason::Signal | WaitReason::Force)
            ) {
                return Some(TimeoutListenException::Signaled);
            }
            if matches!(outcome, WaitOutcome::Completed(WaitReason::Timeout)) {
                return Some(TimeoutListenException::Timeout);
            }

            let elapsed = timeout.saturating_sub(remaining);
            timeout = remaining;
            kdebugln!(
                "event: listen_with_timeout woke event={:#x} task={} listener={:?} outcome={:?} elapsed={:?} remaining={:?}",
                self.debug_id(),
                task.tid(),
                listener,
                outcome,
                elapsed,
                timeout,
            );
        }
    }
}

#[derive(Debug)]
pub enum TimeoutListenException {
    Timeout,
    Signaled,
}

impl Event {
    fn debug_id(&self) -> usize {
        self as *const Self as usize
    }

    fn prepare_listener(
        &self,
        task: &Arc<Task>,
        exclusive: bool,
        interruptible: bool,
    ) -> (WaitGuard, Listener) {
        let begin = wait::begin_wait(task, interruptible);
        let (guard, token) = begin.into_parts();
        let listener = Listener::new(task, token);
        self.register_listener(listener.clone(), exclusive);
        (guard, listener)
    }

    fn register_listener(&self, listener: Listener, exclusive: bool) {
        let mut inner = self.inner.lock();
        let queue = if exclusive {
            &mut inner.exclusive
        } else {
            &mut inner.non_exclusive
        };

        if queue.contains(&listener) {
            knoticeln!(
                "event: listener already registered event={:#x} listener={:?} exclusive={}",
                self.debug_id(),
                listener,
                exclusive,
            );
            return;
        }

        kdebugln!(
            "event: register listener event={:#x} listener={:?} exclusive={}",
            self.debug_id(),
            listener,
            exclusive,
        );
        queue.push_back(listener);
    }

    fn clean_listener(&self, listener: &Listener, exclusive: bool) {
        let mut inner = self.inner.lock();
        let queue = if exclusive {
            &mut inner.exclusive
        } else {
            &mut inner.non_exclusive
        };
        let old_len = queue.len();
        queue.retain(|l| l != listener);
        let removed = old_len != queue.len();
        kdebugln!(
            "event: clean listener event={:#x} listener={:?} exclusive={} removed={}",
            self.debug_id(),
            listener,
            exclusive,
            removed,
        );
    }

    fn pop_listener(&self, queue: ListenerQueueKind) -> Option<Listener> {
        let mut inner = self.inner.lock();
        let listener = match queue {
            ListenerQueueKind::NonExclusive => inner.non_exclusive.pop_front(),
            ListenerQueueKind::Exclusive => inner.exclusive.pop_front(),
        };

        if let Some(listener) = &listener {
            kdebugln!(
                "event: detach listener event={:#x} listener={:?} queue={:?}",
                self.debug_id(),
                listener,
                queue,
            );
        }

        listener
    }

    /// Return true only when an exclusive listener successfully consumed one
    /// publish quota slot.
    fn wake_detached_listener(
        &self,
        listener: Listener,
        queue: ListenerQueueKind,
        mode: WakeMode,
    ) -> bool {
        let Some(task) = listener.task() else {
            kdebugln!(
                "event: stale listener task dropped event={:#x} listener={:?}",
                self.debug_id(),
                listener,
            );
            return false;
        };

        let result = wait::wake_wait(&task, listener.token(), WaitReason::Event, mode);
        match result {
            WakeResult::Woke { placement } => {
                kdebugln!(
                    "event: listener woke event={:#x} task={} listener={:?} placement={:?}",
                    self.debug_id(),
                    task.tid(),
                    listener,
                    placement,
                );
                true
            },
            WakeResult::ModeBlocked => {
                let requeued =
                    self.requeue_blocked_listener_if_current_armed(listener, queue, mode);
                kdebugln!(
                    "event: mode-blocked listener requeue event={:#x} queue={:?} requeued={}",
                    self.debug_id(),
                    queue,
                    requeued,
                );
                false
            },
            WakeResult::Stale
            | WakeResult::AlreadyCompleted(_)
            | WakeResult::AlreadyCancelled(_)
            | WakeResult::Retired => {
                kdebugln!(
                    "event: discard listener event={:#x} task={} listener={:?} result={:?}",
                    self.debug_id(),
                    task.tid(),
                    listener,
                    result,
                );
                false
            },
        }
    }

    fn requeue_blocked_listener_if_current_armed(
        &self,
        listener: Listener,
        queue: ListenerQueueKind,
        mode: WakeMode,
    ) -> bool {
        let Some(task) = listener.task() else {
            kdebugln!(
                "event: requeue failed, task dropped event={:#x} listener={:?}",
                self.debug_id(),
                listener,
            );
            return false;
        };

        let Some(permit) = wait::requeue_permit_if_mode_blocked(&task, listener.token(), mode)
        else {
            kdebugln!(
                "event: requeue denied by wait core event={:#x} listener={:?}",
                self.debug_id(),
                listener,
            );
            return false;
        };

        let wait_id = permit.wait_id();
        let mut inner = self.inner.lock();
        let target_queue = match queue {
            ListenerQueueKind::NonExclusive => &mut inner.non_exclusive,
            ListenerQueueKind::Exclusive => &mut inner.exclusive,
        };
        if target_queue.contains(&listener) {
            kdebugln!(
                "event: requeue skipped duplicate event={:#x} listener={:?} wait={:#x} queue={:?}",
                self.debug_id(),
                listener,
                wait_id,
                queue,
            );
            return true;
        }

        target_queue.push_back(listener);
        kdebugln!(
            "event: requeued blocked listener event={:#x} wait={:#x} queue={:?}",
            self.debug_id(),
            wait_id,
            queue,
        );
        true
    }

    fn schedule_with_wait_token_timeout(
        &self,
        task: &Arc<Task>,
        token: WakeToken,
        timeout: Duration,
    ) -> Duration {
        schedule_wait_with_timeout(task, token, Some(timeout))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListenerQueueKind {
    NonExclusive,
    Exclusive,
}

#[derive(Clone)]
struct WaitTarget {
    task: Weak<Task>,
    token: WakeToken,
    tid: Tid,
}

impl WaitTarget {
    fn new(task: &Arc<Task>, token: WakeToken) -> Self {
        Self {
            task: Arc::downgrade(task),
            token,
            tid: task.tid(),
        }
    }
}

impl Debug for WaitTarget {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WaitTarget")
            .field("tid", &self.tid)
            .field("wait_id", &self.token.wait_id())
            .finish()
    }
}

#[derive(Clone)]
struct Listener {
    target: WaitTarget,
}

impl Listener {
    fn new(task: &Arc<Task>, token: WakeToken) -> Self {
        Self {
            target: WaitTarget::new(task, token),
        }
    }

    fn task(&self) -> Option<Arc<Task>> {
        self.target.task.upgrade()
    }

    fn token(&self) -> &WakeToken {
        &self.target.token
    }
}

impl Debug for Listener {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Listener")
            .field("target", &self.target)
            .finish()
    }
}

impl PartialEq for Listener {
    fn eq(&self, other: &Self) -> bool {
        self.target.token.same_wait(&other.target.token)
    }
}

impl Eq for Listener {}
