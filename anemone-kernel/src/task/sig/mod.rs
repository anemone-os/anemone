//! POSIX signals implementation.
//!
//! Note: we didn't take effort to abstract away from Linux's signal
//! implementation, since signal is a POSIX API, and we can't have 2 different
//! POSIX implementations in the same kernel. However this does not mean we
//! should copy Linux's implementation directly, we just follows similar design,
//! semantics, and the same uapi, to provide compatibility with Unix/Linux
//! user-space.
//!
//! Just a reminder: si_code, sa_flags and so on are not Linux-specific,
//! they are all defined in POSIX. So existence of [SiCode] or [SaFlags] doesn't
//! mean our kernel is polluted by Linux-internal stuff. But indeed we take the
//! same encoding as Linux for those fields for compatibility.
//!
//! Much logic relies on the fact that signal numbers are between 0 and 63. We
//! just hardcoded this fact in many places. We can refactor this later.

use anemone_abi::process::linux::{signal as linux_signal, signal::NSIG, ucontext::UContext};

use crate::{
    prelude::*,
    sched::CurrentWaitOutcome,
    syscall::{handler::TryFromSyscallArg, user_access::UserWritePtr},
    task::{
        exit::kernel_exit_group,
        sig::{
            disposition::{KSigAction, SaFlags, SignalAction, SignalDisposition},
            info::{SiCode, SigInfoFields},
            set::SigSet,
        },
    },
};

mod api;
pub use api::*;
mod hal;
pub use hal::*;

pub mod altstack;
pub mod disposition;
pub mod info;
pub mod set;

/// Per-task signal mask state.
///
/// The `restore` slot is the single delayed-restore owner.
/// `active_restore_slot` is identity metadata for linear temporary-mask tokens;
/// it is not a second mask source.
#[derive(Debug)]
pub struct TaskSigMaskState {
    current: SigSet,
    restore: Option<SigSet>,
    active_restore_slot: Option<TemporarySigMaskSlotId>,
    next_restore_slot: TemporarySigMaskSlotId,
}

impl TaskSigMaskState {
    pub const fn new() -> Self {
        Self {
            current: SigSet::new(),
            restore: None,
            active_restore_slot: None,
            next_restore_slot: TemporarySigMaskSlotId::new(1),
        }
    }

    fn current(&self) -> SigSet {
        self.current
    }

    fn assert_valid_mask(mask: SigSet) {
        assert!(
            !mask.get(SigNo::SIGKILL) && !mask.get(SigNo::SIGSTOP),
            "SIGKILL and SIGSTOP cannot be masked"
        );
    }

    fn assert_restore_slot_invariant(&self, task_id: Tid, context: &'static str) {
        assert!(
            self.restore.is_some() == self.active_restore_slot.is_some(),
            "temporary signal mask restore slot invariant failed: task={} context={} restore_present={} active_slot={:?}",
            task_id,
            context,
            self.restore.is_some(),
            self.active_restore_slot,
        );
    }

    fn assert_no_pending_restore(&self, task_id: Tid, context: &'static str) {
        self.assert_restore_slot_invariant(task_id, context);
        assert!(
            self.restore.is_none(),
            "ordinary signal mask mutation while temporary restore is pending: task={} context={} active_slot={:?}",
            task_id,
            context,
            self.active_restore_slot,
        );
    }

    fn set_permanent_current(&mut self, task_id: Tid, new_mask: SigSet) {
        self.assert_no_pending_restore(task_id, "set_permanent_current");
        Self::assert_valid_mask(new_mask);
        self.current = new_mask;
    }

    fn mutate_current(
        &mut self,
        task_id: Tid,
        context: &'static str,
        f: impl FnOnce(&mut SigSet),
    ) -> SigSet {
        self.assert_no_pending_restore(task_id, context);
        self.mutate_current_allowing_pending_restore(f)
    }

    fn mutate_current_for_signal_delivery(&mut self, f: impl FnOnce(&mut SigSet)) -> SigSet {
        self.mutate_current_allowing_pending_restore(f)
    }

    fn mutate_current_allowing_pending_restore(&mut self, f: impl FnOnce(&mut SigSet)) -> SigSet {
        let old_mask = self.current;
        f(&mut self.current);
        Self::assert_valid_mask(self.current);
        old_mask
    }

    fn begin_temporary(&mut self, task_id: Tid, new_mask: SigSet) -> TemporarySigMaskSlotId {
        self.assert_no_pending_restore(task_id, "begin_temporary_sig_mask");
        Self::assert_valid_mask(new_mask);

        let old_mask = self.current;
        let slot = self.next_restore_slot;
        self.next_restore_slot = self.next_restore_slot.next();
        self.restore = Some(old_mask);
        self.active_restore_slot = Some(slot);
        self.current = new_mask;
        self.assert_restore_slot_invariant(task_id, "begin_temporary_sig_mask");
        slot
    }

    fn assert_active_restore_slot(
        &self,
        task_id: Tid,
        slot: TemporarySigMaskSlotId,
        context: &'static str,
    ) {
        self.assert_restore_slot_invariant(task_id, context);
        assert!(
            self.active_restore_slot == Some(slot),
            "temporary signal mask token slot mismatch: task={} context={} token_slot={:?} active_slot={:?}",
            task_id,
            context,
            slot,
            self.active_restore_slot,
        );
    }

    fn restore_temporary_now(&mut self, task_id: Tid, slot: TemporarySigMaskSlotId) {
        self.assert_active_restore_slot(task_id, slot, "restore_temporary_now");
        let old_mask = self
            .restore
            .take()
            .expect("active temporary mask slot must have a restore mask");
        self.active_restore_slot = None;
        Self::assert_valid_mask(old_mask);
        self.current = old_mask;
        self.assert_restore_slot_invariant(task_id, "restore_temporary_now");
    }

    fn assert_defer_slot(&self, task_id: Tid, slot: TemporarySigMaskSlotId) {
        self.assert_active_restore_slot(task_id, slot, "defer_temporary_to_signal_delivery");
    }

    fn sigmask_to_save_for_signal_frame(&self) -> SigSet {
        self.restore.unwrap_or(self.current)
    }

    fn signal_frame_committed_restore_mask(&mut self, task_id: Tid) {
        self.assert_restore_slot_invariant(task_id, "signal_frame_committed_restore_mask");
        self.restore = None;
        self.active_restore_slot = None;
        self.assert_restore_slot_invariant(task_id, "signal_frame_committed_restore_mask");
    }

    fn restore_temporary_if_pending(&mut self, task_id: Tid) {
        self.assert_restore_slot_invariant(task_id, "restore_temporary_if_pending");
        if let Some(old_mask) = self.restore.take() {
            self.active_restore_slot = None;
            Self::assert_valid_mask(old_mask);
            self.current = old_mask;
        }
        self.assert_restore_slot_invariant(task_id, "restore_temporary_if_pending");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TemporarySigMaskSlotId(u64);

impl TemporarySigMaskSlotId {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("temporary sigmask slot id overflow"),
        )
    }
}

/// Linear ownership token for a pending temporary signal-mask restore.
///
/// Dropping this token never restores the old mask and never clears the restore
/// slot. Callers must end it with exactly one terminal method.
#[must_use = "temporary signal mask tokens must be ended with restore_now() or defer_to_signal_delivery()"]
pub struct TemporarySigMaskToken {
    task: Arc<Task>,
    slot: TemporarySigMaskSlotId,
    active: bool,
}

impl TemporarySigMaskToken {
    fn new(task: Arc<Task>, slot: TemporarySigMaskSlotId) -> Self {
        Self {
            task,
            slot,
            active: true,
        }
    }

    fn assert_current_task(&self, context: &'static str) {
        let current = get_current_task();
        assert!(
            Arc::ptr_eq(&current, &self.task),
            "temporary signal mask token used on non-owner task: context={} owner={} current={} slot={:?}",
            context,
            self.task.tid(),
            current.tid(),
            self.slot,
        );
    }

    /// Restore the old mask immediately and clear the pending restore slot.
    pub fn restore_now(mut self) {
        self.assert_current_task("restore_now");
        self.task
            .sig_mask
            .lock()
            .restore_temporary_now(self.task.tid(), self.slot);
        self.active = false;
    }

    /// Leave restore responsibility with trap-return signal delivery.
    pub fn defer_to_signal_delivery(mut self) {
        self.assert_current_task("defer_to_signal_delivery");
        self.task
            .sig_mask
            .lock()
            .assert_defer_slot(self.task.tid(), self.slot);
        self.active = false;
    }
}

impl Drop for TemporarySigMaskToken {
    fn drop(&mut self) {
        if self.active {
            kwarningln!(
                "temporary signal mask token leaked without terminal method: task={} slot={:?}",
                self.task.tid(),
                self.slot,
            );
            assert!(
                !self.active,
                "temporary signal mask token leaked without terminal method"
            );
        }
    }
}

/// Typed wait outcome candidate for delayed temporary-mask classification.
///
/// This is intentionally signal-owned. Delayed-restore callsites should pass
/// their scheduler/latch outcome through this type instead of interpreting
/// pending queues, dispositions, ignore/default/custom actions, or force wakes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporaryMaskWaitCandidate {
    PredicateReady,
    Timeout,
    Signal,
    Force,
    Cancelled,
    Unexpected,
}

impl From<CurrentWaitOutcome> for TemporaryMaskWaitCandidate {
    fn from(value: CurrentWaitOutcome) -> Self {
        match value {
            CurrentWaitOutcome::PredicateReady => Self::PredicateReady,
            CurrentWaitOutcome::Timeout => Self::Timeout,
            CurrentWaitOutcome::Signal => Self::Signal,
            CurrentWaitOutcome::Force => Self::Force,
            CurrentWaitOutcome::Cancelled => Self::Cancelled,
            CurrentWaitOutcome::Unexpected => Self::Unexpected,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporaryMaskWaitContext {
    RtSigsuspend,
    Ppoll,
    Pselect6,
}

impl TemporaryMaskWaitContext {
    const fn as_str(self) -> &'static str {
        match self {
            Self::RtSigsuspend => "rt_sigsuspend",
            Self::Ppoll => "ppoll",
            Self::Pselect6 => "pselect6",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporaryMaskWaitReturn {
    /// The wait was not a signal-delivery carrier; restore the token and let
    /// the caller map its original typed wait result.
    OriginalOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporaryMaskWaitDecision {
    /// A stable trap-return delivery target is already reserved for this task.
    DeferToTrapReturnDelivery,
    RestoreThenReturn(TemporaryMaskWaitReturn),
    RestoreThenFailClosed(SysError),
    /// A force wake matched a non-returning signal target. Callers must not map
    /// this to an ordinary `EINTR` carrier.
    NoReturnForce,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SigNo(usize);

macro_rules! define_typed_signo {
    ($($no:ident),*) => {
        $(
            pub const $no: Self = Self($no as usize);
        )*
    };
}
use anemone_abi::process::linux::signal::*;
impl SigNo {
    define_typed_signo!(
        SIGHUP, SIGINT, SIGQUIT, SIGILL, SIGTRAP, SIGABRT, SIGBUS, SIGFPE, SIGKILL, SIGUSR1,
        SIGSEGV, SIGUSR2, SIGPIPE, SIGALRM, SIGTERM, SIGCHLD, SIGCONT, SIGSTOP, SIGTSTP, SIGTTIN,
        SIGTTOU, SIGURG, SIGXCPU, SIGXFSZ, SIGVTALRM, SIGPROF, SIGWINCH, SIGIO, SIGPWR, SIGSYS
    );
}

impl SigNo {
    pub fn new(sig: usize) -> Self {
        assert!(
            sig < NSIG && sig != 0,
            "signal number {} is out of range",
            sig
        );
        Self(sig)
    }

    pub const fn as_usize(&self) -> usize {
        self.0
    }

    pub const fn is_realtime(&self) -> bool {
        self.as_usize() >= SIGRTMIN as usize && self.as_usize() <= SIGRTMAX as usize
    }

    pub const fn is_unreliable(&self) -> bool {
        !self.is_realtime()
    }

    /// Get the index of the realtime signal, if this is a realtime signal.
    pub const fn realtime_index(&self) -> Option<usize> {
        if self.is_realtime() {
            Some(self.as_usize() - SIGRTMIN as usize)
        } else {
            None
        }
    }
}

impl TryFromSyscallArg for SigNo {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        if raw == 0 || raw >= NSIG as u64 {
            return Err(SysError::InvalidArgument);
        }
        Ok(Self::new(raw as usize))
    }
}

/// A sent/sending signal.
#[derive(Debug, Clone)]
pub struct Signal {
    /// Signal number.
    no: SigNo,
    /// `si_errno` in `struct siginfo_t`.
    ///
    /// Linux almost always doesn't set this field, nor does user-space. We
    /// define it here for completeness.
    errno: i32,
    /// How the signal is generated.
    code: SiCode,
    /// Various information about the signal, depends on `code`.
    fields: SigInfoFields,
}

impl Signal {
    /// Create a new [Signal] with the given fields. `errno` is set to 0.
    ///
    /// Panics if the fields are not valid for the given code, which indicates a
    /// bug in kernel code.
    pub fn new(no: SigNo, code: SiCode, fields: SigInfoFields) -> Self {
        debug_assert!(fields.validate_with(code));
        Self {
            no,
            errno: 0,
            code,
            fields,
        }
    }

    /// Create a new [Signal] with the given fields and errno.
    ///
    /// Panics if the fields are not valid for the given code, which indicates a
    /// bug in kernel code.
    ///
    /// Why will you need to set errno? Idk. But let's just provide this API for
    /// completeness.
    pub fn new_with_errno(no: SigNo, code: SiCode, fields: SigInfoFields, errno: i32) -> Self {
        debug_assert!(fields.validate_with(code));
        Self {
            no,
            errno,
            code,
            fields,
        }
    }

    pub fn to_linux_siginfo(self) -> SigInfoWrapper {
        let mut kbuf = linux_signal::sifields::SigInfoFields::default();
        self.fields.serialize_to_linux(&mut kbuf);

        SigInfoWrapper {
            info: linux_signal::SigInfo {
                si_signo: self.no.as_usize() as i32,
                si_errno: self.errno,
                si_code: self.code.to_linux_code(),
                fields: kbuf,
            },
        }
    }
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

    fn fetch_unreserved_any(&mut self, mask: SigSet) -> Option<Signal> {
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
    fn reserve_any_for_delivery(&mut self, mask: SigSet) -> bool {
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

    fn reserve_specific_for_delivery(&mut self, set: SigSet) -> bool {
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

    fn reserve_delivery_target(&mut self, signal: Signal) -> SigNo {
        assert!(
            self.reserved_delivery.is_none(),
            "temporary signal delivery target is already reserved"
        );
        let no = signal.no;
        self.reserved_delivery = Some(signal);
        no
    }

    fn reserved_delivery_signo(&self) -> Option<SigNo> {
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
    /// Check whether this task has unmasked signals.
    ///
    /// 'masked' here refers to the signal mask of this task.
    ///
    /// This method relies on the fact that ignored signals won't be delivered
    /// into [PendingSignals]. See [Task::recv_signal] for details.
    pub fn has_unmasked_signal(&self) -> bool {
        let prv_pending = {
            let pending = self.sig_pending.lock();
            let mask = self.snapshot_current_sig_mask();
            pending.has_unmasked(mask)
        };
        if prv_pending {
            return true;
        }

        // here is a window. does this matter? i think not. but im not sure.

        let tg = self.get_thread_group();
        let tg_inner = tg.inner.read();
        let shared_pending = {
            let pending = tg_inner.sig_pending.lock();
            let mask = self.snapshot_current_sig_mask();
            pending.has_unmasked(mask)
        };
        shared_pending
    }

    pub fn has_specific_signal(&self, set: SigSet) -> bool {
        {
            let pending = self.sig_pending.lock();
            if pending.has_specific(set) {
                return true;
            }
        }
        {
            let tg = self.get_thread_group();
            let tg_inner = tg.inner.read();
            let pending = tg_inner.sig_pending.lock();
            if pending.has_specific(set) {
                return true;
            }
        }
        false
    }

    /// Send a signal to this task (I.e. let this task receive a signal).
    /// - For unreliable signals, we just set the bit in `sig_pending`. if the
    ///   slot is already occupied, the old signal will be overwritten, and lost
    ///   forever.
    /// - For realtime signals, new signals will be pushed to the back of the
    ///   queue, and they won't be lost.
    ///
    /// If the signal is masked, task won't be notified, except for [SIGKILL]
    /// and [SIGSTOP].
    ///
    /// If the disposition of the signal satisfies [SignalAction::is_ignored],
    /// the signal won't be delivered, even if it is unmasked.
    pub fn recv_signal(self: &Arc<Self>, signal: Signal) {
        // kspecialln!("{:?} -> {:?}", self.tid(), signal.no);
        kdebugln!("task {} recv_signal: {:?}", self.tid(), signal);
        let no = signal.no;

        let disp = self.sig_disposition.read().get_disposition(no);
        if disp.action.is_ignored() {
            kdebugln!("signal {:?} is ignored by the task, not queuing", no);
            return;
        }

        self.sig_pending.lock().push_signal(signal);

        if self.is_current_sig_mask_blocking(no) && !matches!(no, SigNo::SIGKILL | SigNo::SIGSTOP) {
            // signal masked. nothing to do now, just wait for the task to
            // unmask it.
        } else {
            kdebugln!("signal {:?} is not masked, notifying task", no);
            notify(
                self,
                if matches!(no, SigNo::SIGKILL | SigNo::SIGSTOP) {
                    true
                } else {
                    false
                },
            );
        }
    }

    /// Called when this task is about to return to user-space.
    ///
    /// Masked signals won't be fetched.
    ///
    /// See [PendingSignals::fetch_any] for the order of fetching signals.
    pub fn fetch_signal(&self) -> Option<Signal> {
        // first private pending
        {
            let mut pending = self.sig_pending.lock();
            let mask = self.snapshot_current_sig_mask();
            if let Some(signal) = pending.fetch_any(mask) {
                return Some(signal);
            }
        }

        // no private signals satisfied the criteria. check shared pending signals.
        {
            let tg = self.get_thread_group();
            let tg_inner = tg.inner.read();
            let mut pending = tg_inner.sig_pending.lock();
            let mask = self.snapshot_current_sig_mask();
            if let Some(signal) = pending.fetch_any(mask) {
                return Some(signal);
            }
        }

        None
    }

    /// See [PendingSignals::fetch_specific] for more details.
    pub fn fetch_specific_signal(&self, set: SigSet) -> Option<Signal> {
        // first private pending
        {
            let mut pending = self.sig_pending.lock();
            if let Some(signal) = pending.fetch_specific(set) {
                return Some(signal);
            }
        }

        // no private signals satisfied the criteria. check shared pending signals.
        {
            let tg = self.get_thread_group();
            let tg_inner = tg.inner.read();
            let mut pending = tg_inner.sig_pending.lock();
            if let Some(signal) = pending.fetch_specific(set) {
                return Some(signal);
            }
        }

        None
    }

    pub fn classify_temporary_mask_wait(
        &self,
        candidate: impl Into<TemporaryMaskWaitCandidate>,
        context: TemporaryMaskWaitContext,
    ) -> TemporaryMaskWaitDecision {
        let candidate = candidate.into();
        match candidate {
            TemporaryMaskWaitCandidate::PredicateReady | TemporaryMaskWaitCandidate::Timeout => {
                TemporaryMaskWaitDecision::RestoreThenReturn(
                    TemporaryMaskWaitReturn::OriginalOutcome,
                )
            },
            TemporaryMaskWaitCandidate::Cancelled | TemporaryMaskWaitCandidate::Unexpected => {
                self.log_temporary_mask_classifier_fail_closed(candidate, context);
                TemporaryMaskWaitDecision::RestoreThenFailClosed(SysError::IO)
            },
            TemporaryMaskWaitCandidate::Signal | TemporaryMaskWaitCandidate::Force => {
                self.classify_signal_temporary_mask_candidate(candidate, context)
            },
        }
    }

    fn classify_signal_temporary_mask_candidate(
        &self,
        candidate: TemporaryMaskWaitCandidate,
        context: TemporaryMaskWaitContext,
    ) -> TemporaryMaskWaitDecision {
        let force_only = matches!(candidate, TemporaryMaskWaitCandidate::Force);
        let Some(signo) = self.reserve_temporary_mask_delivery_target(force_only) else {
            self.log_temporary_mask_classifier_fail_closed(candidate, context);
            return TemporaryMaskWaitDecision::RestoreThenFailClosed(SysError::IO);
        };

        let action = self.sig_disposition.read().get_disposition(signo).action;
        kdebugln!(
            "{}: temporary-mask classifier reserved task={} candidate={:?} signo={:?} action={:?}",
            context.as_str(),
            self.tid(),
            candidate,
            signo,
            action,
        );

        if matches!(signo, SigNo::SIGKILL | SigNo::SIGSTOP) {
            // Force-wake targets are deliberately not represented as an
            // ordinary EINTR carrier. The signal is reserved for trap return;
            // the callsite must restore the token and enter its no-return
            // force path instead of treating the wait as a recoverable signal.
            return TemporaryMaskWaitDecision::NoReturnForce;
        }

        if force_only {
            self.log_temporary_mask_classifier_fail_closed(candidate, context);
            return TemporaryMaskWaitDecision::RestoreThenFailClosed(SysError::IO);
        }

        TemporaryMaskWaitDecision::DeferToTrapReturnDelivery
    }

    fn reserve_temporary_mask_delivery_target(&self, force_only: bool) -> Option<SigNo> {
        if let Some(signo) = self.private_reserved_delivery_signo() {
            return Some(signo);
        }

        if let Some(signo) = self.reserve_private_temporary_mask_delivery_target(force_only) {
            return Some(signo);
        }

        self.reserve_shared_temporary_mask_delivery_target(force_only)
    }

    fn private_reserved_delivery_signo(&self) -> Option<SigNo> {
        self.sig_pending.lock().reserved_delivery_signo()
    }

    fn reserve_private_temporary_mask_delivery_target(&self, force_only: bool) -> Option<SigNo> {
        let mut pending = self.sig_pending.lock();
        if let Some(signo) = pending.reserved_delivery_signo() {
            return Some(signo);
        }

        let reserved = if force_only {
            pending.reserve_specific_for_delivery(force_signal_set())
        } else {
            let mask = self.sig_mask.lock().current();
            pending.reserve_any_for_delivery(mask)
        };

        reserved.then(|| {
            pending
                .reserved_delivery_signo()
                .expect("reserved temporary delivery target must record a signal number")
        })
    }

    fn reserve_shared_temporary_mask_delivery_target(&self, force_only: bool) -> Option<SigNo> {
        let signal = {
            let tg = self.get_thread_group();
            let tg_inner = tg.inner.read();
            let mut pending = tg_inner.sig_pending.lock();
            if force_only {
                pending.fetch_specific(force_signal_set())
            } else {
                let mask = self.sig_mask.lock().current();
                pending.fetch_unreserved_any(mask)
            }
        }?;

        let mut pending = self.sig_pending.lock();
        Some(pending.reserve_delivery_target(signal))
    }

    fn log_temporary_mask_classifier_fail_closed(
        &self,
        candidate: TemporaryMaskWaitCandidate,
        context: TemporaryMaskWaitContext,
    ) {
        kwarningln!(
            "{}: temporary-mask classifier fail-closed task={} candidate={:?} current_mask={:?} private_pending={:?} shared_pending={:?}",
            context.as_str(),
            self.tid(),
            candidate,
            self.snapshot_current_sig_mask(),
            self.pending_signal_set(),
            self.get_thread_group().shared_pending_signal_set(),
        );
    }

    /// Snapshot the current signal mask. Pending delayed-restore state is not
    /// exposed by this API.
    pub fn snapshot_current_sig_mask(&self) -> SigSet {
        self.sig_mask.lock().current()
    }

    /// Return a snapshot of this task's private pending signal set.
    pub fn pending_signal_set(&self) -> SigSet {
        self.sig_pending.lock().to_sigset()
    }

    /// Set the permanent current signal mask. Caller is responsible for
    /// ensuring the validity of `new_mask`, i.e. it should not have SIGKILL and
    /// SIGSTOP set.
    pub fn set_permanent_sig_mask(&self, new_mask: SigSet) {
        self.sig_mask
            .lock()
            .set_permanent_current(self.tid(), new_mask);
    }

    /// Mutate the current mask for ordinary current-mask operations and return
    /// the previous mask.
    pub fn mutate_current_sig_mask(&self, f: impl FnOnce(&mut SigSet)) -> SigSet {
        self.sig_mask
            .lock()
            .mutate_current(self.tid(), "mutate_current_sig_mask", f)
    }

    /// Restore the current mask from a committed signal frame context.
    ///
    /// This is intentionally distinct from delayed temporary-mask restore. It
    /// does not read, consume, or overwrite the pending restore slot.
    pub fn restore_sigframe_current_sig_mask(&self, new_mask: SigSet) {
        self.set_permanent_sig_mask(new_mask);
    }

    /// Temporarily mutate current signal mask inside a syscall body and return
    /// the previous mask. The caller must restore with
    /// [Task::restore_syscall_body_current_sig_mask].
    pub fn mutate_syscall_body_current_sig_mask(&self, f: impl FnOnce(&mut SigSet)) -> SigSet {
        self.mutate_current_sig_mask(f)
    }

    /// Restore a syscall-body-only temporary current mask.
    pub fn restore_syscall_body_current_sig_mask(&self, old_mask: SigSet) {
        self.set_permanent_sig_mask(old_mask);
    }

    /// Begin a delayed temporary signal mask window for the current task.
    ///
    /// This installs `new_mask` as current and records the old mask in the
    /// single restore slot before returning a linear token.
    pub fn begin_temporary_sig_mask(self: &Arc<Self>, new_mask: SigSet) -> TemporarySigMaskToken {
        let task_id = self.tid();
        let slot = self.sig_mask.lock().begin_temporary(task_id, new_mask);
        TemporarySigMaskToken::new(self.clone(), slot)
    }

    /// Mask value that should be encoded into a signal frame.
    ///
    /// During a pending temporary-mask window this returns the saved old mask;
    /// otherwise it returns the current mask.
    pub fn sigmask_to_save_for_signal_frame(&self) -> SigSet {
        self.sig_mask.lock().sigmask_to_save_for_signal_frame()
    }

    /// Consume the pending restore slot after a user signal frame has been
    /// committed and restore responsibility has moved to `rt_sigreturn()`.
    pub fn signal_frame_committed_restore_mask(&self) {
        self.sig_mask
            .lock()
            .signal_frame_committed_restore_mask(self.tid());
    }

    /// Restore a pending temporary mask before returning to user mode without a
    /// committed handler frame.
    pub fn restore_temporary_sig_mask_if_pending(&self) {
        self.sig_mask
            .lock()
            .restore_temporary_if_pending(self.tid());
    }

    /// Signal delivery may install handler masks while a temporary restore is
    /// pending. Ordinary mutation helpers intentionally reject that state.
    pub fn mutate_current_sig_mask_for_signal_delivery(
        &self,
        f: impl FnOnce(&mut SigSet),
    ) -> SigSet {
        self.sig_mask.lock().mutate_current_for_signal_delivery(f)
    }

    fn is_current_sig_mask_blocking(&self, no: SigNo) -> bool {
        self.snapshot_current_sig_mask().get(no)
    }
}

fn force_signal_set() -> SigSet {
    SigSet::new_with_signos(&[SigNo::SIGKILL, SigNo::SIGSTOP])
}

impl ThreadGroup {
    /// Get the signal disposition of this thread group.
    ///
    /// Internally, this relies on the fact that
    /// [super::clone::CloneFlags::THREAD] must be used with
    /// [clone::CloneFlags::SIGHAND], so that all threads in the same thread
    /// group share the same [SignalDisposition].
    ///
    /// Return `None` if this thread group has no members, (i.e. the thread
    /// group is waiting to be reaped).
    pub fn signal_disposition(&self) -> Option<Arc<NoIrqRwLock<SignalDisposition>>> {
        let mut disp = None;
        self.for_each_member(|member| {
            disp = Some(member.sig_disposition.clone());
            return;
        });
        disp
    }

    /// Return a snapshot of this thread group's shared pending signal set.
    pub fn shared_pending_signal_set(&self) -> SigSet {
        self.inner.read().sig_pending.lock().to_sigset()
    }

    /// Send a signal to this thread group.
    ///
    /// If the disposition of the signal satisfies [SignalAction::is_ignored],
    /// the signal won't be delivered.
    pub fn recv_signal(&self, signal: Signal) {
        // kspecialln!("{} -> {:?}", self.tgid(), signal.no);
        let no = signal.no;

        let disp = {
            let Some(disps) = self.signal_disposition() else {
                knoticeln!(
                    "trying to send signal {:?} to a thread group with no members",
                    no
                );

                return;
            };
            disps.read().get_disposition(no)
        };

        if disp.action.is_ignored() {
            kdebugln!(
                "signal {:?} is ignored by the thread group, not queuing",
                no
            );
            return;
        }

        {
            let inner = self.inner.write();
            inner.sig_pending.lock().push_signal(signal);
        }

        self.for_each_member(|member| {
            if member.is_current_sig_mask_blocking(no)
                && !matches!(no, SigNo::SIGKILL | SigNo::SIGSTOP)
            {
                // signal masked. nothing to do now, just wait for the task to
                // unmask it.
            } else {
                notify(
                    member,
                    if matches!(no, SigNo::SIGKILL | SigNo::SIGSTOP) {
                        true
                    } else {
                        false
                    },
                );
            }
        });
    }

    /// Flush pending signals in the given set.
    ///
    /// Both each member's private pending signals and shared pending signals
    /// will be flushed.
    ///
    /// TODO: [SignalDisposition] actually can be shared by multiple thread
    /// groups. currently we restrict clone's flags to avoid this. But later we
    /// should support that, and this method won't be put in [ThreadGroup].
    pub fn flush_specific_signals(&self, set: SigSet) {
        {
            let inner = self.inner.write();
            let mut pending = inner.sig_pending.lock();
            pending.flush_specific(set);
        }
        self.for_each_member(|member| {
            member.sig_pending.lock().flush_specific(set);
        });
    }
}

/// **Called by trap handling code when returning to user-space.**
///
/// Internally a loop for handling pending signals.
///
/// Since we support kernel preemption, it seems no need to limit how many
/// signals we handle in one go? idk.
pub fn handle_signals(
    trapframe: &mut TrapFrame,
    mut restart_syscall: Option<(RestartSyscall, SyscallCtx)>,
) {
    let mut committed_handler_frame = false;
    loop {
        if let Some(signal) = get_current_task().fetch_signal() {
            if perform_signal_action(signal, trapframe, &mut restart_syscall) {
                committed_handler_frame = true;
                break;
            }
        } else {
            break;
        }
    }

    // Ignored signals do not leave an rt_sigreturn frame behind. If this
    // trap-return pass consumed no handler signal, signal code remains the
    // owner responsible for closing any deferred temporary-mask restore.
    if !committed_handler_frame {
        get_current_task().restore_temporary_sig_mask_if_pending();
    }
}

/// Return `true` if the signal handler is a user-defined handler and we just
/// prepare the signal frame, then get into the handler in the common
/// trap-return path.
fn perform_signal_action(
    signal: Signal,
    trapframe: &mut TrapFrame,
    restart_syscall: &mut Option<(RestartSyscall, SyscallCtx)>,
) -> bool {
    let mut break_loop = false;

    let no = signal.no;
    let task = get_current_task();
    let KSigAction {
        action,
        flags,
        restorer,
        mask,
    } = task.sig_disposition.read().get_disposition(no);

    match action {
        SignalAction::Default(default) => {
            #[cfg(feature = "bench_local_test")]
            if no == SigNo::SIGKILL {
                match &signal.fields {
                    SigInfoFields::Kill(killer) | SigInfoFields::TKill(killer) => {
                        kerrln!(
                            "[special_report] sigkill terminate target_tid={} target_tgid={} target_name=\"{}\" killer_tgid={} killer_uid={} si_code={:?}",
                            task.tid(),
                            task.tgid(),
                            task.name(),
                            killer.pid,
                            killer.uid.get(),
                            signal.code
                        );
                    },
                    fields => {
                        kerrln!(
                            "[special_report] sigkill terminate target_tid={} target_tgid={} target_name=\"{}\" killer=unknown si_code={:?} fields={:?}",
                            task.tid(),
                            task.tgid(),
                            task.name(),
                            signal.code,
                            fields
                        );
                    },
                }
            }
            drop(task);
            default(no);
            return false;
        },
        SignalAction::Ignore => {
            // do nothing.
        },
        SignalAction::Custom(handler_addr) => {
            let mask_to_save = task.sigmask_to_save_for_signal_frame();
            task.mutate_current_sig_mask_for_signal_delivery(|sig_mask| {
                assert!(
                    !mask.get(SigNo::SIGKILL) && !mask.get(SigNo::SIGSTOP),
                    "SIGKILL and SIGSTOP cannot be masked"
                );
                sig_mask.union_with(&mask);
                if !flags.contains(SaFlags::NODEFER) {
                    sig_mask.set(no);
                }
            });

            // if SA_ONSTACK is set, and altstack is configured:
            // - if we're not currently on the altstack, use the altstack.
            // - if we're already on the altstack, we have a reentrancy. we just push
            //   sigframe onto currently-using stack.

            let (altstack, init_sp) = {
                let curr_sp = trapframe.sp();
                let altstack = *task.sig_altstack.lock();
                if let Some(altstack) = altstack {
                    let mut ss = altstack.to_linux_sigstack();
                    if flags.contains(SaFlags::ONSTACK) {
                        if altstack.contains_addr(VirtAddr::new(curr_sp)) {
                            // we're already on altstack. this is a reentrancy. continue to use this
                            // altstack.
                            ss.ss_flags |= linux_signal::SS_ONSTACK;
                            (ss, curr_sp)
                        } else {
                            // first time to use altstack.
                            (ss, ss.ss_sp as u64 + ss.ss_size as u64)
                        }
                    } else {
                        // SA_ONSTACK is not set but altstack is configured.
                        (ss, curr_sp)
                    }
                } else {
                    // altstack not configured. just use current stack.
                    (
                        linux_signal::SigStack {
                            ss_sp: 0 as *mut u8,
                            ss_flags: linux_signal::SS_DISABLE,
                            ss_size: 0,
                        },
                        curr_sp,
                    )
                }
            };

            let mut ucontext = UContext::ZEROED;

            if flags.contains(SaFlags::RESTART) {
                // note the take. this ensures only the first signal handler with SA_RESTART can
                // restart the syscall.
                if let Some((restart, syscall_ctx)) = restart_syscall.take() {
                    match restart {
                        RestartSyscall::Idempotent => {
                            kdebugln!("restarting syscall: sysno = {}", syscall_ctx.syscall_no());

                            TrapArch::restore_syscall_ctx(trapframe, &syscall_ctx);
                            // arguments are still in registers, and will be
                            // encoded into ucontext.
                        },
                    }
                }

                // this must be done before encoding ucontext.

                // no need to set break_loop, since all user-defined handlers
                // will set that anyway.
            }

            SignalArch::encode_ucontext(
                &mut ucontext,
                trapframe,
                mask_to_save,
                altstack,
                task.fpu_used(),
            );

            // construct signal frame on user stack.
            let frame = RtSigFrame {
                siginfo: signal.to_linux_siginfo(),
                ucontext,
            };

            let sigframe_base = VirtAddr::new(align_down_power_of_2!(
                init_sp - size_of::<RtSigFrame>() as u64,
                // 16 bytes should be enough for all architectures.
                16
            ) as u64);
            let write_sigframe = {
                let usp = task.clone_uspace_handle();
                let mut guard = usp.lock();
                match UserWritePtr::<RtSigFrame>::try_new(sigframe_base, &mut guard) {
                    Ok(mut uptr) => {
                        uptr.write(frame);
                        Ok(())
                    },
                    Err(e) => Err(e),
                }
            };
            if let Err(e) = write_sigframe {
                knoticeln!(
                    "perform_signal_action: failed to write sigframe to {} user stack: {:?}",
                    task.tid(),
                    e
                );
                kernel_exit_group(ExitCode::Signaled(SigNo::SIGSEGV))
            }

            // we're done. finally prepare the trapframe.
            SignalArch::prepare_trapframe_for_signal_handler(
                trapframe,
                no,
                handler_addr,
                sigframe_base,
            );

            // place this after preparing trapframe for overwriting.
            if flags.contains(SaFlags::RESTORER) {
                trapframe.set_return_addr(restorer.get());
            }

            task.signal_frame_committed_restore_mask();
            break_loop = true;
        },
    }

    if flags.contains(SaFlags::ONESHOT) {
        task.sig_disposition.write().set_to_default(no);
    }

    break_loop
}
