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
            self.unreliable[no] = Some(signal);
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
        if let Some(signal) = self.reserved_delivery.take() {
            return Some(signal);
        }

        self.fetch_unreserved_any(mask)
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
