use crate::{
    fs::{PollEvent, PollRegisterResult, PollRequest},
    prelude::*,
    task::files::OpenedFileFinalReleaseCtx,
};

use super::super::{
    TtyProgress,
    discipline::{InputRead, TtySignalControl},
    file::TtyOperation,
    port::TtyRxUnit,
    terminal::Terminal,
};

pub(super) const DESCRIPTION_PREPARED: u8 = 0;
const DESCRIPTION_LIVE: u8 = 1;
const DESCRIPTION_RELEASED: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PairPhase {
    Prepared,
    Live,
    Retired,
}

struct PairInner {
    phase: PairPhase,
    slave_descriptions: usize,
}

/// Sole owner of one PTY episode's liveness and peer participation.
///
/// `operation` only orders a bounded Terminal commit against description
/// release. It is not state truth and is never held while an operation waits.
pub(crate) struct PtyPairState {
    terminal: Arc<Terminal>,
    operation: Mutex<()>,
    inner: SpinLock<PairInner>,
}

impl PtyPairState {
    pub(super) fn try_new(terminal: Arc<Terminal>) -> Result<Arc<Self>, SysError> {
        Arc::try_new(Self {
            terminal,
            operation: Mutex::new(()),
            inner: SpinLock::new(PairInner {
                phase: PairPhase::Prepared,
                slave_descriptions: 0,
            }),
        })
        .map_err(|_| SysError::OutOfMemory)
    }

    pub(super) fn commit_master(&self, description_phase: &AtomicU8) {
        let _operation = self.operation.lock();
        let mut inner = self.inner.lock();
        assert_eq!(
            inner.phase,
            PairPhase::Prepared,
            "PTY master description committed outside pair prepare"
        );
        assert_eq!(
            description_phase.compare_exchange(
                DESCRIPTION_PREPARED,
                DESCRIPTION_LIVE,
                Ordering::AcqRel,
                Ordering::Acquire,
            ),
            Ok(DESCRIPTION_PREPARED),
            "PTY master description committed more than once"
        );
        inner.phase = PairPhase::Live;
    }

    pub(super) fn commit_slave(&self, description_phase: &AtomicU8) -> Result<(), SysError> {
        {
            let _operation = self.operation.lock();
            let mut inner = self.inner.lock();
            if inner.phase != PairPhase::Live {
                return Err(SysError::IO);
            }
            assert_eq!(
                description_phase.compare_exchange(
                    DESCRIPTION_PREPARED,
                    DESCRIPTION_LIVE,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ),
                Ok(DESCRIPTION_PREPARED),
                "PTY slave description committed more than once"
            );
            inner.slave_descriptions = inner
                .slave_descriptions
                .checked_add(1)
                .expect("PTY slave description count overflow");
        }
        Ok(())
    }

    fn release_slave(&self, description_phase: &AtomicU8) {
        {
            let _operation = self.operation.lock();
            if description_phase.swap(DESCRIPTION_RELEASED, Ordering::AcqRel) != DESCRIPTION_LIVE {
                return;
            }
            let mut inner = self.inner.lock();
            assert!(
                inner.slave_descriptions != 0,
                "PTY slave description count underflow"
            );
            inner.slave_descriptions -= 1;
            let peer_became_absent =
                inner.phase == PairPhase::Live && inner.slave_descriptions == 0;
            drop(inner);
            // Linux discards the now-unconsumable slave input at the last slave
            // close, while master-readable slave output keeps buffered precedence.
            // Keep a reopening description behind the input flush so the old
            // last-close episode cannot discard bytes admitted for the new peer.
            if peer_became_absent {
                self.terminal.pty_peer_absent();
            }
        }
        self.notify_state_change();
    }

    fn retire_master(&self, description_phase: &AtomicU8) {
        {
            let _operation = self.operation.lock();
            if description_phase.swap(DESCRIPTION_RELEASED, Ordering::AcqRel) != DESCRIPTION_LIVE {
                return;
            }
            let mut inner = self.inner.lock();
            assert_eq!(
                inner.phase,
                PairPhase::Live,
                "PTY master description released outside live pair"
            );
            inner.phase = PairPhase::Retired;
        }
        // Pair retirement is already irreversible before Terminal cleanup.
        // Flush and wake happen with no pair guard held.
        self.terminal.pty_hangup();
        self.notify_state_change();
    }

    pub(super) fn live_slave_count(&self) -> Option<usize> {
        let inner = self.inner.lock();
        (inner.phase == PairPhase::Live).then_some(inner.slave_descriptions)
    }

    pub(super) fn is_retired(&self) -> bool {
        self.inner.lock().phase == PairPhase::Retired
    }

    pub(super) fn wait_until(&self, predicate: impl Fn() -> bool) -> Result<(), SysError> {
        self.terminal.wait_for_progress(predicate)
    }

    pub(super) fn register_poll_route(&self, request: &PollRequest<'_>) -> bool {
        self.terminal.register_progress_route(request)
    }

    pub(super) fn notify_state_change(&self) {
        if self.terminal.drain_check_pending() && !self.terminal.output_pending() {
            self.terminal.complete_drain_if(true);
        }
        // Pair predicates share the Terminal's existing progress channel. The
        // route registry is notification storage only; readiness and lifecycle
        // truth remain in Terminal and pair respectively.
        self.terminal.publish_progress();
    }
}

impl TtyProgress for PtyPairState {
    fn wake(&self) {
        self.notify_state_change();
    }
}

pub(super) struct PtyMasterDescription {
    pair: Arc<PtyPairState>,
    pub(super) phase: AtomicU8,
    pub(super) base_final_release: Option<for<'a> fn(OpenedFileFinalReleaseCtx<'a>)>,
}

impl PtyMasterDescription {
    pub(super) fn new(
        pair: Arc<PtyPairState>,
        base_final_release: Option<for<'a> fn(OpenedFileFinalReleaseCtx<'a>)>,
    ) -> Self {
        Self {
            pair,
            phase: AtomicU8::new(DESCRIPTION_PREPARED),
            base_final_release,
        }
    }

    pub(super) fn is_live(&self) -> bool {
        self.phase.load(Ordering::Acquire) == DESCRIPTION_LIVE && !self.pair.is_retired()
    }

    pub(super) fn release(&self) {
        self.pair.retire_master(&self.phase);
    }

    pub(super) fn read(&self, dst: &mut [u8], ctx: FileIoCtx) -> Result<usize, SysError> {
        loop {
            let outcome = {
                let _operation = self.pair.operation.lock();
                if !self.is_live() {
                    return Err(SysError::IO);
                }
                let count = self.pair.terminal.read_output(dst);
                if count != 0 {
                    Some(Ok(count))
                } else if self.pair.live_slave_count() == Some(0) {
                    Some(Err(SysError::IO))
                } else {
                    None
                }
            };
            if let Some(outcome) = outcome {
                self.pair.notify_state_change();
                return outcome;
            }
            if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) {
                return Err(SysError::Again);
            }
            self.pair.wait_until(|| {
                !self.is_live()
                    || self.pair.terminal.output_pending()
                    || self.pair.live_slave_count() == Some(0)
            })?;
        }
    }

    pub(super) fn write(
        &self,
        source: &[u8],
        ctx: FileIoCtx,
        mut signal_foreground: impl FnMut(TtySignalControl) -> bool,
    ) -> Result<usize, SysError> {
        let mut written = 0;
        while written < source.len() {
            let effect = {
                let _operation = self.pair.operation.lock();
                if !self.is_live() {
                    return if written == 0 {
                        Err(SysError::IO)
                    } else {
                        Ok(written)
                    };
                }
                if self.pair.live_slave_count() == Some(0) {
                    // Linux accepts master writes while the slave has no opened
                    // description, but the bytes are not retained for a later
                    // reopen. This is an intentional ABI result, not a success stub.
                    return Ok(source.len());
                }
                self.pair
                    .terminal
                    .receive_pty_rx_unit_effect(TtyRxUnit::Byte(source[written]))
            };
            if effect.consumed() {
                written += 1;
                self.pair.notify_state_change();
                if let Some(signal) = effect.signal()
                    && !signal_foreground(signal)
                {
                    self.pair.terminal.record_no_foreground_input_signal();
                }
                continue;
            }
            if written != 0 {
                return Ok(written);
            }
            if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) {
                return Err(SysError::Again);
            }
            let byte = source[written];
            self.pair.wait_until(|| {
                !self.is_live()
                    || self.pair.live_slave_count() == Some(0)
                    || self
                        .pair
                        .terminal
                        .can_receive_rx_unit(TtyRxUnit::Byte(byte))
            })?;
        }
        Ok(written)
    }

    pub(super) fn poll(&self, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
        let interests = request.interests();
        if request.is_register() {
            if !self.pair.register_poll_route(request) {
                return Ok(PollRegisterResult::Unsupported);
            }
            // See the slave register path: the installed route makes a mixed owner
            // snapshot a recheck hint, never the final readiness decision.
            return Ok(PollRegisterResult::Subscribed(self.poll_events(interests)));
        }

        // Final snapshots run after the iomux wait round is retired and may take
        // the operation mutex to serialize lifecycle with Terminal cleanup.
        let _operation = self.pair.operation.lock();
        Ok(PollRegisterResult::Ready(self.poll_events(interests)))
    }

    fn poll_events(&self, interests: PollEvent) -> PollEvent {
        let mut ready = PollEvent::empty();
        let peer = self.pair.live_slave_count();
        if !self.is_live() {
            ready |= PollEvent::ERROR | PollEvent::HANG_UP;
            ready |= interests & (PollEvent::READABLE | PollEvent::WRITABLE);
        } else {
            if interests.contains(PollEvent::READABLE) && self.pair.terminal.output_pending() {
                ready |= PollEvent::READABLE;
            }
            if interests.contains(PollEvent::WRITABLE)
                && (peer == Some(0) || self.pair.terminal.input_writable())
            {
                ready |= PollEvent::WRITABLE;
            }
            if peer == Some(0) {
                ready |= PollEvent::HANG_UP;
            }
        }
        ready
    }
}

impl TtyOperation for PtyMasterDescription {
    fn run_if_live(&self, operation: &mut dyn FnMut()) -> Result<(), SysError> {
        let _operation = self.pair.operation.lock();
        if !self.is_live() {
            return Err(SysError::IO);
        }
        operation();
        Ok(())
    }
}

#[derive(Opaque)]
pub(in crate::device::tty) struct PtySlaveDescription {
    pub(super) pair: Arc<PtyPairState>,
    pub(super) phase: AtomicU8,
    pub(super) base_final_release: Option<for<'a> fn(OpenedFileFinalReleaseCtx<'a>)>,
}

impl PtySlaveDescription {
    fn phase_is_live(&self) -> bool {
        self.phase.load(Ordering::Acquire) == DESCRIPTION_LIVE
    }

    pub(in crate::device::tty) fn is_released_or_hung_up(&self) -> bool {
        !self.phase_is_live() || self.pair.is_retired()
    }

    pub(in crate::device::tty) fn read(
        &self,
        terminal: &Terminal,
        dst: &mut [u8],
        ctx: FileIoCtx,
    ) -> Result<usize, SysError> {
        loop {
            let result = {
                let _operation = self.pair.operation.lock();
                if !self.phase_is_live() || self.pair.is_retired() {
                    return Ok(0);
                }
                terminal.read_pty_input(dst)
            };
            match result {
                InputRead::Bytes(count) => {
                    self.pair.notify_state_change();
                    return Ok(count);
                },
                InputRead::Eof => {
                    self.pair.notify_state_change();
                    return Ok(0);
                },
                InputRead::Empty if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) => {
                    return Err(SysError::Again);
                },
                InputRead::Empty => self
                    .pair
                    .wait_until(|| self.is_released_or_hung_up() || terminal.readable())?,
            }
        }
    }

    pub(in crate::device::tty) fn write(
        &self,
        terminal: &Terminal,
        source: &[u8],
        ctx: FileIoCtx,
    ) -> Result<usize, SysError> {
        loop {
            let written = {
                let _operation = self.pair.operation.lock();
                if !self.phase_is_live() || self.pair.is_retired() {
                    return Err(SysError::IO);
                }
                terminal.enqueue_pty_output(source)
            };
            if written != 0 {
                self.pair.notify_state_change();
                return Ok(written);
            }
            if ctx.status_flags().contains(FileOpStatusFlags::NONBLOCK) {
                return Err(SysError::Again);
            }
            self.pair
                .wait_until(|| self.is_released_or_hung_up() || terminal.writable())?;
        }
    }

    pub(in crate::device::tty) fn poll(
        &self,
        terminal: &Terminal,
        request: &PollRequest<'_>,
    ) -> PollRegisterResult {
        if request.is_register() {
            if !self.pair.register_poll_route(request) {
                return PollRegisterResult::Unsupported;
            }
            // Registration runs inside an active scheduler wait and cannot take
            // the sleepable operation mutex. The route is installed first, so
            // any lifecycle/data-plane transition crossing these owner snapshots
            // pretriggers the round and forces a consistent final snapshot.
            return PollRegisterResult::Subscribed(self.poll_events(terminal, request.interests()));
        }

        // Snapshot scans run outside an active wait. Serialize pair lifecycle
        // with Terminal cleanup so the result cannot combine pre-retirement
        // liveness with post-retirement flushed buffers.
        let _operation = self.pair.operation.lock();
        PollRegisterResult::Ready(self.poll_events(terminal, request.interests()))
    }

    fn poll_events(&self, terminal: &Terminal, interests: PollEvent) -> PollEvent {
        if self.is_released_or_hung_up() {
            (interests & (PollEvent::READABLE | PollEvent::WRITABLE))
                | PollEvent::ERROR
                | PollEvent::HANG_UP
        } else {
            terminal
                .poll(&PollRequest::snapshot(interests))
                .expect_ready("PTY slave Terminal snapshot")
        }
    }

    pub(super) fn release(&self) {
        self.pair.release_slave(&self.phase);
    }
}

impl TtyOperation for PtySlaveDescription {
    fn run_if_live(&self, operation: &mut dyn FnMut()) -> Result<(), SysError> {
        let _operation = self.pair.operation.lock();
        if self.is_released_or_hung_up() {
            return Err(SysError::IO);
        }
        operation();
        Ok(())
    }
}
