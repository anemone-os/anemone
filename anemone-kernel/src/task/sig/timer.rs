//! Signal-owned POSIX timer notification registration and delivery handoff.
//!
//! A registration reserves one slot in the target thread group's shared
//! pending owner. Timer expiry fills that slot without allocating. Signal owns
//! admission, masking, target notification, pending selection, and siginfo
//! serialization; the future POSIX timer owner receives only typed enqueue
//! outcomes and a dequeue callback carrying immutable timer identity.

use core::fmt::{Debug, Formatter};

use crate::{
    prelude::*,
    task::{Task, ThreadGroup, ThreadGroupLifeCycle},
};

use super::{
    SigNo, Signal,
    info::{SiCode, SigInfoFields, SigTimer},
    pending::TimerSignalSlotId,
};

/// Identity preserved from one timer expiry episode until signal dequeue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PosixTimerSignalIdentity {
    timer_id: i32,
    generation: u64,
    episode: u64,
}

impl PosixTimerSignalIdentity {
    pub(crate) const fn new(timer_id: i32, generation: u64, episode: u64) -> Self {
        Self {
            timer_id,
            generation,
            episode,
        }
    }

    pub(crate) const fn timer_id(self) -> i32 {
        self.timer_id
    }

    pub(crate) const fn generation(self) -> u64 {
        self.generation
    }

    pub(crate) const fn episode(self) -> u64 {
        self.episode
    }
}

/// Result of attempting to publish one timer notification episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PosixTimerSignalEnqueue {
    Queued,
    AlreadyPending,
    Ignored,
    /// The signal owner applied an uncatchable control occurrence without
    /// publishing a userspace-deliverable pending item.
    Consumed,
    TargetExited,
}

/// Callback allocated with the timer object, never on its expiry path.
pub(crate) type PosixTimerSignalCallback = dyn Fn(PosixTimerSignalIdentity) + Send + Sync + 'static;

/// Signal-private handoff consumed exactly once after pending dequeue.
pub(super) struct TimerSignalDelivery {
    identity: PosixTimerSignalIdentity,
    callback: Arc<PosixTimerSignalCallback>,
    target: Weak<ThreadGroup>,
    slot: TimerSignalSlotId,
}

impl TimerSignalDelivery {
    pub(super) fn new(
        identity: PosixTimerSignalIdentity,
        callback: Arc<PosixTimerSignalCallback>,
        target: Weak<ThreadGroup>,
        slot: TimerSignalSlotId,
    ) -> Self {
        Self {
            identity,
            callback,
            target,
            slot,
        }
    }

    fn complete(self) {
        if let Some(target) = self.target.upgrade() {
            target.finish_timer_signal_slot(self.slot);
        }
        (self.callback)(self.identity);
    }
}

impl Debug for TimerSignalDelivery {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TimerSignalDelivery")
            .field("identity", &self.identity)
            .field("slot", &self.slot)
            .finish_non_exhaustive()
    }
}

/// Preallocated capability for one future ThreadGroup-owned POSIX timer.
///
/// Dropping the registration prevents future enqueue attempts but does not
/// revoke a notification already owned by Signal. Such an occurrence retains
/// its immutable siginfo and weak-owner callback until normal delivery or
/// signal-owned flush.
pub(crate) struct PosixTimerSignalRegistration {
    target: Weak<ThreadGroup>,
    slot: TimerSignalSlotId,
}

impl PosixTimerSignalRegistration {
    pub(crate) fn try_new(
        target: &Arc<ThreadGroup>,
        no: SigNo,
        timer_id: i32,
        sigval: u64,
        callback: Arc<PosixTimerSignalCallback>,
    ) -> Result<Self, SysError> {
        let slot = {
            let inner = target.inner.read();
            if !matches!(inner.status.life_cycle(), ThreadGroupLifeCycle::Alive)
                || inner.members.is_empty()
            {
                return Err(SysError::NoSuchProcess);
            }
            inner.sig_pending.lock().try_register_timer_signal(
                Arc::downgrade(target),
                no,
                timer_id,
                sigval,
                callback,
            )?
        };
        Ok(Self {
            target: Arc::downgrade(target),
            slot,
        })
    }

    /// Publish or update one expiry episode without exposing pending internals.
    pub(crate) fn enqueue(
        &self,
        generation: u64,
        episode: u64,
        overrun: i32,
    ) -> PosixTimerSignalEnqueue {
        let Some(target) = self.target.upgrade() else {
            return PosixTimerSignalEnqueue::TargetExited;
        };

        let no = {
            let inner = target.inner.read();
            inner.sig_pending.lock().timer_signal_no(self.slot)
        };
        if super::generation::is_job_control_signal(no) {
            return target
                .enqueue_timer_job_control_signal(self.slot, generation, episode, overrun);
        }

        // Snapshot Arc targets before entering the signal leaf. The snapshot is
        // revalidated against ThreadGroup membership below and used for wakeups
        // only after all owner locks are released.
        let mut members = target.get_members();
        let (outcome, no) = {
            let inner = target.inner.read();
            if !matches!(inner.status.life_cycle(), ThreadGroupLifeCycle::Alive) {
                return PosixTimerSignalEnqueue::TargetExited;
            }
            members.retain(|member| inner.members.contains(&member.tid()));
            let Some(disposition_owner) = members.first() else {
                return PosixTimerSignalEnqueue::TargetExited;
            };

            // Preserve the established signal-leaf order: pending precedes the
            // shared disposition. This makes ignored admission atomic with a
            // concurrent SIG_IGN update followed by pending flush.
            let mut pending = inner.sig_pending.lock();
            let no = pending.timer_signal_no(self.slot);
            let ignored = disposition_owner
                .sig_disposition
                .read()
                .get_disposition(no)
                .action
                .is_ignored();
            let outcome =
                pending.enqueue_timer_signal(self.slot, generation, episode, overrun, ignored);
            (outcome, no)
        };

        if matches!(outcome, PosixTimerSignalEnqueue::Queued) {
            for member in members {
                if no == SigNo::SIGKILL || !member.is_current_sig_mask_blocking(no) {
                    notify(&member, no == SigNo::SIGKILL);
                }
            }
        }
        outcome
    }
}

impl Debug for PosixTimerSignalRegistration {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PosixTimerSignalRegistration")
            .field("slot", &self.slot)
            .finish_non_exhaustive()
    }
}

impl Drop for PosixTimerSignalRegistration {
    fn drop(&mut self) {
        let Some(target) = self.target.upgrade() else {
            return;
        };
        // Registration metadata may own the last callback Arc. Move it out of
        // the signal lock so captured weak owner state is also destroyed
        // unlocked. A pending occurrence keeps its own callback Arc.
        let registration = {
            let inner = target.inner.read();
            inner.sig_pending.lock().unregister_timer_signal(self.slot)
        };
        drop(registration);
    }
}

impl ThreadGroup {
    fn finish_timer_signal_slot(&self, slot: TimerSignalSlotId) {
        let inner = self.inner.read();
        inner.sig_pending.lock().finish_timer_signal(slot);
    }

    /// Complete every notification retired by a signal-owned flush.
    ///
    /// Each iteration extracts one immutable occurrence under the shared
    /// pending lock, releases it, then calls the future timer owner. This keeps
    /// the lock direction from becoming Signal -> POSIX timer object.
    pub(super) fn finish_retired_timer_signal_handoffs(&self) {
        loop {
            let signal = {
                let inner = self.inner.read();
                inner.sig_pending.lock().take_retired_timer_signal()
            };
            let Some(mut signal) = signal else {
                break;
            };
            signal.finish_timer_signal_handoff();
        }
    }
}

impl Task {
    /// Close a task-private timer delivery reservation before task teardown.
    ///
    /// The signal is detached while holding only the private pending lock; slot
    /// retirement and the timer-owner callback run after that guard is gone.
    pub(in crate::task) fn finish_reserved_timer_signal_handoff_for_exit(&self) {
        let signal = self
            .sig_pending
            .lock()
            .take_reserved_timer_signal_for_exit();
        if let Some(mut signal) = signal {
            signal.finish_timer_signal_handoff();
        }
    }
}

impl Signal {
    pub(super) fn new_posix_timer(
        no: SigNo,
        timer_id: i32,
        overrun: i32,
        sigval: u64,
        identity: PosixTimerSignalIdentity,
        callback: Arc<PosixTimerSignalCallback>,
        target: Weak<ThreadGroup>,
        slot: TimerSignalSlotId,
    ) -> Self {
        Self {
            no,
            errno: 0,
            code: SiCode::Timer,
            fields: SigInfoFields::Timer(SigTimer {
                tid: timer_id,
                overrun,
                sigval,
                sys_private: 0,
            }),
            default_stop_epoch: None,
            purpose: super::SignalPurpose::Ordinary,
            timer_delivery: Some(TimerSignalDelivery::new(identity, callback, target, slot)),
        }
    }

    pub(super) fn timer_signal_identity(&self) -> Option<PosixTimerSignalIdentity> {
        self.timer_delivery
            .as_ref()
            .map(|delivery| delivery.identity)
    }

    pub(super) fn update_timer_signal(&mut self, identity: PosixTimerSignalIdentity, overrun: i32) {
        let delivery = self
            .timer_delivery
            .as_mut()
            .expect("SI_TIMER signal lacks delivery identity");
        assert_eq!(
            delivery.identity.timer_id(),
            identity.timer_id(),
            "one POSIX timer signal slot changed timer identity"
        );
        delivery.identity = identity;
        let SigInfoFields::Timer(fields) = &mut self.fields else {
            panic!("POSIX timer delivery has non-timer siginfo fields")
        };
        fields.overrun = overrun;
    }

    pub(super) fn finish_timer_signal_handoff(&mut self) {
        if let Some(delivery) = self.timer_delivery.take() {
            delivery.complete();
        }
    }
}
