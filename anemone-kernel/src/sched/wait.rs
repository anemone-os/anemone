//! Scheduler wait-core skeleton.
//!
//! Phase 1 of RFC-20260601-sched-wait-refactor establishes the shared types
//! and the task scheduling-state container.  Production Event, timeout, and
//! signal wait paths still use the legacy status compatibility entry point
//! until stale-safe wake placement and the park latch land in later phases.

use core::fmt::{Debug, Formatter};

use crate::prelude::*;

/// Internal scheduler state for a task.
///
/// `LegacyWaiting` is the migration compatibility state written by
/// `Task::update_status_with()`.  New wait-core code must use `Waiting`, which
/// carries a stable `WaitState` identity for one wait round.
#[derive(Clone, Debug)]
pub enum TaskSchedState {
    Runnable,
    Waiting {
        state: Arc<WaitState>,
        interruptible: bool,
        park: ParkState,
    },
    LegacyWaiting {
        interruptible: bool,
    },
    Zombie,
}

impl TaskSchedState {
    pub fn as_task_status(&self) -> TaskStatus {
        match self {
            Self::Runnable => TaskStatus::Runnable,
            Self::Waiting { interruptible, .. } | Self::LegacyWaiting { interruptible } => {
                TaskStatus::Waiting {
                    interruptible: *interruptible,
                }
            },
            Self::Zombie => TaskStatus::Zombie,
        }
    }

    pub fn from_legacy_status(status: TaskStatus) -> Self {
        match status {
            TaskStatus::Runnable => Self::Runnable,
            TaskStatus::Zombie => Self::Zombie,
            TaskStatus::Waiting { interruptible } => Self::LegacyWaiting { interruptible },
        }
    }

    pub fn is_wait_core_waiting(&self) -> bool {
        matches!(self, Self::Waiting { .. })
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
pub enum WaitReason {
    Event,
    Timeout,
    Signal,
    Force,
    PredicateReady,
    Cancelled,
}

/// Wake mode requested by a producer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeMode {
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
pub enum WaitStateStatus {
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

    pub fn status(&self) -> WaitStateStatus {
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
pub struct WaitGuard {
    task: Arc<Task>,
    state: Arc<WaitState>,
}

impl WaitGuard {
    pub fn token(&self) -> WakeToken {
        WakeToken {
            state: self.state.clone(),
        }
    }

    pub fn wait_id(&self) -> usize {
        self.state.debug_id()
    }
}

/// Restricted wake capability held by event sources.
#[derive(Clone, Debug)]
pub struct WakeToken {
    state: Arc<WaitState>,
}

impl WakeToken {
    pub fn wait_id(&self) -> usize {
        self.state.debug_id()
    }

    pub fn same_wait(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }
}

#[derive(Debug)]
pub struct BeginWait {
    guard: WaitGuard,
    token: WakeToken,
}

impl BeginWait {
    pub fn token(&self) -> WakeToken {
        self.token.clone()
    }

    pub fn into_parts(self) -> (WaitGuard, WakeToken) {
        (self.guard, self.token)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitResult {
    Cancelled(WaitReason),
    Completed(WaitReason),
    Retired,
    Stale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitOutcome {
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

/// Result for wake attempts through the new wait core.
///
/// Phase 1 intentionally never reports `Woke`: the stale-safe physical
/// placement entry point does not exist yet, so exposing a logical wake success
/// would create a half-protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeResult {
    DisabledUntilWakePlacement,
}

/// Start one wait-core round for `task`.
///
/// The linearization point is the task scheduling-state transaction.  This API
/// is not wired into production wait sources in phase 1.
pub fn begin_wait(task: &Arc<Task>, interruptible: bool) -> BeginWait {
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
pub fn cancel_wait(guard: &WaitGuard, reason: WaitReason) -> WaitResult {
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
        _ => (prev, WaitResult::Stale),
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
pub fn finish_wait(guard: WaitGuard) -> WaitOutcome {
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

/// Wake a wait round through a source-held token.
///
/// This is a disabled phase-1 skeleton.  It must not be connected to Event,
/// timeout, or signal paths until phase 2 adds stale-safe wake placement.
pub(crate) fn wake_wait(
    task: &Arc<Task>,
    token: &WakeToken,
    reason: WaitReason,
    mode: WakeMode,
) -> WakeResult {
    kdebugln!(
        "wait_core: wake_wait disabled task={} wait={:#x} reason={:?} mode={:?}",
        task.tid(),
        token.wait_id(),
        reason,
        mode,
    );
    let _ = mode.allows(true);
    WakeResult::DisabledUntilWakePlacement
}

/// Wake the currently active wait without an external token.
///
/// This helper is scheduler-internal and disabled until phase 2 closes the
/// post-commit placement semantics.
pub(crate) fn wake_active_wait(task: &Arc<Task>, reason: WaitReason, mode: WakeMode) -> WakeResult {
    kdebugln!(
        "wait_core: wake_active_wait disabled task={} reason={:?} mode={:?}",
        task.tid(),
        reason,
        mode,
    );
    let _ = mode.allows(true);
    WakeResult::DisabledUntilWakePlacement
}
