use crate::{
    fs::{PollEvent, PollRegisterResult, PollRequest},
    prelude::*,
    task::files::OpenedFileFinalReleaseCtx,
};

use super::{
    super::{
        TtyProgress,
        discipline::{InputRead, TtySignalControl},
        file::TtyOperation,
        port::TtyRxUnit,
        relation,
        terminal::Terminal,
    },
    PtyBindingCapability,
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
    slave_locked: bool,
    slave_descriptions: usize,
    /// Number of live, non-cloneable bounded-effect permits.
    active_effects: usize,
}

/// Sole owner of one PTY episode's liveness and peer participation.
///
/// `operation` orders bounded Terminal commits and effect admission against
/// description release. It is not state truth and is never held while an
/// operation waits.
pub(crate) struct PtyPairState {
    terminal: Arc<Terminal>,
    operation: Mutex<()>,
    inner: SpinLock<PairInner>,
    /// Notification storage only; `inner.active_effects` is the drain truth.
    effects_drained: Event,
}

/// Proof that one bounded guards-out PTY effect committed before retirement.
///
/// The permit carries no liveness truth and must not cross a blocking wait.
/// Retirement blocks new permits at the pair owner, then waits for the old
/// permits before publishing its hangup effects.
pub(in crate::device::tty) struct PtyEffectPermit {
    pair: Arc<PtyPairState>,
}

impl PtyPairState {
    pub(super) fn try_new(terminal: Arc<Terminal>) -> Result<Arc<Self>, SysError> {
        Arc::try_new(Self {
            terminal,
            operation: Mutex::new(()),
            inner: SpinLock::new(PairInner {
                phase: PairPhase::Prepared,
                slave_locked: true,
                slave_descriptions: 0,
                active_effects: 0,
            }),
            effects_drained: Event::new(),
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
            if inner.slave_locked {
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

    fn commit_master_retirement(&self, description_phase: &AtomicU8) -> bool {
        {
            let _operation = self.operation.lock();
            if description_phase.swap(DESCRIPTION_RELEASED, Ordering::AcqRel) != DESCRIPTION_LIVE {
                return false;
            }
            let mut inner = self.inner.lock();
            assert_eq!(
                inner.phase,
                PairPhase::Live,
                "PTY master description released outside live pair"
            );
            inner.phase = PairPhase::Retired;
        }
        true
    }

    fn retire_master(&self, description_phase: &AtomicU8) -> bool {
        if !self.commit_master_retirement(description_phase) {
            return false;
        }
        // Pair retirement is already irreversible before Terminal cleanup.
        // Buffer cleanup happens with no pair guard held; the master owner
        // sends the final waiter hint only after cross-owner hangup effects.
        self.terminal.pty_hangup();
        true
    }

    fn run_effect_if_live(
        self: &Arc<Self>,
        description_phase: &AtomicU8,
        operation: &mut dyn FnMut(),
    ) -> Result<PtyEffectPermit, SysError> {
        let _operation = self.operation.lock();
        if description_phase.load(Ordering::Acquire) != DESCRIPTION_LIVE {
            return Err(SysError::IO);
        }
        let mut inner = self.inner.lock();
        if inner.phase != PairPhase::Live {
            return Err(SysError::IO);
        }
        inner.active_effects = inner
            .active_effects
            .checked_add(1)
            .expect("PTY bounded effect count overflow");
        let permit = PtyEffectPermit { pair: self.clone() };
        drop(inner);
        operation();
        Ok(permit)
    }

    fn wait_effects_drained(&self) {
        if self.inner.lock().active_effects == 0 {
            return;
        }
        self.effects_drained
            .listen_uninterruptible(false, || self.inner.lock().active_effects == 0);
    }

    pub(super) fn live_slave_count(&self) -> Option<usize> {
        let inner = self.inner.lock();
        (inner.phase == PairPhase::Live).then_some(inner.slave_descriptions)
    }

    pub(super) fn is_retired(&self) -> bool {
        self.inner.lock().phase == PairPhase::Retired
    }

    pub(super) fn slave_locked(&self) -> bool {
        self.inner.lock().slave_locked
    }

    pub(super) fn set_slave_locked(&self, locked: bool) -> Result<(), SysError> {
        let _operation = self.operation.lock();
        let mut inner = self.inner.lock();
        if inner.phase != PairPhase::Live {
            return Err(SysError::IO);
        }
        inner.slave_locked = locked;
        Ok(())
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

impl Drop for PtyEffectPermit {
    fn drop(&mut self) {
        let drained = {
            let mut inner = self.pair.inner.lock();
            assert!(
                inner.active_effects != 0,
                "PTY bounded effect count underflow"
            );
            inner.active_effects -= 1;
            inner.active_effects == 0
        };
        if drained {
            self.pair.effects_drained.publish(usize::MAX, true);
        }
    }
}

impl TtyProgress for PtyPairState {
    fn wake(&self) {
        self.notify_state_change();
    }
}

pub(super) struct PtyMasterDescription {
    pair: Arc<PtyPairState>,
    index: u32,
    pub(super) phase: AtomicU8,
    /// Outer `None` means the creation-time hook has not been composed yet;
    /// the inner value is the pre-existing owner hook, if any.
    pub(super) base_final_release:
        SpinLock<Option<Option<for<'a> fn(OpenedFileFinalReleaseCtx<'a>)>>>,
    cleanup: SpinLock<Option<PtyMasterCleanup>>,
}

struct PtyMasterCleanup {
    binding: PtyBindingCapability,
    relation: relation::RelationParticipant,
}

impl PtyMasterDescription {
    pub(super) fn new(pair: Arc<PtyPairState>, index: u32) -> Self {
        Self {
            pair,
            index,
            phase: AtomicU8::new(DESCRIPTION_PREPARED),
            base_final_release: SpinLock::new(None),
            cleanup: SpinLock::new(None),
        }
    }

    pub(super) fn install_cleanup(
        &self,
        binding: PtyBindingCapability,
        participant: relation::RelationParticipant,
    ) {
        let old = self.cleanup.lock().replace(PtyMasterCleanup {
            binding,
            relation: participant,
        });
        assert!(old.is_none(), "PTY master cleanup installed more than once");
    }

    pub(super) fn abort_prepared_cleanup(&self) {
        assert_eq!(
            self.phase.load(Ordering::Acquire),
            DESCRIPTION_PREPARED,
            "live PTY master cleanup cannot be aborted"
        );
        let cleanup = self
            .cleanup
            .lock()
            .take()
            .expect("prepared PTY master missing installed cleanup");
        // Relation and binding capabilities may run cross-owner destructors;
        // the master cleanup slot is already undiscoverable before they drop.
        drop(cleanup);
    }

    pub(super) fn index(&self) -> u32 {
        self.index
    }

    pub(super) fn slave_locked(&self) -> bool {
        self.pair.slave_locked()
    }

    pub(super) fn set_slave_locked(&self, locked: bool) -> Result<(), SysError> {
        self.pair.set_slave_locked(locked)
    }

    pub(super) fn binding(&self) -> Option<PtyBindingCapability> {
        self.cleanup
            .lock()
            .as_ref()
            .map(|cleanup| cleanup.binding.clone())
    }

    pub(super) fn pair(&self) -> Arc<PtyPairState> {
        self.pair.clone()
    }

    pub(super) fn is_live(&self) -> bool {
        self.phase.load(Ordering::Acquire) == DESCRIPTION_LIVE && !self.pair.is_retired()
    }

    pub(super) fn release(&self) {
        if !self.pair.retire_master(&self.phase) {
            return;
        }
        let cleanup = self
            .cleanup
            .lock()
            .take()
            .expect("live PTY master missing static cleanup capability");
        cleanup.binding.retire();
        let hangup = cleanup.relation.retire_for_hangup();
        // Relation discoverability is gone before waiting, so a pre-retirement
        // operation can finish only the effect for which it already holds a
        // permit. Hangup SIGHUP/SIGCONT are published after all such effects.
        self.pair.wait_effects_drained();
        if let Some(effect) = hangup {
            effect.deliver();
        }
        self.pair.notify_state_change();
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
            let mut effect = None;
            let permit = match self.run_effect_if_live(&mut || {
                if self.pair.live_slave_count() != Some(0) {
                    effect = Some(
                        self.pair
                            .terminal
                            .receive_pty_rx_unit_effect(TtyRxUnit::Byte(source[written])),
                    );
                }
            }) {
                Ok(permit) => permit,
                Err(error) if written == 0 => return Err(error),
                Err(_) => return Ok(written),
            };
            let Some(effect) = effect else {
                // Linux accepts master writes while the slave has no opened
                // description, but the bytes are not retained for a later
                // reopen. This is an intentional ABI result, not a success stub.
                return Ok(source.len());
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
            // A bounded effect permit must never survive into a capacity wait.
            drop(permit);
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

    fn run_effect_if_live(&self, operation: &mut dyn FnMut()) -> Result<PtyEffectPermit, SysError> {
        self.pair.run_effect_if_live(&self.phase, operation)
    }
}

#[derive(Opaque)]
pub(in crate::device::tty) struct PtySlaveDescription {
    pub(super) pair: Arc<PtyPairState>,
    pub(super) phase: AtomicU8,
    /// Outer `None` means the creation-time hook has not been composed yet;
    /// the inner value is the pre-existing owner hook, if any.
    pub(super) base_final_release:
        SpinLock<Option<Option<for<'a> fn(OpenedFileFinalReleaseCtx<'a>)>>>,
}

impl PtySlaveDescription {
    fn phase_is_live(&self) -> bool {
        self.phase.load(Ordering::Acquire) == DESCRIPTION_LIVE
    }

    pub(in crate::device::tty) fn is_released_or_hung_up(&self) -> bool {
        !self.phase_is_live() || self.pair.is_retired()
    }

    pub(in crate::device::tty) fn read_once(
        &self,
        terminal: &Terminal,
        dst: &mut [u8],
    ) -> InputRead {
        let result = {
            let _operation = self.pair.operation.lock();
            if !self.phase_is_live() || self.pair.is_retired() {
                return InputRead::Eof;
            }
            terminal.read_pty_input(dst)
        };
        if result != InputRead::Empty {
            self.pair.notify_state_change();
        }
        result
    }

    pub(in crate::device::tty) fn wait_readable(
        &self,
        terminal: &Terminal,
    ) -> Result<(), SysError> {
        self.pair
            .wait_until(|| self.is_released_or_hung_up() || terminal.readable())
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

    fn run_effect_if_live(&self, operation: &mut dyn FnMut()) -> Result<PtyEffectPermit, SysError> {
        self.pair.run_effect_if_live(&self.phase, operation)
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::device::tty::port::{TtyLineSnapshot, TtyParity};

    fn live_pair() -> (Arc<PtyPairState>, AtomicU8) {
        let terminal = Terminal::try_new(TtyLineSnapshot {
            baud: 115200,
            parity: TtyParity::None,
            data_bits: 8,
        })
        .unwrap();
        let pair = PtyPairState::try_new(terminal).unwrap();
        let phase = AtomicU8::new(DESCRIPTION_PREPARED);
        pair.commit_master(&phase);
        (pair, phase)
    }

    #[kunit]
    fn effect_permit_first_blocks_new_effects_until_old_permit_drains() {
        let (pair, master_phase) = live_pair();
        let mut ran = false;
        let permit = pair
            .run_effect_if_live(&master_phase, &mut || ran = true)
            .unwrap();
        assert!(ran);
        assert_eq!(pair.inner.lock().active_effects, 1);

        assert!(pair.commit_master_retirement(&master_phase));
        let still_live_description = AtomicU8::new(DESCRIPTION_LIVE);
        assert!(
            pair.run_effect_if_live(&still_live_description, &mut || {})
                .is_err()
        );
        assert_eq!(pair.inner.lock().active_effects, 1);

        drop(permit);
        assert_eq!(pair.inner.lock().active_effects, 0);
    }

    #[kunit]
    fn retirement_first_denies_effect_permit() {
        let (pair, master_phase) = live_pair();
        assert!(pair.commit_master_retirement(&master_phase));
        let still_live_description = AtomicU8::new(DESCRIPTION_LIVE);
        assert!(
            pair.run_effect_if_live(&still_live_description, &mut || {})
                .is_err()
        );
        assert_eq!(pair.inner.lock().active_effects, 0);
    }
}
