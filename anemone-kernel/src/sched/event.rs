//! Event, Publisher, and Listener.
//!
//! Almost the same as linux's wait queue, but with a more appropriate name,
//! maybe?

use crate::prelude::*;

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
    /// This [SpinLock] must be embedded into [Event] itself, cz the correctness
    /// of [Event] relies on certain lock ordering. If we put the [SpinLock]
    /// outside of [Event], then the safety can't be guaranteed.
    ///
    /// **Only [SpinLock::lock_irqsave] can be used, since an [Event] can be
    /// accessed from interrupt context.**
    inner: SpinLock<EventInner>,
}

#[derive(Debug)]
struct EventInner {
    non_exclusive: VecDeque<Listener>,
    exclusive: VecDeque<Listener>,
}

impl Event {
    pub const fn new() -> Self {
        Self {
            inner: SpinLock::new(EventInner {
                non_exclusive: VecDeque::new(),
                exclusive: VecDeque::new(),
            }),
        }
    }

    /// This method is intenionally designed not to return any error. I.e. it's
    /// a ***fire-and-forget*** operation. Caller is both not expected and
    /// unable to handle any error.
    pub fn publish(&self, n_exclusive: usize, wakeup_uninterruptible: bool) {
        let mut to_wakeup = vec![];

        {
            let mut inner = self.inner.lock_irqsave();

            // 1. wake up non-exclusive listeners. they are usually those observers who
            //    don't consume resources.
            while let Some(listener) = inner.non_exclusive.pop_front() {
                to_wakeup.push(listener);
            }

            // 2. wake up exclusive listeners. they are usually those actors who consume
            //    resources, so we only wake up a few of them.
            for _ in 0..n_exclusive {
                let Some(listener) = inner.exclusive.pop_front() else {
                    break;
                };
                to_wakeup.push(listener);
            }
        }

        for listener in to_wakeup {
            knoticeln!("waking up listener {:?}", listener);
            if let Err(e) = try_to_wake_up(
                &listener.task,
                if wakeup_uninterruptible {
                    &[
                        TaskStatus::Waiting {
                            interruptible: false,
                        },
                        TaskStatus::Waiting {
                            interruptible: true,
                        },
                    ]
                } else {
                    &[TaskStatus::Waiting {
                        interruptible: true,
                    }]
                },
            ) {
                knoticeln!(
                    "failed to wake up listener {:?} for task {}, error: {:?}, maybe it has been woken up by other event?",
                    listener,
                    listener.task.tid(),
                    e,
                );
            }
        }
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
        let listener = Listener { task: task.clone() };
        let ret;

        let mut guard = PreemptGuard::new();

        loop {
            // ugly and costly... we should use intrusive linked list later.
            self.prepare_listener(&task, exclusive, true);

            // if a preemption occurs here, then the listener will never be woken up!

            if prediction() {
                ret = true;
                break;
            }

            if task.has_unmasked_signal() {
                kdebugln!(
                    "task {} has unmasked signal, breaking the wait loop",
                    task.tid()
                );
                ret = false;
                break;
            }

            unsafe {
                drop(guard);
                with_intr_disabled(|| {
                    schedule();
                });
                guard = PreemptGuard::new();
            }
        }

        self.clean_listener(&listener, exclusive);

        ret
    }

    /// Block listening. Won't be woken up by signals.
    ///
    /// **Ensure no lock or guard is held when calling this method.**
    pub fn listen_uninterruptible<P>(&self, exclusive: bool, prediction: P)
    where
        P: Fn() -> bool,
    {
        let task = get_current_task();
        let listener = Listener { task: task.clone() };

        let mut guard = PreemptGuard::new();

        loop {
            // ugly and costly... we should use intrusive linked list later.
            self.prepare_listener(&task, exclusive, false);

            // if a preemption occurs here, then the listener will never be woken up!

            if prediction() {
                break;
            }

            // do not check pending signals here, since this is uninterruptible wait.

            unsafe {
                drop(guard);
                with_intr_disabled(|| {
                    schedule();
                });
                guard = PreemptGuard::new();
            }
        }

        self.clean_listener(&listener, exclusive);
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
        let listener = Listener { task: task.clone() };

        let mut guard = PreemptGuard::new();

        loop {
            self.prepare_listener(&task, exclusive, true);

            if prediction() {
                self.clean_listener(&listener, exclusive);
                return None;
            }

            if task.has_unmasked_signal() {
                kdebugln!(
                    "task {} has unmasked signal, breaking the wait loop",
                    task.tid()
                );
                self.clean_listener(&listener, exclusive);
                return Some(TimeoutListenException::Signaled);
            }

            if timeout == Duration::ZERO {
                kdebugln!("listen_with_timeout: timeout is zero, returning immediately");
                self.clean_listener(&listener, exclusive);
                return Some(TimeoutListenException::Timeout);
            }

            unsafe {
                drop(guard);
                timeout = schedule_with_timeout(Some(timeout));
                guard = PreemptGuard::new();
            }
        }
    }
}

#[derive(Debug)]
pub enum TimeoutListenException {
    Timeout,
    Signaled,
}

impl Event {
    fn prepare_listener(&self, listener: &Arc<Task>, exclusive: bool, interruptible: bool) {
        let mut inner = self.inner.lock_irqsave();
        listener.update_status_with(|_prev| {
            let listener = Listener {
                task: listener.clone(),
            };

            if exclusive {
                if inner.exclusive.contains(&listener) {
                    knoticeln!(
                        "task {} is already listening to this event exclusively, won't add it again",
                        listener.task.tid()
                    );
                    return (TaskStatus::Waiting { interruptible }, ());
                }
                inner.exclusive.push_back(listener);
            } else {
                if inner.non_exclusive.contains(&listener) {
                    knoticeln!(
                        "task {} is already listening to this event non-exclusively, won't add it again",
                        listener.task.tid()
                    );
                    return (TaskStatus::Waiting { interruptible }, ());
                }
                inner.non_exclusive.push_back(listener);
            }

            (TaskStatus::Waiting { interruptible }, ())
        })
    }

    fn clean_listener(&self, listener: &Listener, exclusive: bool) {
        let mut inner = self.inner.lock_irqsave();
        listener.task.update_status_with(|_prev| {
            if exclusive {
                inner.exclusive.retain(|l| l != listener);
            } else {
                inner.non_exclusive.retain(|l| l != listener);
            }

            (TaskStatus::Runnable, ())
        });
    }
}

#[derive(Debug, Clone)]
struct Listener {
    task: Arc<Task>,
}

impl PartialEq for Listener {
    fn eq(&self, other: &Self) -> bool {
        self.task.tid().eq(&other.task.tid())
    }
}

impl Eq for Listener {}
