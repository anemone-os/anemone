//! Signal-owned POSIX timer notification registration and delivery handoff.
//!
//! A registration reserves one slot in either the target thread group's shared
//! pending owner or an exact task's private pending owner. Timer expiry fills
//! that slot without allocating. Signal owns admission, masking, target
//! notification, pending selection, and siginfo serialization; the POSIX timer
//! owner receives only typed enqueue outcomes and an unlocked completion
//! callback carrying immutable timer identity.

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

/// Why Signal stopped owning one published timer occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PosixTimerSignalCompletion {
    /// Ordinary trap-return or synchronous wait claimed the occurrence.
    Dequeued,
    /// Disposition cleanup or task exit removed it before delivery.
    Flushed,
}

/// Callback allocated with the timer object, never on its expiry path.
///
/// A dequeued live periodic timer returns its finalized overrun. Signal keeps
/// ownership of the siginfo representation and applies that narrow result
/// before frame or synchronous-wait copyout.
pub(crate) type PosixTimerSignalCallback = dyn Fn(PosixTimerSignalIdentity, PosixTimerSignalCompletion) -> Option<i32>
    + Send
    + Sync
    + 'static;

/// Pending owner selected when the registration is created.
///
/// Both variants are weak so a timer registration cannot extend the target's
/// executable or membership lifetime. In particular, private expiry never
/// resolves the numeric TID again and therefore cannot bind a reused identity.
#[derive(Clone)]
pub(super) enum TimerSignalPendingOwner {
    Shared(Weak<ThreadGroup>),
    Private(Weak<Task>),
}

impl TimerSignalPendingOwner {
    fn finish_slot(&self, slot: TimerSignalSlotId) {
        match self {
            Self::Shared(target) => {
                if let Some(target) = target.upgrade() {
                    target.finish_timer_signal_slot(slot);
                }
            },
            Self::Private(target) => {
                if let Some(target) = target.upgrade() {
                    target.finish_timer_signal_slot(slot);
                }
            },
        }
    }
}

impl Debug for TimerSignalPendingOwner {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Shared(_) => f.write_str("Shared"),
            Self::Private(_) => f.write_str("Private"),
        }
    }
}

/// Signal-private handoff consumed exactly once after pending dequeue.
pub(super) struct TimerSignalDelivery {
    identity: PosixTimerSignalIdentity,
    callback: Arc<PosixTimerSignalCallback>,
    owner: TimerSignalPendingOwner,
    slot: TimerSignalSlotId,
}

impl TimerSignalDelivery {
    pub(super) fn new(
        identity: PosixTimerSignalIdentity,
        callback: Arc<PosixTimerSignalCallback>,
        owner: TimerSignalPendingOwner,
        slot: TimerSignalSlotId,
    ) -> Self {
        Self {
            identity,
            callback,
            owner,
            slot,
        }
    }

    fn complete(self, reason: PosixTimerSignalCompletion) -> Option<i32> {
        self.owner.finish_slot(self.slot);
        (self.callback)(self.identity, reason)
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
    owner: TimerSignalPendingOwner,
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
                TimerSignalPendingOwner::Shared(Arc::downgrade(target)),
                no,
                timer_id,
                sigval,
                callback,
            )?
        };
        Ok(Self {
            owner: TimerSignalPendingOwner::Shared(Arc::downgrade(target)),
            slot,
        })
    }

    /// Reserve a task-private slot while the target still admits registrations.
    ///
    /// The syscall boundary resolves and validates the same-thread-group TID;
    /// this registration retains only the non-rebinding exact-task capability.
    pub(crate) fn try_new_private(
        target: &Arc<Task>,
        no: SigNo,
        timer_id: i32,
        sigval: u64,
        callback: Arc<PosixTimerSignalCallback>,
    ) -> Result<Self, SysError> {
        let owner = TimerSignalPendingOwner::Private(Arc::downgrade(target));
        let slot = target.sig_pending.lock().try_register_timer_signal(
            owner.clone(),
            no,
            timer_id,
            sigval,
            callback,
        )?;
        Ok(Self { owner, slot })
    }

    /// Publish or update one expiry episode without exposing pending internals.
    pub(crate) fn enqueue(
        &self,
        generation: u64,
        episode: u64,
        overrun: i32,
    ) -> PosixTimerSignalEnqueue {
        match &self.owner {
            TimerSignalPendingOwner::Shared(target) => {
                let Some(target) = target.upgrade() else {
                    return PosixTimerSignalEnqueue::TargetExited;
                };
                self.enqueue_shared(&target, generation, episode, overrun)
            },
            TimerSignalPendingOwner::Private(target) => {
                let Some(target) = target.upgrade() else {
                    return PosixTimerSignalEnqueue::TargetExited;
                };
                self.enqueue_private(&target, generation, episode, overrun)
            },
        }
    }

    fn enqueue_shared(
        &self,
        target: &Arc<ThreadGroup>,
        generation: u64,
        episode: u64,
        overrun: i32,
    ) -> PosixTimerSignalEnqueue {
        let no = {
            let inner = target.inner.read();
            inner.sig_pending.lock().timer_signal_no(self.slot)
        };
        if super::generation::is_job_control_signal(no) {
            return target
                .enqueue_timer_job_control_signal(self.slot, no, generation, episode, overrun);
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

    fn enqueue_private(
        &self,
        target: &Arc<Task>,
        generation: u64,
        episode: u64,
        overrun: i32,
    ) -> PosixTimerSignalEnqueue {
        let no = {
            let pending = target.sig_pending.lock();
            let Some(no) = pending.admitted_timer_signal_no(self.slot) else {
                return PosixTimerSignalEnqueue::TargetExited;
            };
            no
        };
        if super::generation::is_job_control_signal(no) {
            return target.enqueue_private_timer_job_control_signal(
                self.slot, no, generation, episode, overrun,
            );
        }

        let (outcome, no) = {
            // Admission closure and expiry publication share this lock. Once
            // exit closes it, no retained Weak<Task> can publish a late signal.
            let mut pending = target.sig_pending.lock();
            let Some(no) = pending.admitted_timer_signal_no(self.slot) else {
                return PosixTimerSignalEnqueue::TargetExited;
            };
            let ignored = target
                .sig_disposition
                .read()
                .get_disposition(no)
                .action
                .is_ignored();
            let outcome =
                pending.enqueue_timer_signal(self.slot, generation, episode, overrun, ignored);
            (outcome, no)
        };

        if matches!(outcome, PosixTimerSignalEnqueue::Queued)
            && (no == SigNo::SIGKILL || !target.is_current_sig_mask_blocking(no))
        {
            notify(target, no == SigNo::SIGKILL);
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
        // Registration metadata may own the last callback Arc. Move it out of
        // the signal lock so captured weak owner state is also destroyed
        // unlocked. A pending occurrence keeps its own callback Arc.
        let registration = match &self.owner {
            TimerSignalPendingOwner::Shared(target) => {
                let Some(target) = target.upgrade() else {
                    return;
                };
                let inner = target.inner.read();
                inner.sig_pending.lock().unregister_timer_signal(self.slot)
            },
            TimerSignalPendingOwner::Private(target) => {
                let Some(target) = target.upgrade() else {
                    return;
                };
                target.sig_pending.lock().unregister_timer_signal(self.slot)
            },
        };
        drop(registration);
    }
}

impl ThreadGroup {
    fn finish_timer_signal_slot(&self, slot: TimerSignalSlotId) {
        let inner = self.inner.read();
        inner.sig_pending.lock().finish_timer_signal(slot);
    }
}

impl Task {
    fn finish_timer_signal_slot(&self, slot: TimerSignalSlotId) {
        self.sig_pending.lock().finish_timer_signal(slot);
    }

    /// Close private timer admission and retire all registrations/occurrences.
    ///
    /// Exit performs this before topology detach. The first lock acquisition is
    /// the admission linearization point; every callback and registration drop
    /// below runs after the private pending guard has been released.
    pub(in crate::task) fn retire_private_timer_signals_for_exit(&self, tg: &ThreadGroup) {
        assert_eq!(
            self.tgid(),
            tg.tgid(),
            "timer signal exit cleanup used a foreign ThreadGroup"
        );
        let signals = {
            // Timer job-control admission holds the ThreadGroup write guard.
            // Taking its read side here prevents exit from closing the private
            // owner halfway through one generation transaction. This guard is
            // deliberately released before any callback or destructor below.
            let _inner = tg.inner.read();
            self.sig_pending.lock().close_timer_admission_for_exit()
        };
        for mut signal in signals {
            signal.finish_timer_signal_handoff(PosixTimerSignalCompletion::Flushed);
        }
        loop {
            let registration = self
                .sig_pending
                .lock()
                .take_exiting_timer_signal_registration();
            let Some(registration) = registration else {
                break;
            };
            drop(registration);
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
        owner: TimerSignalPendingOwner,
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
            timer_delivery: Some(TimerSignalDelivery::new(identity, callback, owner, slot)),
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

    pub(super) fn finish_timer_signal_handoff(&mut self, reason: PosixTimerSignalCompletion) {
        if let Some(delivery) = self.timer_delivery.take() {
            let Some(overrun) = delivery.complete(reason) else {
                return;
            };
            assert_eq!(
                reason,
                PosixTimerSignalCompletion::Dequeued,
                "only a dequeued POSIX timer occurrence can finalize overrun"
            );
            let SigInfoFields::Timer(fields) = &mut self.fields else {
                panic!("POSIX timer completion lost SI_TIMER fields");
            };
            fields.overrun = overrun;
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use core::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::{
        sched::class::SchedEntity,
        task::sig::{
            disposition::{KSigAction, SaFlags, SignalAction},
            info::{SiCode, SigInfoFields, SigKill},
            set::SigSet,
        },
    };

    fn callback_log() -> (
        Arc<SpinLock<Vec<(PosixTimerSignalIdentity, PosixTimerSignalCompletion)>>>,
        Arc<PosixTimerSignalCallback>,
    ) {
        let log = Arc::new(SpinLock::new(Vec::new()));
        let callback_log = log.clone();
        let callback: Arc<PosixTimerSignalCallback> = Arc::new(move |identity, reason| {
            callback_log.lock().push((identity, reason));
            None
        });
        (log, callback)
    }

    fn detached_target(tgid: Tid) -> Arc<Task> {
        fn unused_entry() {}

        let (task, guard) = unsafe {
            Task::new_kernel(
                "kunit-private-timer-target",
                unused_entry as *const (),
                ParameterList::empty(),
                None,
                Some(tgid),
                SchedEntity::new_default(),
                TaskFlags::empty(),
                Some(cur_cpu_id()),
            )
        }
        .expect("failed to construct private timer KUnit target");
        unsafe {
            guard.forget();
        }
        task.detach_files_for_exit();
        Arc::new(task)
    }

    #[kunit]
    fn private_delivery_callback_reenters_after_pending_unlock() {
        let target = get_current_task();
        let callback_target = Arc::downgrade(&target);
        let callbacks = Arc::new(AtomicUsize::new(0));
        let callback_count = callbacks.clone();
        let callback: Arc<PosixTimerSignalCallback> = Arc::new(move |identity, reason| {
            assert_eq!(identity, PosixTimerSignalIdentity::new(91, 3, 4));
            assert_eq!(reason, PosixTimerSignalCompletion::Dequeued);
            let target = callback_target
                .upgrade()
                .expect("private timer callback lost its live test target");
            // Re-entering the same private owner proves completion did not run
            // under the pending guard that detached this occurrence.
            assert!(!target.pending_signal_set().get(SigNo::SIGUSR2));
            callback_count.fetch_add(1, Ordering::SeqCst);
            None
        });
        let registration = PosixTimerSignalRegistration::try_new_private(
            &target,
            SigNo::SIGUSR2,
            91,
            0x91,
            callback,
        )
        .unwrap();

        assert_eq!(
            registration.enqueue(3, 4, 0),
            PosixTimerSignalEnqueue::Queued
        );
        let signal = target
            .fetch_specific_signal(SigSet::new_with_signos(&[SigNo::SIGUSR2]))
            .expect("private timer signal was not published to the exact task");
        assert_eq!(signal.no, SigNo::SIGUSR2);
        assert_eq!(callbacks.load(Ordering::SeqCst), 1);
        drop(registration);
    }

    #[kunit]
    fn private_sigcont_timer_flushes_on_stop_generation_and_requeues() {
        let target = get_current_task();
        let tg = target.get_thread_group();
        let old_mask = target.snapshot_current_sig_mask();
        let mut blocked = old_mask;
        blocked.set(SigNo::SIGCONT);
        blocked.set(SigNo::SIGTSTP);
        target.set_permanent_sig_mask(blocked);

        let (log, callback) = callback_log();
        let registration =
            PosixTimerSignalRegistration::try_new_private(&target, SigNo::SIGCONT, 95, 0, callback)
                .unwrap();
        assert_eq!(
            registration.enqueue(1, 1, 0),
            PosixTimerSignalEnqueue::Queued
        );

        // Stop-class generation removes the opposite SIGCONT class under the
        // ThreadGroup transaction. The old timer occurrence must complete as a
        // flush before this same registration admits a new episode.
        target.recv_signal(Signal::new(
            SigNo::SIGTSTP,
            SiCode::User,
            SigInfoFields::Kill(SigKill {
                pid: target.tid(),
                uid: Uid::new(0),
            }),
        ));
        assert_eq!(
            log.lock().as_slice(),
            &[(
                PosixTimerSignalIdentity::new(95, 1, 1),
                PosixTimerSignalCompletion::Flushed,
            )]
        );
        tg.flush_specific_signals(SigSet::new_with_signos(&[SigNo::SIGTSTP]));

        assert_eq!(
            registration.enqueue(2, 2, 0),
            PosixTimerSignalEnqueue::Queued
        );
        let signal = target
            .fetch_specific_signal(SigSet::new_with_signos(&[SigNo::SIGCONT]))
            .expect("replacement private SIGCONT timer occurrence was not fetchable");
        assert_eq!(signal.no, SigNo::SIGCONT);
        assert_eq!(
            log.lock().as_slice(),
            &[
                (
                    PosixTimerSignalIdentity::new(95, 1, 1),
                    PosixTimerSignalCompletion::Flushed,
                ),
                (
                    PosixTimerSignalIdentity::new(95, 2, 2),
                    PosixTimerSignalCompletion::Dequeued,
                ),
            ]
        );

        drop(registration);
        target.set_permanent_sig_mask(old_mask);
    }

    #[kunit]
    fn private_timer_reuses_registration_after_live_ignore() {
        let owner = get_current_task().get_thread_group();
        let target = detached_target(owner.tgid());
        target.set_permanent_sig_mask(SigSet::new_with_signos(&[SigNo::SIGUSR2]));
        target.sig_disposition.write().set_disposition(
            SigNo::SIGUSR2,
            KSigAction {
                action: SignalAction::Ignore,
                flags: SaFlags::empty(),
                restorer: VirtAddr::new(0),
                mask: SigSet::new(),
            },
        );

        let (log, callback) = callback_log();
        let registration =
            PosixTimerSignalRegistration::try_new_private(&target, SigNo::SIGUSR2, 96, 0, callback)
                .unwrap();
        assert_eq!(
            registration.enqueue(1, 1, 0),
            PosixTimerSignalEnqueue::Ignored
        );
        assert!(log.lock().is_empty());

        // Ignored is not a terminal registration state. Restoring a live
        // deliverable disposition must let the next expiry use the same slot.
        target
            .sig_disposition
            .write()
            .set_to_default(SigNo::SIGUSR2);
        assert_eq!(
            registration.enqueue(2, 2, 0),
            PosixTimerSignalEnqueue::Queued
        );
        let signal = target
            .fetch_specific_signal(SigSet::new_with_signos(&[SigNo::SIGUSR2]))
            .expect("private timer did not queue after disposition recovery");
        assert_eq!(signal.no, SigNo::SIGUSR2);
        assert_eq!(
            log.lock().as_slice(),
            &[(
                PosixTimerSignalIdentity::new(96, 2, 2),
                PosixTimerSignalCompletion::Dequeued,
            )]
        );
        drop(registration);
    }

    #[kunit]
    fn private_target_exit_closes_all_three_occurrence_stages() {
        let owner = get_current_task().get_thread_group();

        // Expiry after admission closure observes the original identity as
        // exited and cannot publish or bind a later task with the same TID.
        let before_expiry = detached_target(owner.tgid());
        before_expiry.set_permanent_sig_mask(SigSet::new_with_signos(&[SigNo::SIGUSR1]));
        let callback: Arc<PosixTimerSignalCallback> =
            Arc::new(|_, _| panic!("pre-expiry target exit must not complete a queued occurrence"));
        let registration = PosixTimerSignalRegistration::try_new_private(
            &before_expiry,
            SigNo::SIGUSR1,
            101,
            0,
            callback,
        )
        .unwrap();
        before_expiry.retire_private_timer_signals_for_exit(&owner);
        assert_eq!(
            registration.enqueue(1, 1, 0),
            PosixTimerSignalEnqueue::TargetExited
        );
        assert!(matches!(
            PosixTimerSignalRegistration::try_new_private(
                &before_expiry,
                SigNo::SIGUSR1,
                102,
                0,
                Arc::new(|_, _| None),
            ),
            Err(SysError::NoSuchProcess)
        ));
        drop(registration);

        // A still-pending occurrence is detached and completed as Flushed;
        // dropping the external registration afterwards is an idempotent no-op.
        let pending_target = detached_target(owner.tgid());
        pending_target.set_permanent_sig_mask(SigSet::new_with_signos(&[SigNo::SIGUSR1]));
        let pending_log = Arc::new(SpinLock::new(Vec::new()));
        let callback_log = pending_log.clone();
        let registration = PosixTimerSignalRegistration::try_new_private(
            &pending_target,
            SigNo::SIGUSR1,
            103,
            0,
            Arc::new(move |identity, reason| {
                callback_log.lock().push((identity, reason));
                None
            }),
        )
        .unwrap();
        assert_eq!(
            registration.enqueue(2, 3, 0),
            PosixTimerSignalEnqueue::Queued
        );
        pending_target.retire_private_timer_signals_for_exit(&owner);
        assert_eq!(
            pending_log.lock().as_slice(),
            &[(
                PosixTimerSignalIdentity::new(103, 2, 3),
                PosixTimerSignalCompletion::Flushed,
            )]
        );
        drop(registration);

        // Once a consumer has dequeued an occurrence, exit only withdraws the
        // registration. Its already-owned completion remains Dequeued.
        let dequeued_target = detached_target(owner.tgid());
        dequeued_target.set_permanent_sig_mask(SigSet::new_with_signos(&[SigNo::SIGUSR1]));
        let dequeued_log = Arc::new(SpinLock::new(Vec::new()));
        let callback_log = dequeued_log.clone();
        let registration = PosixTimerSignalRegistration::try_new_private(
            &dequeued_target,
            SigNo::SIGUSR1,
            104,
            0,
            Arc::new(move |identity, reason| {
                callback_log.lock().push((identity, reason));
                None
            }),
        )
        .unwrap();
        assert_eq!(
            registration.enqueue(4, 5, 0),
            PosixTimerSignalEnqueue::Queued
        );
        let mut signal = dequeued_target
            .sig_pending
            .lock()
            .fetch_specific(SigSet::new_with_signos(&[SigNo::SIGUSR1]))
            .expect("dequeue-stage private timer occurrence was not published");
        dequeued_target.retire_private_timer_signals_for_exit(&owner);
        signal.finish_timer_signal_handoff(PosixTimerSignalCompletion::Dequeued);
        assert_eq!(
            dequeued_log.lock().as_slice(),
            &[(
                PosixTimerSignalIdentity::new(104, 4, 5),
                PosixTimerSignalCompletion::Dequeued,
            )]
        );
        drop(registration);
    }
}
