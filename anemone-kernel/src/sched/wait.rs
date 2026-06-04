//! Scheduler wait core.
//!
//! Event, timeout, and signal wait paths are migrated onto this core. The
//! wait-core wake API owns both logical completion and stale-safe physical
//! placement.

use core::fmt::{Debug, Formatter};
use core::marker::PhantomData;

use crate::prelude::*;

/// Internal scheduler state for a task.
#[derive(Clone, Debug)]
pub enum TaskSchedState {
    Runnable,
    Waiting {
        state: Arc<WaitState>,
        interruptible: bool,
        park: ParkState,
    },
    Zombie,
}

impl TaskSchedState {
    pub fn as_task_status(&self) -> TaskStatus {
        match self {
            Self::Runnable => TaskStatus::Runnable,
            Self::Waiting { interruptible, .. } => {
                TaskStatus::Waiting {
                    interruptible: *interruptible,
                }
            },
            Self::Zombie => TaskStatus::Zombie,
        }
    }

    pub fn is_wait_core_waiting(&self) -> bool {
        matches!(self, Self::Waiting { .. })
    }

    pub fn is_runnable(&self) -> bool {
        matches!(self, Self::Runnable)
    }
}

/// Park latch state.  Phase 1 only creates the state container; `schedule()`
/// starts consuming the latch when stale-safe wake placement lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParkState {
    PrePark,
    Parked,
}

/// Why a wait round is completed or cancelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WaitReason {
    Event,
    Latch,
    Timeout,
    Signal,
    Force,
    PredicateReady,
    Cancelled,
}

/// Wake mode requested by a producer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WakeMode {
    InterruptibleOnly,
    AnyWait,
    Force,
}

impl WakeMode {
    fn allows(self, interruptible: bool) -> bool {
        match self {
            Self::InterruptibleOnly => interruptible,
            Self::AnyWait | Self::Force => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WaitStateStatus {
    Armed,
    Completed(WaitReason),
    Cancelled(WaitReason),
    Retired,
}

pub struct WaitState {
    status: NoIrqRwLock<WaitStateStatus>,
    created_by: Tid,
    created_at: Instant,
}

impl WaitState {
    fn new(task: &Task) -> Arc<Self> {
        Arc::new(Self {
            status: NoIrqRwLock::new(WaitStateStatus::Armed),
            created_by: task.tid(),
            created_at: Instant::now(),
        })
    }

    pub(super) fn status(&self) -> WaitStateStatus {
        *self.status.read()
    }

    pub fn debug_id(&self) -> usize {
        self as *const Self as usize
    }

    fn cancel_if_armed(&self, reason: WaitReason) -> WaitResult {
        let mut status = self.status.write();
        match *status {
            WaitStateStatus::Armed => {
                *status = WaitStateStatus::Cancelled(reason);
                WaitResult::Cancelled(reason)
            },
            WaitStateStatus::Completed(reason) => WaitResult::Completed(reason),
            WaitStateStatus::Cancelled(reason) => WaitResult::Cancelled(reason),
            WaitStateStatus::Retired => WaitResult::Retired,
        }
    }

    fn complete_if_armed(&self, reason: WaitReason) -> WaitTransition {
        let mut status = self.status.write();
        match *status {
            WaitStateStatus::Armed => {
                *status = WaitStateStatus::Completed(reason);
                WaitTransition::Completed
            },
            WaitStateStatus::Completed(reason) => WaitTransition::AlreadyCompleted(reason),
            WaitStateStatus::Cancelled(reason) => WaitTransition::AlreadyCancelled(reason),
            WaitStateStatus::Retired => WaitTransition::Retired,
        }
    }

    fn retire(&self) -> WaitOutcome {
        let mut status = self.status.write();
        let outcome = WaitOutcome::from_status(*status);
        *status = WaitStateStatus::Retired;
        outcome
    }
}

impl Debug for WaitState {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WaitState")
            .field("id", &self.debug_id())
            .field("status", &self.status())
            .field("created_by", &self.created_by)
            .field("created_at", &self.created_at)
            .finish()
    }
}

/// Capability held by the waiter.
///
/// It owns active cleanup and retirement for exactly one wait round.  It is not
/// cloneable by design.
#[derive(Debug)]
struct WaitGuard {
    task: Arc<Task>,
    state: Arc<WaitState>,
}

impl WaitGuard {
    fn wait_id(&self) -> usize {
        self.state.debug_id()
    }
}

/// Restricted wake capability held by event sources.
#[derive(Clone, Debug)]
pub(super) struct WakeToken {
    state: Arc<WaitState>,
}

impl WakeToken {
    pub fn wait_id(&self) -> usize {
        self.state.debug_id()
    }

    pub fn same_wait(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    pub(super) fn is_armed(&self) -> bool {
        self.state.status() == WaitStateStatus::Armed
    }
}

#[derive(Debug)]
struct BeginWait {
    guard: WaitGuard,
    token: WakeToken,
}

impl BeginWait {
    fn into_parts(self) -> (WaitGuard, WakeToken) {
        (self.guard, self.token)
    }
}

/// Owned handle for a current task wait round.
///
/// Higher-level wait adapters use this facade to begin, actively cancel, and
/// explicitly finish one wait round without exposing the raw lifecycle
/// primitives as general scheduler state operations.
#[derive(Debug)]
pub(super) struct ActiveWait {
    guard: WaitGuard,
    token: WakeToken,
}

impl ActiveWait {
    pub fn begin(task: &Arc<Task>, interruptible: bool) -> Self {
        let begin = begin_wait(task, interruptible);
        let (guard, token) = begin.into_parts();
        Self { guard, token }
    }

    pub fn token(&self) -> WakeToken {
        self.token.clone()
    }

    pub fn cancel(&self, reason: WaitReason) {
        cancel_wait(&self.guard, reason);
    }

    pub fn finish(self) -> WaitOutcome {
        finish_wait(self.guard)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WaitTransition {
    Completed,
    AlreadyCompleted(WaitReason),
    AlreadyCancelled(WaitReason),
    Retired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WaitResult {
    Cancelled(WaitReason),
    Completed(WaitReason),
    Retired,
    Stale,
}

impl WaitResult {
    fn from_status(status: WaitStateStatus) -> Self {
        match status {
            WaitStateStatus::Armed => Self::Stale,
            WaitStateStatus::Completed(reason) => Self::Completed(reason),
            WaitStateStatus::Cancelled(reason) => Self::Cancelled(reason),
            WaitStateStatus::Retired => Self::Retired,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WaitOutcome {
    Armed,
    Completed(WaitReason),
    Cancelled(WaitReason),
    Retired,
}

impl WaitOutcome {
    fn from_status(status: WaitStateStatus) -> Self {
        match status {
            WaitStateStatus::Armed => Self::Armed,
            WaitStateStatus::Completed(reason) => Self::Completed(reason),
            WaitStateStatus::Cancelled(reason) => Self::Cancelled(reason),
            WaitStateStatus::Retired => Self::Retired,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeEnqueueResult {
    Stale,
    AlreadyCurrent,
    ParkPending,
    AlreadyQueued,
    Enqueued,
}

/// Result for wake attempts through the wait core.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WakeResult {
    Woke {
        placement: WakeEnqueueResult,
    },
    ModeBlocked,
    Stale,
    AlreadyCompleted(WaitReason),
    AlreadyCancelled(WaitReason),
    Retired,
}

/// Short-lived capability proving a mode-blocked listener still belongs to the
/// current armed wait round.
///
/// The permit is intentionally non-cloneable and carries a borrow tied to the
/// sched-state transaction that created it. It only authorizes Event to requeue
/// the already-detached listener while that transaction is still live.
#[derive(Debug)]
pub(super) struct RequeuePermit<'a> {
    wait_id: usize,
    _guard: crate::sync::rwlock::WriteIrqSaveGuard<'a, TaskSchedState>,
    _not_send_sync: PhantomData<*mut ()>,
}

impl RequeuePermit<'_> {
    pub fn wait_id(&self) -> usize {
        self.wait_id
    }
}

/// Start one wait-core round for `task`.
///
/// The linearization point is the task scheduling-state transaction.  This API
/// stays private to the wait core.  Wait adapters should use [ActiveWait]
/// instead of reaching for raw lifecycle primitives.
fn begin_wait(task: &Arc<Task>, interruptible: bool) -> BeginWait {
    let state = WaitState::new(task);
    let guard = WaitGuard {
        task: task.clone(),
        state: state.clone(),
    };
    let token = WakeToken {
        state: state.clone(),
    };

    task.update_sched_state_with(|prev| {
        assert!(
            matches!(prev, TaskSchedState::Runnable),
            "begin_wait requires a runnable task, got {:?}",
            prev
        );
        (
            TaskSchedState::Waiting {
                state: state.clone(),
                interruptible,
                park: ParkState::PrePark,
            },
            (),
        )
    });

    kdebugln!(
        "wait_core: begin task={} wait={:#x} interruptible={}",
        task.tid(),
        state.debug_id(),
        interruptible,
    );

    BeginWait { guard, token }
}

/// Cancel a wait round owned by `guard`.
///
/// This is waiter-owned cleanup only. Remote or async completion must use
/// [wake_wait] or [wake_active_wait] so logical completion and stale-safe
/// placement stay coupled.
fn cancel_wait(guard: &WaitGuard, reason: WaitReason) -> WaitResult {
    let result = guard.task.update_sched_state_with(|prev| match prev {
        TaskSchedState::Waiting {
            state,
            interruptible,
            park,
        } if Arc::ptr_eq(&state, &guard.state) => {
            let result = guard.state.cancel_if_armed(reason);
            match result {
                WaitResult::Cancelled(_) => (TaskSchedState::Runnable, result),
                _ => (
                    TaskSchedState::Waiting {
                        state,
                        interruptible,
                        park,
                    },
                    result,
                ),
            }
        },
        _ => {
            let result = WaitResult::from_status(guard.state.status());
            (prev, result)
        },
    });

    kdebugln!(
        "wait_core: cancel task={} wait={:#x} reason={:?} result={:?}",
        guard.task.tid(),
        guard.wait_id(),
        reason,
        result,
    );

    result
}

/// Retire the wait round and return its final recorded outcome.
///
/// This is waiter-owned cleanup only. Remote or async completion must use
/// [wake_wait] or [wake_active_wait] so logical completion and stale-safe
/// placement stay coupled.
fn finish_wait(guard: WaitGuard) -> WaitOutcome {
    let outcome = guard.task.update_sched_state_with(|prev| match prev {
        TaskSchedState::Waiting { state, .. } if Arc::ptr_eq(&state, &guard.state) => {
            let outcome = guard.state.retire();
            (TaskSchedState::Runnable, Some(outcome))
        },
        _ => (prev, None),
    });
    let outcome = outcome.unwrap_or_else(|| guard.state.retire());

    kdebugln!(
        "wait_core: finish task={} wait={:#x} outcome={:?}",
        guard.task.tid(),
        guard.wait_id(),
        outcome,
    );

    outcome
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WakeCommit {
    Woke { park: ParkState },
    ModeBlocked,
    Stale,
    AlreadyCompleted(WaitReason),
    AlreadyCancelled(WaitReason),
    Retired,
}

impl From<WaitTransition> for WakeCommit {
    fn from(transition: WaitTransition) -> Self {
        match transition {
            WaitTransition::Completed => unreachable!("new completion must carry park state"),
            WaitTransition::AlreadyCompleted(reason) => Self::AlreadyCompleted(reason),
            WaitTransition::AlreadyCancelled(reason) => Self::AlreadyCancelled(reason),
            WaitTransition::Retired => Self::Retired,
        }
    }
}

/// Wake a wait round through a source-held token.
///
/// This is the remote/producer completion entry point. Do not use it for
/// waiter-owned cleanup; `cancel_wait()` and `finish_wait()` are the local
/// cleanup paths.
///
/// `WakeResult::Woke` means the wait core has completed the logical wake and
/// executed one stale-safe physical placement attempt.
pub(super) fn wake_wait(
    task: &Arc<Task>,
    token: &WakeToken,
    reason: WaitReason,
    mode: WakeMode,
) -> WakeResult {
    let commit = task.update_sched_state_with(|prev| match prev {
        TaskSchedState::Waiting {
            state,
            interruptible,
            park,
        } if Arc::ptr_eq(&state, &token.state) => {
            match state.status() {
                WaitStateStatus::Armed => {},
                WaitStateStatus::Completed(reason) => {
                    return (
                        TaskSchedState::Waiting {
                            state,
                            interruptible,
                            park,
                        },
                        WakeCommit::AlreadyCompleted(reason),
                    );
                },
                WaitStateStatus::Cancelled(reason) => {
                    return (
                        TaskSchedState::Waiting {
                            state,
                            interruptible,
                            park,
                        },
                        WakeCommit::AlreadyCancelled(reason),
                    );
                },
                WaitStateStatus::Retired => {
                    return (
                        TaskSchedState::Waiting {
                            state,
                            interruptible,
                            park,
                        },
                        WakeCommit::Retired,
                    );
                },
            }

            if !mode.allows(interruptible) {
                return (
                    TaskSchedState::Waiting {
                        state,
                        interruptible,
                        park,
                    },
                    WakeCommit::ModeBlocked,
                );
            }

            match state.complete_if_armed(reason) {
                WaitTransition::Completed => (TaskSchedState::Runnable, WakeCommit::Woke { park }),
                transition => (
                    TaskSchedState::Waiting {
                        state,
                        interruptible,
                        park,
                    },
                    WakeCommit::from(transition),
                ),
            }
        },
        _ => (prev, WakeCommit::Stale),
    });

    finish_wake_attempt(task, Some(token.wait_id()), reason, mode, commit)
}

/// Wake the currently active wait without an external token.
///
/// This is the remote/producer completion entry point for active waits. Do not
/// use it for waiter-owned cleanup; `cancel_wait()` and `finish_wait()` are the
/// local cleanup paths.
///
/// `WakeResult::Woke` means the wait core has completed the logical wake and
/// executed one stale-safe physical placement attempt.
pub(super) fn wake_active_wait(task: &Arc<Task>, reason: WaitReason, mode: WakeMode) -> WakeResult {
    let mut wait_id = None;
    let commit = task.update_sched_state_with(|prev| match prev {
        TaskSchedState::Waiting {
            state,
            interruptible,
            park,
        } => {
            wait_id = Some(state.debug_id());
            match state.status() {
                WaitStateStatus::Armed => {},
                WaitStateStatus::Completed(reason) => {
                    return (
                        TaskSchedState::Waiting {
                            state,
                            interruptible,
                            park,
                        },
                        WakeCommit::AlreadyCompleted(reason),
                    );
                },
                WaitStateStatus::Cancelled(reason) => {
                    return (
                        TaskSchedState::Waiting {
                            state,
                            interruptible,
                            park,
                        },
                        WakeCommit::AlreadyCancelled(reason),
                    );
                },
                WaitStateStatus::Retired => {
                    return (
                        TaskSchedState::Waiting {
                            state,
                            interruptible,
                            park,
                        },
                        WakeCommit::Retired,
                    );
                },
            }

            if !mode.allows(interruptible) {
                return (
                    TaskSchedState::Waiting {
                        state,
                        interruptible,
                        park,
                    },
                    WakeCommit::ModeBlocked,
                );
            }

            match state.complete_if_armed(reason) {
                WaitTransition::Completed => (TaskSchedState::Runnable, WakeCommit::Woke { park }),
                transition => (
                    TaskSchedState::Waiting {
                        state,
                        interruptible,
                        park,
                    },
                    WakeCommit::from(transition),
                ),
            }
        },
        _ => (prev, WakeCommit::Stale),
    });

    finish_wake_attempt(task, wait_id, reason, mode, commit)
}

/// Return a short-lived Event requeue permit when `token` still names the
/// task's current armed wait and `mode` is still blocked by its interruptible
/// flag.
///
/// This is not a general query API. The returned permit keeps the task
/// sched-state write guard alive so Event can requeue the detached listener
/// under the required task-state -> event-lock ordering.
pub(super) fn requeue_permit_if_mode_blocked<'a>(
    task: &'a Arc<Task>,
    token: &WakeToken,
    mode: WakeMode,
) -> Option<RequeuePermit<'a>> {
    let guard = task.sched_state_guard();
    match &*guard {
        TaskSchedState::Waiting {
            state,
            interruptible,
            ..
        } if Arc::ptr_eq(state, &token.state)
            && state.status() == WaitStateStatus::Armed
            && !mode.allows(*interruptible) =>
        {
            let wait_id = state.debug_id();
            kdebugln!(
                "wait_core: requeue permit task={} wait={:#x} mode={:?}",
                task.tid(),
                wait_id,
                mode,
            );
            Some(RequeuePermit {
                wait_id,
                _guard: guard,
                _not_send_sync: PhantomData,
            })
        },
        other => {
            kdebugln!(
                "wait_core: requeue permit denied task={} wait={:#x} mode={:?} state={:?} token_status={:?}",
                task.tid(),
                token.wait_id(),
                mode,
                other,
                token.state.status(),
            );
            None
        },
    }
}

fn finish_wake_attempt(
    task: &Arc<Task>,
    wait_id: Option<usize>,
    reason: WaitReason,
    mode: WakeMode,
    commit: WakeCommit,
) -> WakeResult {
    let result = match commit {
        WakeCommit::Woke { park } => {
            let placement = crate::sched::wake_enqueue(task.clone(), park);
            WakeResult::Woke { placement }
        },
        WakeCommit::ModeBlocked => WakeResult::ModeBlocked,
        WakeCommit::Stale => WakeResult::Stale,
        WakeCommit::AlreadyCompleted(reason) => WakeResult::AlreadyCompleted(reason),
        WakeCommit::AlreadyCancelled(reason) => WakeResult::AlreadyCancelled(reason),
        WakeCommit::Retired => WakeResult::Retired,
    };

    kdebugln!(
        "wait_core: wake task={} wait={:?} reason={:?} mode={:?} result={:?}",
        task.tid(),
        wait_id,
        reason,
        mode,
        result,
    );

    result
}
