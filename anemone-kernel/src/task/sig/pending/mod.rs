use anemone_abi::process::linux::signal::*;

use crate::{
    prelude::*,
    task::{
        Task, ThreadGroup,
        sig::{SigNo, Signal, set::SigSet},
    },
};

pub(super) mod timer;

use timer::TimerPending;

#[derive(Debug)]
struct SequencedSignal {
    arrival: u64,
    signal: Signal,
}

/// Per task pending signals.
///
/// Ignored signals won't be recorded here. See [Task::recv_signal] and
/// [ThreadGroup::recv_signal] for details.
#[derive(Debug)]
pub struct PendingSignals {
    /// Stable handoff target for trap-return delivery.
    ///
    /// For a task-private signal, `classify_temporary_mask_wait()` moves the
    /// target out of the ordinary private pending set before allowing a
    /// temporary-mask caller to defer restore. For a shared thread-group
    /// signal, the classifier first claims it from the shared pending set,
    /// then moves it into this current-task private reservation. In both
    /// cases the signal is no longer eligible for ordinary private/shared
    /// pending competition and must be consumed first by `handle_signals()`
    /// through `Task::fetch_signal()`.
    reserved_delivery: Option<Signal>,
    /// Unreliable signals are not queued.
    ///
    /// Plus 1 for easy indexing, since signal numbers start from 1.
    unreliable: [Option<Signal>; NUNRELIABLESIG + 1],
    /// POSIX.1b realtime signals.
    realtime: [VecDeque<SequencedSignal>; NRTSIG],
    /// Signal-owned POSIX timer notification resources and admission state.
    timer: TimerPending,
    /// Shared arrival order for realtime occurrences and per-timer slots. It is
    /// pending-owner protocol state, not a timestamp.
    next_arrival: u64,
}

pub(super) struct FetchedSignal {
    pub(super) signal: Signal,
    pub(super) reserved: bool,
}

fn allocate_arrival(next_arrival: &mut u64) -> u64 {
    let arrival = *next_arrival;
    *next_arrival = next_arrival
        .checked_add(1)
        .expect("signal pending arrival identity exhausted");
    arrival
}

impl PendingSignals {
    pub fn new() -> Self {
        Self {
            reserved_delivery: None,
            unreliable: [const { None }; NUNRELIABLESIG + 1],
            realtime: [const { VecDeque::new() }; NRTSIG],
            timer: TimerPending::new(),
            next_arrival: 1,
        }
    }

    /// Push a signal to the pending signals.
    ///
    /// Panics if the sicode and fields of the signal are not consistent.
    pub fn push_signal(&mut self, signal: Signal) {
        debug_assert!(signal.fields.validate_with(signal.code));

        if let Some(rt_idx) = signal.no.realtime_index() {
            let arrival = allocate_arrival(&mut self.next_arrival);
            self.realtime[rt_idx].push_back(SequencedSignal { arrival, signal });
        } else {
            debug_assert!(signal.no.is_unreliable());
            let no = signal.no.as_usize();
            if signal.is_dethread_victim_kill()
                && self.unreliable[no]
                    .as_ref()
                    .is_some_and(|pending| !pending.is_dethread_victim_kill())
            {
                // The exec sibling-teardown occurrence is kernel-private and
                // must never coalesce away an external task-directed SIGKILL.
                return;
            }
            self.unreliable[no] = Some(signal);
        }
    }

    pub(super) fn take_ordinary_sigkill(&mut self) -> bool {
        let slot = &mut self.unreliable[SigNo::SIGKILL.as_usize()];
        if slot
            .as_ref()
            .is_some_and(|signal| !signal.is_dethread_victim_kill())
        {
            slot.take();
            true
        } else {
            false
        }
    }

    /// Convert the pending signals to a [SigSet]. Masked signals are also
    /// included.
    pub fn to_sigset(&self) -> SigSet {
        let mut set = SigSet::new();
        if let Some(signal) = &self.reserved_delivery {
            set.set(signal.no);
        }
        for (no, signal) in self.unreliable.iter().enumerate() {
            if signal.is_some() {
                set.set(SigNo::new(no));
            }
        }

        for (idx, queue) in self.realtime.iter().enumerate() {
            let rt_idx = SigNo::new(SIGRTMIN as usize + idx);
            if !queue.is_empty() {
                set.set(rt_idx);
            }
        }

        self.timer.append_pending_signos(&mut set);

        set
    }

    /// Fetch any pending signal that is not masked by the given [SigSet].
    ///
    /// This method has a well-defined order of fetching signals:
    /// - fatal signals (SIGKILL and SIGSTOP) first, in order of signal number.
    /// - then realtime signals, in order of signal number and arrival time.
    /// - finally rest unreliable signals, in order of signal number.
    ///
    /// TODO: fetch_any_with() for custom order.
    pub fn fetch_any(&mut self, mask: SigSet) -> Option<Signal> {
        self.fetch_matching(mask, |_, _| true)
            .map(|fetched| fetched.signal)
    }

    /// Claim the first signal permitted by the current user-entry phase.
    ///
    /// A disallowed reservation stays task-private and final. Unreserved
    /// occurrences stay in their original pending owner; this helper never
    /// dequeues and republishes a rejected candidate.
    pub(super) fn fetch_matching(
        &mut self,
        mask: SigSet,
        mut allowed: impl FnMut(&Signal, bool) -> bool,
    ) -> Option<FetchedSignal> {
        if self
            .reserved_delivery
            .as_ref()
            .is_some_and(|signal| allowed(signal, true))
        {
            return self.reserved_delivery.take().map(|signal| FetchedSignal {
                signal,
                reserved: true,
            });
        }

        debug_assert!(
            !mask.intersects_with(&SigSet::new_with_signos(&[SigNo::SIGKILL, SigNo::SIGSTOP])),
            "SIGKILL and SIGSTOP cannot be masked"
        );

        for no in [SigNo::SIGKILL, SigNo::SIGSTOP] {
            if self.unreliable[no.as_usize()]
                .as_ref()
                .is_some_and(|signal| allowed(signal, false))
            {
                return self.unreliable[no.as_usize()]
                    .take()
                    .map(|signal| FetchedSignal {
                        signal,
                        reserved: false,
                    });
            }
            if let Some((_, signal)) = self.timer.peek_earliest(no)
                && allowed(signal, false)
            {
                return Some(FetchedSignal {
                    signal: self
                        .timer
                        .take_earliest(no)
                        .expect("timer candidate disappeared"),
                    reserved: false,
                });
            }
        }

        for idx in 0..self.realtime.len() {
            let no = SigNo::new(SIGRTMIN as usize + idx);
            if mask.get(no) {
                continue;
            }
            let timer_candidate = self.timer.peek_earliest(no);
            let ordinary_arrival = self.realtime[idx].front().map(|signal| signal.arrival);
            let timer_arrival = timer_candidate.map(|(arrival, _)| arrival);
            let take_timer = match (ordinary_arrival, timer_arrival) {
                (None, None) => continue,
                (None, Some(_)) => true,
                (Some(_), None) => false,
                (Some(ordinary), Some(timer)) => timer < ordinary,
            };
            let candidate = if take_timer {
                timer_candidate
                    .map(|(_, signal)| signal)
                    .expect("timer candidate disappeared")
            } else {
                &self.realtime[idx]
                    .front()
                    .expect("realtime candidate disappeared")
                    .signal
            };
            if allowed(candidate, false) {
                let signal = if take_timer {
                    self.timer
                        .take_earliest(no)
                        .expect("timer candidate disappeared")
                } else {
                    self.realtime[idx]
                        .pop_front()
                        .expect("realtime candidate disappeared")
                        .signal
                };
                return Some(FetchedSignal {
                    signal,
                    reserved: false,
                });
            }
        }

        for no in 1..SIGRTMIN as usize {
            let no = SigNo::new(no);
            if mask.get(no) {
                continue;
            }
            if self.unreliable[no.as_usize()]
                .as_ref()
                .is_some_and(|signal| allowed(signal, false))
            {
                return self.unreliable[no.as_usize()]
                    .take()
                    .map(|signal| FetchedSignal {
                        signal,
                        reserved: false,
                    });
            }
            if let Some((_, signal)) = self.timer.peek_earliest(no)
                && allowed(signal, false)
            {
                return Some(FetchedSignal {
                    signal: self
                        .timer
                        .take_earliest(no)
                        .expect("timer candidate disappeared"),
                    reserved: false,
                });
            }
        }

        None
    }

    pub(super) fn fetch_unreserved_any(&mut self, mask: SigSet) -> Option<Signal> {
        debug_assert!(
            !mask.intersects_with(&SigSet::new_with_signos(&[SigNo::SIGKILL, SigNo::SIGSTOP])),
            "SIGKILL and SIGSTOP cannot be masked"
        );

        // fatal signals first.
        if let Some(kill) = self.unreliable[SigNo::SIGKILL.as_usize()].take() {
            return Some(kill);
        }
        if let Some(signal) = self.timer.take_earliest(SigNo::SIGKILL) {
            return Some(signal);
        }
        if let Some(stop) = self.unreliable[SigNo::SIGSTOP.as_usize()].take() {
            return Some(stop);
        }
        if let Some(signal) = self.timer.take_earliest(SigNo::SIGSTOP) {
            return Some(signal);
        }

        // Realtime signals first. Merge ordinary and POSIX timer occurrences by
        // the pending owner's arrival identity so source-specific storage does
        // not degrade per-signum FIFO.
        for idx in 0..self.realtime.len() {
            let no = SigNo::new(SIGRTMIN as usize + idx);
            if mask.get(no) {
                continue;
            }
            let timer_arrival = self.timer.peek_earliest(no).map(|(arrival, _)| arrival);
            let ordinary_arrival = self.realtime[idx].front().map(|signal| signal.arrival);
            match (ordinary_arrival, timer_arrival) {
                (None, None) => {},
                (None, Some(_)) => {
                    return self.timer.take_earliest(no);
                },
                (Some(_), None) => {
                    return self.realtime[idx].pop_front().map(|signal| signal.signal);
                },
                (Some(ordinary), Some(timer)) if timer < ordinary => {
                    return self.timer.take_earliest(no);
                },
                (Some(_), Some(_)) => {
                    return self.realtime[idx].pop_front().map(|signal| signal.signal);
                },
            }
        }

        // then rest unreliable signals. here SIGKILL and SIGSTOP are scanned again.
        // but it does not harm.
        for no in 1..SIGRTMIN as usize {
            let no = SigNo::new(no);
            if mask.get(no) {
                continue;
            }
            if let Some(signal) = self.unreliable[no.as_usize()].take() {
                self.unreliable[no.as_usize()] = None;
                return Some(signal);
            }
            if let Some(signal) = self.timer.take_earliest(no) {
                return Some(signal);
            }
        }

        None
    }

    /// Reserve one unmasked signal for this task's next trap-return delivery.
    pub(super) fn reserve_any_for_delivery(&mut self, mask: SigSet) -> bool {
        assert!(
            self.reserved_delivery.is_none(),
            "temporary signal delivery target is already reserved"
        );

        if let Some(signal) = self.fetch_unreserved_any(mask) {
            self.reserved_delivery = Some(signal);
            true
        } else {
            false
        }
    }

    pub(super) fn reserve_specific_for_delivery(&mut self, set: SigSet) -> bool {
        assert!(
            self.reserved_delivery.is_none(),
            "temporary signal delivery target is already reserved"
        );

        if let Some(signal) = self.fetch_specific(set) {
            self.reserved_delivery = Some(signal);
            true
        } else {
            false
        }
    }

    pub(super) fn reserve_delivery_target(&mut self, signal: Signal) -> SigNo {
        assert!(
            self.reserved_delivery.is_none(),
            "temporary signal delivery target is already reserved"
        );
        let no = signal.no;
        self.reserved_delivery = Some(signal);
        no
    }

    pub(super) fn reserved_delivery_signo(&self) -> Option<SigNo> {
        self.reserved_delivery.as_ref().map(|signal| signal.no)
    }

    /// Detach a timer occurrence that cannot reach trap-return delivery because
    /// its task is exiting. Ordinary reservations need no owner callback and
    /// remain part of task-local memory teardown.
    pub(super) fn take_reserved_timer_signal_for_exit(&mut self) -> Option<Signal> {
        if self
            .reserved_delivery
            .as_ref()
            .and_then(Signal::timer_signal_identity)
            .is_some()
        {
            self.reserved_delivery.take()
        } else {
            None
        }
    }

    /// Fetch any pending signal in the given set, and remove it from the
    /// pending signals.
    ///
    /// SIGKILL and SIGSTOP won't be fetched if they're not in the set.
    pub fn fetch_specific(&mut self, set: SigSet) -> Option<Signal> {
        // Realtime signals first, preserving FIFO across ordinary and timer
        // sources for each signal number.
        for idx in 0..self.realtime.len() {
            let no = SigNo::new(SIGRTMIN as usize + idx);
            if !set.get(no) {
                continue;
            }
            let timer_arrival = self.timer.peek_earliest(no).map(|(arrival, _)| arrival);
            let ordinary_arrival = self.realtime[idx].front().map(|signal| signal.arrival);
            match (ordinary_arrival, timer_arrival) {
                (None, None) => {},
                (None, Some(_)) => {
                    return self.timer.take_earliest(no);
                },
                (Some(_), None) => {
                    return self.realtime[idx].pop_front().map(|signal| signal.signal);
                },
                (Some(ordinary), Some(timer)) if timer < ordinary => {
                    return self.timer.take_earliest(no);
                },
                (Some(_), Some(_)) => {
                    return self.realtime[idx].pop_front().map(|signal| signal.signal);
                },
            }
        }

        // unreliable signals.
        for no in 1..SIGRTMIN as usize {
            let no = SigNo::new(no);
            if !set.get(no) {
                continue;
            }
            if let Some(signal) = self.unreliable[no.as_usize()].take() {
                self.unreliable[no.as_usize()] = None;
                return Some(signal);
            }
            if let Some(signal) = self.timer.take_earliest(no) {
                return Some(signal);
            }
        }

        None
    }

    pub fn has_unmasked(&self, mask: SigSet) -> bool {
        if self.reserved_delivery.is_some() {
            return true;
        }
        for no in 1..SIGRTMIN as usize {
            let no = SigNo::new(no);
            if self.unreliable[no.as_usize()].is_some() && !mask.get(no) {
                return true;
            }
        }
        for (idx, queue) in self.realtime.iter().enumerate() {
            let rt_idx = SigNo::new(SIGRTMIN as usize + idx);
            if !queue.is_empty() && !mask.get(rt_idx) {
                return true;
            }
        }
        if self.timer.has_unmasked(mask) {
            return true;
        }
        false
    }

    pub fn has_specific(&self, set: SigSet) -> bool {
        if let Some(signal) = &self.reserved_delivery {
            if set.get(signal.no) {
                return true;
            }
        }
        self.has_dequeueable_specific(set)
    }

    /// Match only occurrences available to ordinary synchronous dequeue.
    /// A temporary-mask reserved target remains exclusively owned by the
    /// trap-return delivery handoff even though `has_specific` exposes it to
    /// job-control/user-entry arbitration.
    pub(super) fn has_dequeueable_specific(&self, set: SigSet) -> bool {
        for no in 1..SIGRTMIN as usize {
            let no = SigNo::new(no);
            if self.unreliable[no.as_usize()].is_some() && set.get(no) {
                return true;
            }
        }
        for (idx, queue) in self.realtime.iter().enumerate() {
            let rt_idx = SigNo::new(SIGRTMIN as usize + idx);
            if !queue.is_empty() && set.get(rt_idx) {
                return true;
            }
        }
        if self.timer.has_specific(set) {
            return true;
        }
        false
    }

    /// Remove all pending signals in the given set from the pending signals.
    ///
    /// Mainly used when a disposition is set to ignore, to flush all pending
    /// signals that are now ignored.
    pub fn flush_specific(&mut self, set: SigSet) -> Vec<Signal> {
        for signo in set {
            if signo.is_realtime() {
                let idx = signo.realtime_index().unwrap();
                kdebugln!("flushing realtime signal {:?}", signo);
                self.realtime[idx].clear();
            } else {
                kdebugln!("flushing unreliable signal {:?}", signo);
                self.unreliable[signo.as_usize()] = None;
            }
        }
        self.timer.retire_matching(set)
    }
}

impl Task {
    /// Return a snapshot of this task's private pending signal set.
    pub fn pending_signal_set(&self) -> SigSet {
        self.sig_pending.lock().to_sigset()
    }
}

impl ThreadGroup {
    /// Return a snapshot of this thread group's shared pending signal set.
    pub fn shared_pending_signal_set(&self) -> SigSet {
        self.inner.read().sig_pending.lock().to_sigset()
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use core::sync::atomic::{AtomicUsize, Ordering};

    use super::{
        timer::{TimerSignalPendingOwner, TimerSignalSlotId},
        *,
    };
    use crate::task::sig::{
        PosixTimerSignalCallback, PosixTimerSignalCompletion, PosixTimerSignalEnqueue,
        PosixTimerSignalIdentity,
        info::{SiCode, SigInfoFields, SigKill, SigRt},
    };

    fn user_signal(no: SigNo) -> Signal {
        Signal::new(
            no,
            SiCode::User,
            SigInfoFields::Kill(SigKill {
                pid: Tid::new(2),
                uid: Uid::new(0),
            }),
        )
    }

    fn queued_signal(no: SigNo, sigval: u64) -> Signal {
        Signal::new(
            no,
            SiCode::Queue,
            SigInfoFields::Rt(SigRt {
                pid: Tid::new(2),
                uid: Uid::new(0),
                sigval,
            }),
        )
    }

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

    fn test_timer_owner() -> TimerSignalPendingOwner {
        TimerSignalPendingOwner::Shared(Weak::new())
    }

    fn register_timer(
        pending: &mut PendingSignals,
        no: SigNo,
        timer_id: i32,
        sigval: u64,
        callback: Arc<PosixTimerSignalCallback>,
    ) -> TimerSignalSlotId {
        pending
            .timer
            .try_register(test_timer_owner(), no, timer_id, sigval, callback)
            .unwrap()
    }

    fn enqueue_timer(
        pending: &mut PendingSignals,
        slot: TimerSignalSlotId,
        generation: u64,
        episode: u64,
        overrun: i32,
        ignored: bool,
    ) -> PosixTimerSignalEnqueue {
        enqueue_timer_for(
            pending,
            slot,
            &test_timer_owner(),
            generation,
            episode,
            overrun,
            ignored,
        )
    }

    fn enqueue_timer_for(
        pending: &mut PendingSignals,
        slot: TimerSignalSlotId,
        owner: &TimerSignalPendingOwner,
        generation: u64,
        episode: u64,
        overrun: i32,
        ignored: bool,
    ) -> PosixTimerSignalEnqueue {
        let PendingSignals {
            timer,
            next_arrival,
            ..
        } = pending;
        timer.enqueue(
            slot,
            owner,
            next_arrival,
            generation,
            episode,
            overrun,
            ignored,
        )
    }

    fn complete_timer_signal(
        pending: &mut PendingSignals,
        slot: TimerSignalSlotId,
        mut signal: Signal,
    ) {
        // Unit tests use an empty target Weak, so mirror the production
        // delivery order explicitly: make the slot reusable, then call owner.
        pending.timer.finish(slot);
        signal.finish_timer_signal_handoff(PosixTimerSignalCompletion::Dequeued);
    }

    fn timer_overrun(signal: &Signal) -> i32 {
        let SigInfoFields::Timer(timer) = signal.fields else {
            panic!("expected SI_TIMER fields")
        };
        timer.overrun
    }

    fn queued_sigval(signal: &Signal) -> u64 {
        let SigInfoFields::Rt(rt) = signal.fields else {
            panic!("expected SI_QUEUE fields")
        };
        rt.sigval
    }

    #[kunit]
    fn test_standard_timer_signals_keep_per_timer_identity() {
        let mut pending = PendingSignals::new();
        let (log, callback) = callback_log();
        let first = register_timer(&mut pending, SigNo::SIGUSR1, 7, 0x71, callback.clone());
        let second = register_timer(&mut pending, SigNo::SIGUSR1, 8, 0x81, callback);

        assert_eq!(
            enqueue_timer(&mut pending, first, 2, 10, 0, false),
            PosixTimerSignalEnqueue::Queued
        );
        assert_eq!(
            enqueue_timer(&mut pending, second, 3, 11, 0, false),
            PosixTimerSignalEnqueue::Queued
        );

        let set = SigSet::new_with_signos(&[SigNo::SIGUSR1]);
        let first_signal = pending.fetch_specific(set).unwrap();
        assert_eq!(
            first_signal.timer_signal_identity(),
            Some(PosixTimerSignalIdentity::new(7, 2, 10))
        );
        complete_timer_signal(&mut pending, first, first_signal);

        let second_signal = pending.fetch_specific(set).unwrap();
        assert_eq!(
            second_signal.timer_signal_identity(),
            Some(PosixTimerSignalIdentity::new(8, 3, 11))
        );
        complete_timer_signal(&mut pending, second, second_signal);
        assert!(pending.fetch_specific(set).is_none());
        assert_eq!(log.lock().as_slice().len(), 2);
    }

    #[kunit]
    fn test_same_timer_duplicate_updates_pending_episode_and_overrun() {
        let mut pending = PendingSignals::new();
        let (_, callback) = callback_log();
        let slot = register_timer(&mut pending, SigNo::SIGUSR1, 12, 0x12, callback);

        assert_eq!(
            enqueue_timer(&mut pending, slot, 4, 9, 0, false),
            PosixTimerSignalEnqueue::Queued
        );
        assert_eq!(
            enqueue_timer(&mut pending, slot, 5, 10, 17, false),
            PosixTimerSignalEnqueue::AlreadyPending
        );

        let signal = pending
            .fetch_specific(SigSet::new_with_signos(&[SigNo::SIGUSR1]))
            .unwrap();
        assert_eq!(
            signal.timer_signal_identity(),
            Some(PosixTimerSignalIdentity::new(12, 5, 10))
        );
        assert_eq!(timer_overrun(&signal), 17);
        complete_timer_signal(&mut pending, slot, signal);
    }

    #[kunit]
    fn test_new_generation_can_queue_while_stale_handoff_is_in_flight() {
        let mut pending = PendingSignals::new();
        let (log, callback) = callback_log();
        let slot = register_timer(&mut pending, SigNo::SIGUSR1, 13, 0, callback.clone());
        assert_eq!(
            enqueue_timer(&mut pending, slot, 1, 1, 0, false),
            PosixTimerSignalEnqueue::Queued
        );
        let stale = pending
            .fetch_specific(SigSet::new_with_signos(&[SigNo::SIGUSR1]))
            .unwrap();

        // Dequeue copied the immutable old episode out of the preallocated
        // pending resource. A new generation must be able to publish its own
        // occurrence before the stale callback returns.
        assert_eq!(
            enqueue_timer(&mut pending, slot, 2, 2, 0, false),
            PosixTimerSignalEnqueue::Queued
        );
        let current = pending
            .fetch_specific(SigSet::new_with_signos(&[SigNo::SIGUSR1]))
            .unwrap();
        drop(pending.timer.unregister(slot, &test_timer_owner()));

        let replacement = register_timer(&mut pending, SigNo::SIGUSR2, 14, 0, callback.clone());
        assert_ne!(replacement.index, slot.index);

        complete_timer_signal(&mut pending, slot, stale);
        complete_timer_signal(&mut pending, slot, current);
        assert_eq!(
            log.lock().as_slice(),
            &[
                (
                    PosixTimerSignalIdentity::new(13, 1, 1),
                    PosixTimerSignalCompletion::Dequeued,
                ),
                (
                    PosixTimerSignalIdentity::new(13, 2, 2),
                    PosixTimerSignalCompletion::Dequeued,
                ),
            ]
        );

        drop(pending.timer.unregister(replacement, &test_timer_owner()));
        let reused = register_timer(&mut pending, SigNo::SIGUSR1, 15, 0, callback);
        assert_eq!(reused.index, slot.index);
        drop(pending.timer.unregister(reused, &test_timer_owner()));
    }

    #[kunit]
    fn test_ignored_timer_signal_is_not_published() {
        let mut pending = PendingSignals::new();
        let (log, callback) = callback_log();
        let slot = register_timer(&mut pending, SigNo::SIGUSR1, 15, 0, callback);

        assert_eq!(
            enqueue_timer(&mut pending, slot, 1, 1, 0, true),
            PosixTimerSignalEnqueue::Ignored
        );
        assert!(!pending.has_specific(SigSet::new_with_signos(&[SigNo::SIGUSR1])));
        assert!(log.lock().is_empty());
    }

    #[kunit]
    fn test_deleted_timer_stale_generation_only_completes_handoff() {
        let mut pending = PendingSignals::new();
        let live_generation = Arc::new(AtomicUsize::new(5));
        let stale_callbacks = Arc::new(AtomicUsize::new(0));
        let callback_generation = live_generation.clone();
        let callback_count = stale_callbacks.clone();
        let callback: Arc<PosixTimerSignalCallback> = Arc::new(move |identity, _reason| {
            if identity.generation() as usize != callback_generation.load(Ordering::SeqCst) {
                callback_count.fetch_add(1, Ordering::SeqCst);
            }
            None
        });
        let slot = register_timer(&mut pending, SigNo::SIGUSR1, 19, 0, callback);
        assert_eq!(
            enqueue_timer(&mut pending, slot, 4, 20, 0, false),
            PosixTimerSignalEnqueue::Queued
        );

        // Deletion removes future enqueue authority but the already-pending
        // occurrence still owns its immutable generation until signal cleanup.
        drop(pending.timer.unregister(slot, &test_timer_owner()));
        let signal = pending
            .fetch_specific(SigSet::new_with_signos(&[SigNo::SIGUSR1]))
            .unwrap();
        complete_timer_signal(&mut pending, slot, signal);
        assert_eq!(stale_callbacks.load(Ordering::SeqCst), 1);
    }

    #[kunit]
    fn test_ordinary_standard_signal_still_uses_one_slot() {
        let mut pending = PendingSignals::new();
        pending.push_signal(user_signal(SigNo::SIGUSR1));
        pending.push_signal(user_signal(SigNo::SIGUSR1));

        let set = SigSet::new_with_signos(&[SigNo::SIGUSR1]);
        assert!(pending.fetch_specific(set).is_some());
        assert!(pending.fetch_specific(set).is_none());
    }

    #[kunit]
    fn test_realtime_fifo_spans_ordinary_and_timer_sources() {
        let mut pending = PendingSignals::new();
        let rt = SigNo::new(SIGRTMIN as usize);
        let (_, callback) = callback_log();
        let slot = register_timer(&mut pending, rt, 23, 0x23, callback);

        pending.push_signal(queued_signal(rt, 1));
        assert_eq!(
            enqueue_timer(&mut pending, slot, 2, 3, 0, false),
            PosixTimerSignalEnqueue::Queued
        );
        pending.push_signal(queued_signal(rt, 4));

        let set = SigSet::new_with_signos(&[rt]);
        assert_eq!(queued_sigval(&pending.fetch_specific(set).unwrap()), 1);
        let timer = pending.fetch_specific(set).unwrap();
        assert_eq!(
            timer.timer_signal_identity(),
            Some(PosixTimerSignalIdentity::new(23, 2, 3))
        );
        complete_timer_signal(&mut pending, slot, timer);
        assert_eq!(queued_sigval(&pending.fetch_specific(set).unwrap()), 4);
    }

    #[kunit]
    fn test_timer_flush_defers_owner_callback_until_after_pending_unlock() {
        let mut pending = PendingSignals::new();
        let (log, callback) = callback_log();
        let slot = register_timer(&mut pending, SigNo::SIGUSR1, 29, 0, callback);
        assert_eq!(
            enqueue_timer(&mut pending, slot, 7, 8, 0, false),
            PosixTimerSignalEnqueue::Queued
        );

        let set = SigSet::new_with_signos(&[SigNo::SIGUSR1]);
        let mut retired = pending.flush_specific(set);
        assert!(pending.fetch_specific(set).is_none());
        assert!(log.lock().is_empty());

        // A new generation can publish before the old flush callback runs;
        // detaching the old occurrence must keep the new episode fetchable.
        assert_eq!(
            enqueue_timer(&mut pending, slot, 8, 9, 0, false),
            PosixTimerSignalEnqueue::Queued
        );
        let mut retired = retired.pop().unwrap();
        pending.timer.finish(slot);
        retired.finish_timer_signal_handoff(PosixTimerSignalCompletion::Flushed);
        let current = pending.fetch_specific(set).unwrap();
        complete_timer_signal(&mut pending, slot, current);
        assert_eq!(
            log.lock().as_slice(),
            &[
                (
                    PosixTimerSignalIdentity::new(29, 7, 8),
                    PosixTimerSignalCompletion::Flushed,
                ),
                (
                    PosixTimerSignalIdentity::new(29, 8, 9),
                    PosixTimerSignalCompletion::Dequeued,
                ),
            ]
        );
    }

    #[kunit]
    fn test_timer_notification_slot_is_reused_only_after_cleanup() {
        let mut pending = PendingSignals::new();
        let (_, callback) = callback_log();
        let first = register_timer(&mut pending, SigNo::SIGUSR1, 31, 0, callback.clone());
        drop(pending.timer.unregister(first, &test_timer_owner()));
        let second = register_timer(&mut pending, SigNo::SIGUSR2, 32, 0, callback);

        assert_eq!(first.index, second.index);
        assert!(second.reuse_generation > first.reuse_generation);
        drop(pending.timer.unregister(second, &test_timer_owner()));
    }

    #[kunit]
    fn test_exiting_task_can_retire_reserved_timer_handoff() {
        let mut pending = PendingSignals::new();
        let (log, callback) = callback_log();
        let slot = register_timer(&mut pending, SigNo::SIGUSR1, 37, 0, callback);
        assert_eq!(
            enqueue_timer(&mut pending, slot, 3, 5, 0, false),
            PosixTimerSignalEnqueue::Queued
        );
        let signal = pending
            .fetch_specific(SigSet::new_with_signos(&[SigNo::SIGUSR1]))
            .unwrap();
        pending.reserve_delivery_target(signal);

        let mut retired = pending.take_reserved_timer_signal_for_exit().unwrap();
        pending.timer.finish(slot);
        retired.finish_timer_signal_handoff(PosixTimerSignalCompletion::Flushed);
        assert_eq!(
            log.lock().as_slice(),
            &[(
                PosixTimerSignalIdentity::new(37, 3, 5),
                PosixTimerSignalCompletion::Flushed,
            )]
        );
    }

    #[kunit]
    fn test_exit_closes_admission_and_flushes_private_timer_state() {
        let mut pending = PendingSignals::new();
        let (log, callback) = callback_log();
        let owner = TimerSignalPendingOwner::Private {
            target: Weak::new(),
            thread_group: Weak::new(),
        };
        let slot = pending
            .timer
            .try_register(owner.clone(), SigNo::SIGUSR1, 41, 0, callback.clone())
            .unwrap();
        assert_eq!(
            enqueue_timer_for(&mut pending, slot, &owner, 5, 7, 0, false),
            PosixTimerSignalEnqueue::Queued
        );

        let reserved = pending.take_reserved_timer_signal_for_exit();
        let mut retired = pending.timer.close_for_exit(reserved);
        assert_eq!(retired.len(), 1);
        assert_eq!(
            enqueue_timer_for(&mut pending, slot, &owner, 5, 8, 0, false),
            PosixTimerSignalEnqueue::TargetExited
        );
        assert_eq!(
            pending.timer.try_register(
                TimerSignalPendingOwner::Private {
                    target: Weak::new(),
                    thread_group: Weak::new(),
                },
                SigNo::SIGUSR2,
                42,
                0,
                callback,
            ),
            Err(SysError::NoSuchProcess)
        );

        let mut retired = retired.pop().unwrap();
        pending.timer.finish(slot);
        retired.finish_timer_signal_handoff(PosixTimerSignalCompletion::Flushed);
        drop(pending.timer.take_exiting_registration());
        assert_eq!(
            log.lock().as_slice(),
            &[(
                PosixTimerSignalIdentity::new(41, 5, 7),
                PosixTimerSignalCompletion::Flushed,
            )]
        );
    }

    #[kunit]
    fn test_reserved_sigcont_survives_stop_class_cleanup() {
        let mut pending = PendingSignals::new();
        pending.push_signal(user_signal(SigNo::SIGCONT));
        assert!(pending.reserve_any_for_delivery(SigSet::new()));
        pending.push_signal(user_signal(SigNo::SIGCONT));

        assert!(
            pending
                .flush_specific(SigSet::new_with_signos(&[SigNo::SIGCONT]))
                .is_empty()
        );
        assert_eq!(pending.reserved_delivery_signo(), Some(SigNo::SIGCONT));
        let sigcont = SigSet::new_with_signos(&[SigNo::SIGCONT]);
        assert!(
            pending.has_specific(sigcont),
            "user-entry arbitration must still observe the reserved target"
        );
        assert!(
            !pending.has_dequeueable_specific(sigcont),
            "synchronous readiness must exclude the reserved target"
        );
        assert!(
            pending.fetch_specific(sigcont).is_none(),
            "ordinary specific dequeue must not compete with reserved delivery"
        );

        let fetched = pending
            .fetch_matching(SigSet::new(), |signal, reserved| {
                reserved && signal.no == SigNo::SIGCONT
            })
            .expect("reserved SIGCONT must remain claimable");
        assert!(fetched.reserved);
        assert_eq!(fetched.signal.no, SigNo::SIGCONT);
        assert!(!pending.has_specific(SigSet::new_with_signos(&[SigNo::SIGCONT])));
    }
}
