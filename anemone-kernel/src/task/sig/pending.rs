use anemone_abi::process::linux::signal::*;

use crate::{
    prelude::*,
    task::{
        Task, ThreadGroup,
        sig::{SigNo, Signal, set::SigSet},
    },
};

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
    realtime: [VecDeque<Signal>; NRTSIG],
}

pub(super) struct FetchedSignal {
    pub(super) signal: Signal,
    pub(super) reserved: bool,
}

impl PendingSignals {
    pub fn new() -> Self {
        Self {
            reserved_delivery: None,
            unreliable: [const { None }; NUNRELIABLESIG + 1],
            realtime: [const { VecDeque::new() }; NRTSIG],
        }
    }

    /// Push a signal to the pending signals.
    ///
    /// Panics if the sicode and fields of the signal are not consistent.
    pub fn push_signal(&mut self, signal: Signal) {
        debug_assert!(signal.fields.validate_with(signal.code));

        if let Some(rt_idx) = signal.no.realtime_index() {
            self.realtime[rt_idx].push_back(signal);
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
        }

        for (idx, queue) in self.realtime.iter_mut().enumerate() {
            let no = SigNo::new(SIGRTMIN as usize + idx);
            if mask.get(no) {
                continue;
            }
            if queue.front().is_some_and(|signal| allowed(signal, false)) {
                return queue.pop_front().map(|signal| FetchedSignal {
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
        if let Some(stop) = self.unreliable[SigNo::SIGSTOP.as_usize()].take() {
            return Some(stop);
        }

        // realtime signals first. we just do a linear scan.
        for (idx, queue) in self.realtime.iter_mut().enumerate() {
            let rt_idx = SigNo::new(SIGRTMIN as usize + idx);
            if mask.get(rt_idx) {
                continue;
            }
            if let Some(signal) = queue.pop_front() {
                return Some(signal);
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

    /// Fetch any pending signal in the given set, and remove it from the
    /// pending signals.
    ///
    /// SIGKILL and SIGSTOP won't be fetched if they're not in the set.
    pub fn fetch_specific(&mut self, set: SigSet) -> Option<Signal> {
        // realtime signals first.
        for (idx, queue) in self.realtime.iter_mut().enumerate() {
            let rt_idx = SigNo::new(SIGRTMIN as usize + idx);
            if !set.get(rt_idx) {
                continue;
            }
            if let Some(signal) = queue.pop_front() {
                return Some(signal);
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
        false
    }

    pub fn has_specific(&self, set: SigSet) -> bool {
        if let Some(signal) = &self.reserved_delivery {
            if set.get(signal.no) {
                return true;
            }
        }
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
        false
    }

    /// Remove all pending signals in the given set from the pending signals.
    ///
    /// Mainly used when a disposition is set to ignore, to flush all pending
    /// signals that are now ignored.
    pub fn flush_specific(&mut self, set: SigSet) {
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
    use super::*;
    use crate::task::sig::info::{SiCode, SigInfoFields, SigKill};

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

    #[kunit]
    fn test_reserved_sigcont_survives_stop_class_cleanup() {
        let mut pending = PendingSignals::new();
        pending.push_signal(user_signal(SigNo::SIGCONT));
        assert!(pending.reserve_any_for_delivery(SigSet::new()));
        pending.push_signal(user_signal(SigNo::SIGCONT));

        pending.flush_specific(SigSet::new_with_signos(&[SigNo::SIGCONT]));
        assert_eq!(pending.reserved_delivery_signo(), Some(SigNo::SIGCONT));

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
