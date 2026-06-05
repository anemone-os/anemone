//! Single-round OR wait latch built on the scheduler wait core.
//!
//! `Latch` is the waiter-owned lifecycle guard for one current-task wait round.
//! Producers only receive cloneable `LatchTrigger`s, which hide the wait-core
//! token and expose a no-return fire-and-forget trigger operation.
//!
//! This module intentionally does not implement a reusable event source or a
//! cross-round notification permit. The wait identity is the underlying
//! `WaitState` named by the wait-core token. Late triggers from an older round
//! must lose through wait-core stale/retired handling, not through source queue
//! cleanup happening to run in time.

use core::marker::PhantomData;

use crate::prelude::*;

use super::{
    higher_level::schedule_wait_with_timeout,
    wait::{self, WaitOutcome, WaitReason, WakeMode, WakeResult, WakeToken},
};

/// Waiter-owned guard for exactly one current-task latch round.
///
/// The consumer side is linear: it owns begin/cancel/schedule/finish and must
/// not be cloned or moved across task contexts. `_not_send_sync` makes this
/// type `!Send`/`!Sync`, while the runtime owner check keeps misuse visible in
/// release builds if a future API accidentally erases that type boundary.
pub struct Latch {
    task: Arc<Task>,
    // `None` means this round has been retired. Keeping the active wait in an
    // option makes double-finish and use-after-finish cheap invariants that can
    // be asserted in release builds.
    active_wait: Option<wait::ActiveWait>,
    owner: Tid,
    wait_id: usize,
    _not_send_sync: PhantomData<*mut ()>,
}

impl Latch {
    /// Begin one latch round for the current task.
    ///
    /// This is the only public constructor for the waiter side. It delegates
    /// wait identity and task state publication to wait core; `Latch` only
    /// narrows the lifecycle API for iomux-style OR waits.
    pub fn begin_current(interruptible: bool) -> Self {
        let task = get_current_task();
        let active_wait = wait::ActiveWait::begin(&task, interruptible);
        let token = active_wait.token();
        let wait_id = token.wait_id();
        let owner = task.tid();

        kdebugln!(
            "latch: begin task={} wait={:#x} interruptible={}",
            owner,
            wait_id,
            interruptible,
        );

        Self {
            task,
            active_wait: Some(active_wait),
            owner,
            wait_id,
            _not_send_sync: PhantomData,
        }
    }

    /// Derive a producer capability for this same wait round.
    ///
    /// Every trigger cloned from this latch carries the same wait-core token.
    /// Source code must store only `LatchTrigger`, never the raw token or the
    /// consumer-owned `Latch`.
    pub fn make_trigger(&self) -> LatchTrigger {
        self.assert_owner("make_trigger");
        if self.active_wait.is_none() {
            kwarningln!(
                "latch: make_trigger after finish task={} wait={:#x}",
                self.owner,
                self.wait_id,
            );
        }
        assert!(
            self.active_wait.is_some(),
            "latch make_trigger after finish"
        );
        let active_wait = self
            .active_wait
            .as_ref()
            .expect("latch active wait disappeared after make_trigger assert");
        let token = active_wait.token();

        kdebugln!(
            "latch: make_trigger task={} wait={:#x}",
            self.owner,
            self.wait_id,
        );

        LatchTrigger::new(&self.task, token)
    }

    /// Try to complete this round from the consumer side.
    ///
    /// Cancellation is an ordinary wait-core completion attempt. If a producer,
    /// timeout, signal, or force already won, wait core keeps that winning
    /// outcome and this call only observes the lost race.
    pub fn cancel(&self, reason: LatchCancelReason) {
        self.assert_owner("cancel");

        if self.active_wait.is_none() {
            kwarningln!(
                "latch: cancel after finish task={} wait={:#x} reason={:?}",
                self.owner,
                self.wait_id,
                reason,
            );
        }
        assert!(self.active_wait.is_some(), "latch cancel after finish");
        let active_wait = self
            .active_wait
            .as_ref()
            .expect("latch active wait disappeared after cancel assert");

        active_wait.cancel(reason.into());
        kdebugln!(
            "latch: cancel task={} wait={:#x} reason={:?}",
            self.owner,
            self.wait_id,
            reason,
        );
    }

    /// Park the owner task with an optional timeout competing on this round.
    ///
    /// Timeout uses the same wait identity as producer triggers. A returned
    /// wakeup is only a readiness hint; iomux callers must finish the latch and
    /// then re-scan their actual fd predicates before deciding a syscall
    /// result.
    pub fn schedule_with_timeout(&self, timeout: Option<Duration>) -> Duration {
        self.assert_owner("schedule_with_timeout");
        if self.active_wait.is_none() {
            kwarningln!(
                "latch: schedule after finish task={} wait={:#x}",
                self.owner,
                self.wait_id,
            );
        }
        assert!(self.active_wait.is_some(), "latch schedule after finish");
        let active_wait = self
            .active_wait
            .as_ref()
            .expect("latch active wait disappeared after schedule assert");
        let token = active_wait.token();
        schedule_wait_with_timeout(&self.task, token, timeout)
    }

    /// Retire the wait round and return the recorded winner.
    ///
    /// `finish(self)` consumes the consumer handle so the normal path has one
    /// explicit retirement point. Drop exists only as an assertion-backed
    /// safety net for missed finish paths.
    pub fn finish(mut self) -> LatchWaitOutcome {
        self.assert_owner("finish");
        let outcome = self.finish_inner("finish");
        LatchWaitOutcome::from(outcome)
    }

    pub fn wait_id(&self) -> usize {
        self.wait_id
    }

    pub fn owner(&self) -> Tid {
        self.owner
    }

    fn finish_inner(&mut self, op: &str) -> WaitOutcome {
        if self.active_wait.is_none() {
            kwarningln!(
                "latch: double finish task={} wait={:#x} op={}",
                self.owner,
                self.wait_id,
                op,
            );
        }
        assert!(self.active_wait.is_some(), "latch double finish");
        let active_wait = self
            .active_wait
            .take()
            .expect("latch active wait disappeared after finish assert");

        let outcome = active_wait.finish();
        kdebugln!(
            "latch: finish task={} wait={:#x} op={} outcome={:?}",
            self.owner,
            self.wait_id,
            op,
            outcome,
        );
        outcome
    }

    fn assert_owner(&self, op: &str) {
        let current = get_current_task();
        let is_owner = Arc::ptr_eq(&current, &self.task);
        if !is_owner {
            kwarningln!(
                "latch: owner check failed op={} owner={} current={} wait={:#x}",
                op,
                self.owner,
                current.tid(),
                self.wait_id,
            );
        }
        assert!(is_owner, "latch used from non-owner task");
    }
}

impl Drop for Latch {
    fn drop(&mut self) {
        if self.active_wait.is_none() {
            return;
        }

        // Missing an explicit finish is a kernel bug, but the wait round must
        // still be retired before the release-build assertion exposes it.
        kwarningln!(
            "latch: drop without finish task={} wait={:#x}",
            self.owner,
            self.wait_id,
        );

        if let Some(active_wait) = self.active_wait.as_ref() {
            active_wait.cancel(LatchCancelReason::Drop.into());
        }
        let outcome = self.finish_inner("drop");
        kdebugln!(
            "latch: drop retired task={} wait={:#x} outcome={:?}",
            self.owner,
            self.wait_id,
            outcome,
        );
        // Drop must retire the wait round before exposing the owner bug.
        assert!(false, "latch dropped without finish");
    }
}

/// Cloneable producer-side trigger for one latch wait round.
///
/// The current strategy is weak task + strong `WakeToken`: a source queue entry
/// cannot keep a task alive, but it can retain the retired wait state until the
/// source prunes or drops the entry. Correctness does not depend on cleanup;
/// Stage 2 must define queue hygiene and resource bounds before fd sources use
/// this in production.
#[derive(Clone)]
pub struct LatchTrigger {
    task: Weak<Task>,
    token: Option<WakeToken>,
    owner: Tid,
    wait_id: usize,
}

impl LatchTrigger {
    fn new(task: &Arc<Task>, token: WakeToken) -> Self {
        Self {
            task: Arc::downgrade(task),
            owner: task.tid(),
            wait_id: token.wait_id(),
            token: Some(token),
        }
    }

    fn retired(owner: Tid, wait_id: usize) -> Self {
        Self {
            task: Weak::new(),
            owner,
            wait_id,
            token: None,
        }
    }

    /// Fire this trigger as a readiness hint.
    ///
    /// The result is intentionally not returned. Producers may log whether the
    /// wake was stale, retired, or successful, but they must not branch into a
    /// second completion protocol or compensate with direct enqueue.
    pub fn trigger(&self) {
        kdebugln!(
            "latch: trigger attempt task={} wait={:#x}",
            self.owner,
            self.wait_id,
        );

        let Some(task) = self.task.upgrade() else {
            kdebugln!(
                "latch: trigger stale task={} wait={:#x} result=task_gone",
                self.owner,
                self.wait_id,
            );
            return;
        };

        let Some(token) = self.token.as_ref() else {
            kdebugln!(
                "latch: trigger retired task={} wait={:#x} result=no_token",
                self.owner,
                self.wait_id,
            );
            return;
        };

        let result = wait::wake_wait(&task, token, WaitReason::Latch, WakeMode::AnyWait);
        match result {
            WakeResult::Woke { placement } => {
                kdebugln!(
                    "latch: trigger woke task={} wait={:#x} placement={:?}",
                    self.owner,
                    self.wait_id,
                    placement,
                );
            },
            WakeResult::Stale => {
                kdebugln!(
                    "latch: trigger stale task={} wait={:#x}",
                    self.owner,
                    self.wait_id,
                );
            },
            WakeResult::Retired => {
                kdebugln!(
                    "latch: trigger retired task={} wait={:#x}",
                    self.owner,
                    self.wait_id,
                );
            },
            other => {
                kdebugln!(
                    "latch: trigger ignored task={} wait={:#x} result={:?}",
                    self.owner,
                    self.wait_id,
                    other,
                );
            },
        }
    }

    /// Debug identity of the wait round this trigger targets.
    ///
    /// This is for logs and source queue diagnostics only; it is not a
    /// correctness key and must not be used to match new rounds.
    pub fn wait_id(&self) -> usize {
        self.wait_id
    }

    /// Whether this trigger can be pruned from a source queue.
    ///
    /// This is a resource-hygiene hint only. Sources must not use it to decide
    /// readiness or to compensate for a failed wake; old trigger correctness is
    /// still owned by wait-core identity and retired-state checks.
    pub fn is_prunable(&self) -> bool {
        if self.task.upgrade().is_none() {
            return true;
        }
        match self.token.as_ref() {
            Some(token) => !token.is_armed(),
            None => true,
        }
    }
}

impl core::fmt::Debug for LatchTrigger {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LatchTrigger")
            .field("owner", &self.owner)
            .field("wait_id", &self.wait_id)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatchCancelReason {
    /// A registration or final scan observed a ready predicate before parking.
    PredicateReady,
    /// A source could not arm this round, so the syscall must not sleep on it.
    RegisterError,
    /// Nonblocking timeout path; no producer capability should decide this.
    TimeoutZero,
    /// Signal precheck before the latch enters schedule.
    SignalPrecheck,
    /// Generic syscall-side error after a latch was begun.
    SyscallError,
    /// Drop safety net for a missed explicit finish.
    Drop,
}

impl From<LatchCancelReason> for WaitReason {
    fn from(value: LatchCancelReason) -> Self {
        match value {
            LatchCancelReason::PredicateReady => Self::PredicateReady,
            LatchCancelReason::RegisterError
            | LatchCancelReason::SyscallError
            | LatchCancelReason::Drop => Self::Cancelled,
            LatchCancelReason::TimeoutZero => Self::Timeout,
            LatchCancelReason::SignalPrecheck => Self::Signal,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatchWaitOutcome {
    /// A producer trigger completed the round. Callers must still re-scan.
    Triggered,
    Timeout,
    Signal,
    Force,
    Cancelled,
    Unexpected,
}

impl From<WaitOutcome> for LatchWaitOutcome {
    fn from(value: WaitOutcome) -> Self {
        match value {
            WaitOutcome::Completed(WaitReason::Latch) => Self::Triggered,
            WaitOutcome::Completed(WaitReason::Timeout) => Self::Timeout,
            WaitOutcome::Completed(WaitReason::Signal) => Self::Signal,
            WaitOutcome::Completed(WaitReason::Force) => Self::Force,
            WaitOutcome::Cancelled(_) => Self::Cancelled,
            _ => Self::Unexpected,
        }
    }
}
