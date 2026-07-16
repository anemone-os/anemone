//! Protocol-neutral DW-MSHC controller execution engine.
//!
//! This layer owns MMIO sequencing, IOS application, bounded polling, PIO,
//! and controller recovery. It deliberately does not decide whether the card
//! protocol is SD Memory, MMC/eMMC, or SDIO.

use super::regs::*;
use crate::{device::mmc::*, mm::remap::IoRemap, prelude::*};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Controller-local readiness owned with the register engine. Card discovery
/// must not create a parallel ready/failed state for the same hardware.
enum ControllerReadiness {
    /// Registers have been mapped but the reset baseline is not committed.
    Probing,
    /// Requests and IOS updates may be accepted.
    Ready,
    /// A submitted transaction failed; only explicit transport recovery may
    /// reopen the host. Card protocol state must be rebuilt afterwards.
    RecoveryRequired,
    /// An explicit recovery call is rebuilding the controller baseline.
    Recovering,
    /// Recovery failed; only an explicit recovery attempt may revive it.
    Offline,
}

/// Sole owner of the DW-MSHC register block and mutable request state.
pub(super) struct DwMshcController {
    // Stage-2 synchronous serialization: with spin_lock_irqsave enabled this
    // keeps local interrupts disabled across one bounded command/polling/PIO
    // transaction. This is the current cold-boot/single-block contract, and
    // must be replaced before IRQ/DMA, multiple outstanding requests,
    // cancellable I/O, SDIO interrupts, or runtime hotplug.
    inner: SpinLock<DwMshcInner>,
}

/// One slot-facing implementation of the generic `MmcHost` contract.
///
/// The JH7110 instances currently expose only slot zero, but keeping this
/// facade separate prevents a future multi-slot controller from duplicating
/// the MMIO owner.
pub(super) struct DwMshcHost {
    controller: Arc<DwMshcController>,
    slot: u8,
    caps: MmcHostCaps,
}

/// Mutable state that must never have more than one behavioral owner.
struct DwMshcInner {
    /// Owns the mapping; raw register/FIFO pointers are derived transiently.
    regs: DwMshcRegs,
    /// Stable synthesis/integration snapshot established during probe.
    layout: DwMshcLayout,
    ciu_clock_hz: u32,
    /// Last fully committed hardware IOS. Failed partial transactions do not
    /// update this snapshot; recovery reapplies it from the reset baseline.
    applied_ios: [MmcIos; 1],
    readiness: ControllerReadiness,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Divider selection and the non-overclocking frequency it actually produces.
struct ClockSetting {
    divider: u8,
    actual_hz: u32,
}

#[derive(Clone, Copy)]
/// Per-slot bit positions shared by PWREN, CLKENA, and CTYPE. Keeping these
/// encodings together prevents call sites from composing anonymous shifts.
struct SlotRegisterFields {
    enable: u32,
    card_type_mask: u32,
    four_bit_bus: u32,
    eight_bit_bus: u32,
}

impl SlotRegisterFields {
    fn for_slot(slot: u8) -> Self {
        assert!(slot < 16, "DW-MSHC CTYPE encodes at most 16 slots");
        let enable = 1u32 << slot;
        let eight_bit_bus = 1u32 << (16 + slot);
        Self {
            enable,
            card_type_mask: enable | eight_bit_bus,
            four_bit_bus: enable,
            eight_bit_bus,
        }
    }
}

#[derive(Clone, Copy)]
/// Whether an internal clock-update command may run immediately or must wait
/// for the preceding data path to become idle.
enum ClockUpdateOrder {
    Immediate,
    AfterPreviousData,
}

#[derive(Clone, Copy, Debug)]
/// Immutable identity snapshot used for publication logs and review evidence.
/// Behavioral register access continues through `DwMshcInner::regs`.
pub(super) struct DwMshcIdentity {
    pub resource_base: PhysAddr,
    pub resource_len: usize,
    pub layout: DwMshcLayout,
}

impl DwMshcController {
    /// Validate controller layout, reset it to a powered-off polling baseline,
    /// and only then construct the serialized owner. No card command is sent.
    pub fn probe(
        remap: IoRemap,
        fifo_depth: u32,
        data_addr: Option<usize>,
        ciu_clock_hz: u32,
    ) -> Result<(Self, DwMshcIdentity), SysError> {
        // step 1: Reject malformed firmware resources before deriving any
        // register pointer, then read synthesis state before reset changes the
        // controller's operational state.
        let resource_len = remap.size() as usize;
        let regs = DwMshcRegs::new(remap).map_err(|error| {
            kerrln!(
                "dw-mshc: invalid MMIO mapping: error={:?}, resource_len={:#x}, required_len={:#x}",
                error,
                resource_len,
                DwMshcRegs::BASELINE_MAPPING_LEN
            );
            SysError::DriverIncompatible
        })?;
        let verid_raw = regs.read(Register::VersionId);
        let hcon = regs.read(Register::HardwareConfiguration);

        // step 2: Combine synthesis data with firmware-provided FIFO geometry
        // and reject layouts this register engine cannot access safely.
        let layout = DwMshcLayout::decode(
            verid_raw,
            hcon,
            fifo_depth,
            data_addr,
            regs.size(),
        )
        .map_err(|error| {
            match error {
                LayoutError::UnsupportedVersion | LayoutError::UnsupportedFifoWidth => {
                    kerrln!(
                        "dw-mshc: TODO(stage 3+): controller layout is currently not supported: error={:?}, verid={:#x}, hcon={:#x}",
                        error,
                        verid_raw,
                        hcon
                    );
                },
                _ => {
                    kerrln!(
                        "dw-mshc: invalid controller layout: error={:?}, verid={:#x}, hcon={:#x}",
                        error,
                        verid_raw,
                        hcon
                    );
                },
            }
            SysError::DriverIncompatible
        })?;

        if layout.slot_count != 1 {
            kerrln!(
                "dw-mshc: TODO(stage 3+): multi-slot controller is currently not supported: slots={}",
                layout.slot_count
            );
            return Err(SysError::NotYetImplemented);
        }

        // step 3: Establish the powered-off polling/PIO baseline before the
        // register owner can be published to an MMC host.
        let mut inner = DwMshcInner {
            regs,
            layout,
            ciu_clock_hz,
            applied_ios: [MmcIos::OFF],
            readiness: ControllerReadiness::Probing,
        };
        inner.initialize_hardware().map_err(|error| {
            kerrln!(
                "dw-mshc: controller initialization failed: error={:?}, verid={:#x}, hcon={:#x}",
                error,
                layout.verid,
                layout.hcon
            );
            SysError::ProbeFailed
        })?;
        inner.readiness = ControllerReadiness::Ready;

        // step 4: Snapshot only immutable identity for publication logs; all
        // behavioral access remains serialized through the register owner.
        let identity = DwMshcIdentity {
            resource_base: inner.regs.phys_base(),
            resource_len: inner.regs.size(),
            layout,
        };
        Ok((
            Self {
                inner: SpinLock::new(inner),
            },
            identity,
        ))
    }
}

impl DwMshcHost {
    pub fn new(controller: Arc<DwMshcController>, caps: MmcHostCaps) -> Self {
        Self {
            controller,
            slot: 0,
            caps,
        }
    }

    fn recover_transport_locked(inner: &mut DwMshcInner) -> Result<(), MmcHostError> {
        // step 1: Accept recovery only after a submitted transaction failed;
        // proactive reset would silently destroy a valid card session.
        match inner.readiness {
            ControllerReadiness::RecoveryRequired | ControllerReadiness::Offline => {},
            ControllerReadiness::Probing
            | ControllerReadiness::Ready
            | ControllerReadiness::Recovering => return Err(MmcHostError::InvalidRequest),
        }

        // initialize_hardware disables card power before the last committed
        // IOS is reapplied. Success restores only transport readiness; the
        // caller must restart card identification/selection.
        inner.readiness = ControllerReadiness::Recovering;
        let previous_ios = inner.applied_ios[0];

        // step 2: Rebuild the transport from a reset, powered-off baseline.
        if let Err(error) = inner.initialize_hardware() {
            inner.readiness = ControllerReadiness::Offline;
            return Err(error);
        }

        // step 3: Reapply only the last fully committed IOS snapshot; partial
        // failed settings never become recovery input.
        if let Err(error) = inner.apply_ios(0, previous_ios) {
            inner.readiness = ControllerReadiness::Offline;
            return Err(error);
        }

        // step 4: Reopen the host transport. The caller still owns CMD0 and
        // all card-session reconstruction before endpoint I/O is possible.
        inner.readiness = ControllerReadiness::Ready;
        Ok(())
    }
}

impl MmcHost for DwMshcHost {
    fn caps(&self) -> MmcHostCaps {
        self.caps
    }

    fn set_ios(&self, requested: MmcIos) -> Result<MmcIos, MmcHostError> {
        // step 1: Serialize the complete electrical transaction and reject it
        // unless both host readiness and advertised capabilities allow it.
        let mut inner = self.controller.inner.lock();
        inner.require_ready()?;
        inner.validate_ios(requested, self.caps)?;

        // step 2: Publish the IOS snapshot only after every power, bus-width,
        // divider, and update-clock operation succeeds.
        match inner.apply_ios(self.slot, requested) {
            Ok(applied) => {
                // Hardware-applied IOS becomes visible only after the complete
                // clock/power/bus-width transaction succeeds.
                inner.applied_ios[self.slot as usize] = applied;
                Ok(applied)
            },
            Err(error) => {
                // step 3: Fail closed after partial MMIO; explicit recovery is
                // required before another setting or command is accepted.
                inner.latch_recovery_required();
                Err(error)
            },
        }
    }

    fn execute(&self, request: &mut MmcRequest<'_>) -> Result<(), MmcHostError> {
        // step 1: Hold the current synchronous ownership boundary across
        // validation, command issue, response capture, and optional PIO.
        let mut inner = self.controller.inner.lock();
        inner.require_ready()?;
        inner.validate_request(self.slot, request, self.caps)?;

        // step 2: Execute exactly one validated request and expose completion
        // only after all required controller phases have converged.
        match inner.execute_request(self.slot, request) {
            Ok(()) => Ok(()),
            Err(error) => {
                // step 3: A failed submitted transaction makes later register
                // state ambiguous, so latch recovery instead of retrying here.
                // Even a protocol-meaningful response timeout can leave a
                // late completion/response in this controller. Latch the
                // transport boundary; discovery may explicitly recover and
                // restart the card session, while runtime I/O fails closed.
                inner.latch_recovery_required();
                Err(error)
            },
        }
    }

    fn recover_transport(&self) -> Result<(), MmcHostError> {
        let mut inner = self.controller.inner.lock();
        Self::recover_transport_locked(&mut inner)
    }
}

impl DwMshcInner {
    fn latch_recovery_required(&mut self) {
        assert_eq!(self.readiness, ControllerReadiness::Ready);
        self.readiness = ControllerReadiness::RecoveryRequired;
    }

    fn require_ready(&self) -> Result<(), MmcHostError> {
        match self.readiness {
            ControllerReadiness::Ready => Ok(()),
            ControllerReadiness::RecoveryRequired => Err(MmcHostError::RecoveryRequired),
            ControllerReadiness::Probing | ControllerReadiness::Recovering => {
                Err(MmcHostError::RecoveryRequired)
            },
            ControllerReadiness::Offline => Err(MmcHostError::ControllerOffline),
        }
    }

    /// Poll a self-clearing or completion condition under the configured hard
    /// deadline. The deadline bounds faulty hardware; it is not a sleep budget.
    fn poll_until(&self, mut predicate: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        let timeout = Duration::from_millis(DW_MSHC_POLL_TIMEOUT_MS);
        loop {
            if predicate() {
                return true;
            }
            if start.elapsed() >= timeout {
                return false;
            }
            core::hint::spin_loop();
        }
    }

    fn log_timeout(&self, phase: &str) {
        kerrln!(
            "dw-mshc: {} timeout after {}ms: ctrl={:#x}, cmd={:#x}, rintsts={:#x}, status={:#x}, tcbcnt={}, tbbcnt={}",
            phase,
            DW_MSHC_POLL_TIMEOUT_MS,
            self.regs.read(Register::Ctrl),
            self.regs.read(Register::Command),
            self.regs.read(Register::RawInterruptStatus),
            self.regs.read(Register::Status),
            self.regs.read(Register::TransferredCardBytes),
            self.regs.read(Register::TransferredBusBytes)
        );
    }

    /// Establish a deterministic, powered-off polling/PIO baseline. Stale W1C
    /// status is cleared before reset so it cannot satisfy a later request.
    fn initialize_hardware(&mut self) -> Result<(), MmcHostError> {
        // step 1: Disable every interrupt/DMA producer before resetting shared
        // controller state.
        self.regs.write(Register::InterruptMask, 0);
        self.regs.update(
            Register::Ctrl,
            (Control::INTERRUPT_ENABLE | Control::DMA_ENABLE | Control::USE_IDMAC).bits(),
            0,
        );
        self.regs.write(Register::IdmacBusMode, 0);

        // step 2: Clear all stale W1C causes before they can satisfy a new poll.
        self.regs
            .acknowledge(RawInterrupt::from_bits_retain(u32::MAX));

        // step 3: Reset controller, FIFO, and DMA state and wait for hardware
        // to commit the reset.
        self.regs
            .update(Register::Ctrl, 0, Control::ALL_RESETS.bits());
        if !self.poll_until(|| self.regs.read(Register::Ctrl) & Control::ALL_RESETS.bits() == 0) {
            self.log_timeout("controller reset");
            return Err(MmcHostError::ControllerOffline);
        }

        // step 4: Program the bounded polling/PIO register baseline from the
        // validated controller layout.
        self.regs.write(Register::Timeout, u32::MAX);
        self.regs.write(
            Register::FifoThreshold,
            FifoThreshold::for_depth(self.layout.fifo_depth).bits(),
        );

        // step 5: Leave card power/clock/bus mode disabled; set_ios owns the
        // transition to a protocol-visible electrical state.
        self.regs.write(Register::PowerEnable, 0);
        self.regs.write(Register::ClockEnable, 0);
        self.regs.write(Register::ClockSource, 0);
        self.regs.write(Register::ClockDivider, 0);
        self.regs.write(Register::CardType, 0);
        self.regs.write(Register::Uhs, 0);
        self.update_clock(ClockUpdateOrder::Immediate)?;
        Ok(())
    }

    /// Issue the controller's internal update-clock transaction. This is not a
    /// card protocol command even though it uses the CMD start/self-clear path.
    fn update_clock(&self, order: ClockUpdateOrder) -> Result<(), MmcHostError> {
        // step 1: Remove a stale hardware-lock cause and publish a zero argument
        // before constructing this controller-internal command.
        self.regs.acknowledge(RawInterrupt::HARDWARE_LOCKED);
        self.regs.write(Register::CommandArgument, 0);
        core::sync::atomic::fence(Ordering::SeqCst);

        // step 2: Add previous-data ordering only when changing a live clock,
        // then set START to commit the update through the CIU command path.
        let mut command = Command::START | Command::UPDATE_CLOCK;
        if matches!(order, ClockUpdateOrder::AfterPreviousData) {
            command |= Command::PREVIOUS_DATA_WAIT;
        }
        self.regs.write(Register::Command, command.bits());

        // step 3: Require START to self-clear and reject a reported hardware
        // lock before treating the new clock registers as committed.
        if !self.poll_until(|| self.regs.read(Register::Command) & Command::START.bits() == 0) {
            self.log_timeout("clock update");
            return Err(MmcHostError::CommandTimeout);
        }
        let status = self.regs.raw_interrupts();
        if status.contains(RawInterrupt::HARDWARE_LOCKED) {
            self.regs.acknowledge(RawInterrupt::HARDWARE_LOCKED);
            return Err(MmcHostError::HardwareLocked);
        }
        Ok(())
    }

    fn validate_ios(&self, requested: MmcIos, caps: MmcHostCaps) -> Result<(), MmcHostError> {
        if !caps.bus_widths.contains(requested.bus_width.capability()) {
            return Err(MmcHostError::UnsupportedIos);
        }
        if !caps
            .signal_voltages
            .contains(requested.signal_voltage.capability())
        {
            kerrln!(
                "dw-mshc: TODO(stage 3+): signal voltage {:?} is currently not supported",
                requested.signal_voltage
            );
            return Err(MmcHostError::UnsupportedIos);
        }
        if requested.timing != MmcTiming::Legacy {
            kerrln!(
                "dw-mshc: TODO(stage 3+): timing {:?} is currently not supported",
                requested.timing
            );
            return Err(MmcHostError::UnsupportedIos);
        }
        if requested.power_mode == MmcPowerMode::Off && requested.clock_hz != 0 {
            return Err(MmcHostError::InvalidRequest);
        }
        if requested.clock_hz != 0
            && (requested.clock_hz < caps.min_clock_hz || requested.clock_hz > caps.max_clock_hz)
        {
            return Err(MmcHostError::UnsupportedIos);
        }
        Ok(())
    }

    fn apply_ios(&mut self, slot: u8, requested: MmcIos) -> Result<MmcIos, MmcHostError> {
        assert!((slot as usize) < self.applied_ios.len());
        assert!(
            slot == 0,
            "the current JH7110 integration publishes only slot zero"
        );
        let fields = SlotRegisterFields::for_slot(slot);

        // step 1: Power-off requests disable the card clock before removing
        // power. Commit to applied_ios happens only in the caller.
        if requested.power_mode == MmcPowerMode::Off {
            self.configure_clock(slot, 0)?;
            self.regs.update(Register::PowerEnable, fields.enable, 0);
            return Ok(MmcIos {
                clock_hz: 0,
                ..requested
            });
        }

        // step 2: Apply card power and the firmware-constrained bus width.
        self.regs.update(Register::PowerEnable, 0, fields.enable);
        let card_type = match requested.bus_width {
            MmcBusWidth::One => 0,
            MmcBusWidth::Four => fields.four_bit_bus,
            MmcBusWidth::Eight => fields.eight_bit_bus,
        };
        self.regs
            .update(Register::CardType, fields.card_type_mask, card_type);

        // step 3: Keep the current 3.3 V/non-DDR mode, then apply the requested
        // clock with disable -> divider -> enable update ordering.
        self.regs.write(Register::Uhs, 0);
        let actual_hz = self.configure_clock(slot, requested.clock_hz)?;
        Ok(MmcIos {
            clock_hz: actual_hz,
            ..requested
        })
    }

    fn configure_clock(&self, slot: u8, requested_hz: u32) -> Result<u32, MmcHostError> {
        assert!(
            slot == 0,
            "the current divider layout supports only slot zero"
        );
        let fields = SlotRegisterFields::for_slot(slot);

        // step 3.1: Disable the card clock and commit that state through the
        // CIU update-clock command before touching its divider.
        self.regs.update(Register::ClockEnable, fields.enable, 0);
        self.regs.write(Register::ClockSource, 0);
        self.update_clock(ClockUpdateOrder::AfterPreviousData)?;
        if requested_hz == 0 {
            return Ok(0);
        }

        // step 3.2: Select the smallest divider whose integer output cannot
        // exceed the protocol owner's requested frequency.
        let setting =
            calculate_clock(self.ciu_clock_hz, requested_hz).ok_or(MmcHostError::UnsupportedIos)?;
        self.regs
            .write(Register::ClockDivider, setting.divider as u32);
        self.update_clock(ClockUpdateOrder::AfterPreviousData)?;

        // step 3.3: Re-enable the card clock and commit the new divider before
        // returning its actual hardware frequency.
        self.regs.update(Register::ClockEnable, 0, fields.enable);
        self.update_clock(ClockUpdateOrder::AfterPreviousData)?;
        Ok(setting.actual_hz)
    }

    fn validate_request(
        &self,
        slot: u8,
        request: &MmcRequest<'_>,
        caps: MmcHostCaps,
    ) -> Result<(), MmcHostError> {
        assert!((slot as usize) < self.applied_ios.len());
        if request.command.opcode > 63 {
            return Err(MmcHostError::InvalidRequest);
        }
        if request.stop.is_some() {
            kerrln!(
                "dw-mshc: TODO(stage 3+): explicit stop-command transactions are currently not supported"
            );
            return Err(MmcHostError::UnsupportedRequest);
        }
        // Requests are valid only against the last fully committed IOS, never
        // against a partially programmed setting from a failed transaction.
        let ios = self.applied_ios[slot as usize];
        if ios.power_mode == MmcPowerMode::Off || ios.clock_hz == 0 {
            return Err(MmcHostError::InvalidRequest);
        }

        let Some(data) = request.data.as_ref() else {
            return Ok(());
        };
        if data.block_size() == 0 || data.blocks() == 0 {
            return Err(MmcHostError::InvalidRequest);
        }
        if data.blocks() > caps.max_block_count {
            kerrln!(
                "dw-mshc: TODO(stage 3+): multi-block request is currently not supported: blocks={}",
                data.blocks()
            );
            return Err(MmcHostError::UnsupportedRequest);
        }
        if data.block_size() > caps.max_block_size {
            return Err(MmcHostError::InvalidRequest);
        }
        let bytes = (data.block_size() as usize)
            .checked_mul(data.blocks() as usize)
            .ok_or(MmcHostError::InvalidRequest)?;
        if bytes != data.buffer_len() || bytes > caps.max_request_bytes {
            return Err(MmcHostError::InvalidRequest);
        }
        if !bytes.is_multiple_of(self.layout.fifo_width.bytes()) {
            kerrln!(
                "dw-mshc: TODO(stage 3+): partial FIFO words are currently not supported: bytes={}, word={}",
                bytes,
                self.layout.fifo_width.bytes()
            );
            return Err(MmcHostError::UnsupportedRequest);
        }
        Ok(())
    }

    fn execute_request(
        &mut self,
        _slot: u8,
        request: &mut MmcRequest<'_>,
    ) -> Result<(), MmcHostError> {
        // This engine translates framing and moves bytes only. Response/card
        // status interpretation remains above the host-controller boundary.
        // step 1: Data commands wait for DAT busy to clear so they cannot
        // overlap a preceding transfer/program operation.
        if request.data.is_some() && !self.poll_until(|| !self.regs.status().busy()) {
            self.log_timeout("previous data busy");
            return Err(MmcHostError::DataTimeout);
        }

        // step 2: Reset and size the FIFO path only when this command carries
        // a data phase.
        if let Some(data) = request.data.as_ref() {
            self.reset_fifo()?;
            self.regs.write(Register::BlockSize, data.block_size());
            self.regs
                .write(Register::ByteCount, data.buffer_len() as u32);
        }

        // step 3: Clear stale W1C completion/error causes before publishing a
        // new command to the CIU.
        self.regs
            .acknowledge(RawInterrupt::from_bits_retain(u32::MAX));

        // step 4: Program argument and framing, order the MMIO writes, then set
        // START to hand the request to hardware.
        let flags = command_flags(&request.command, request.data.as_ref());
        self.regs
            .write(Register::CommandArgument, request.command.argument);
        core::sync::atomic::fence(Ordering::SeqCst);
        self.regs.write(
            Register::Command,
            flags.bits() | request.command.opcode as u32,
        );

        // step 5.1: Wait for the command completion/error event before reading
        // response registers.
        self.wait_command_done()?;

        // step 5.2: Normalize short/long response register order before the
        // protocol layer sees it.
        self.read_response(&mut request.command);

        // step 6: Move the exact validated byte count through the synthesized
        // FIFO width and wait for DATA_OVER.
        let is_write = matches!(request.data.as_ref(), Some(MmcData::Write { .. }));
        if let Some(data) = request.data.as_mut() {
            match data {
                MmcData::Read { buffer, .. } => self.read_data(buffer)?,
                MmcData::Write { buffer, .. } => self.write_data(buffer)?,
            }
        }

        // step 7.1: A completed write may still hold DAT0 busy while the card
        // programs nonvolatile media; returning early would let the following
        // command race that programming phase.
        if is_write && !self.poll_until(|| !self.regs.status().busy()) {
            self.log_timeout("write data busy");
            return Err(MmcHostError::DataTimeout);
        }

        // step 7.2: R1b commands use the same bounded DAT busy convergence.
        if request.command.response_type == MmcResponseType::R1b
            && !self.poll_until(|| !self.regs.status().busy())
        {
            self.log_timeout("busy response");
            return Err(MmcHostError::DataTimeout);
        }
        Ok(())
    }

    fn reset_fifo(&self) -> Result<(), MmcHostError> {
        // step 2.1: Ask hardware to discard prior FIFO contents before sizing
        // the new data phase.
        self.regs
            .update(Register::Ctrl, 0, Control::FIFO_RESET.bits());

        // step 2.2: Require the self-clearing reset bit to converge under the
        // controller deadline before any BLKSIZ/BYTCNT programming proceeds.
        if self.poll_until(|| self.regs.read(Register::Ctrl) & Control::FIFO_RESET.bits() == 0) {
            Ok(())
        } else {
            self.log_timeout("FIFO reset");
            Err(MmcHostError::ControllerOffline)
        }
    }

    fn wait_command_done(&self) -> Result<(), MmcHostError> {
        let start = Instant::now();
        let timeout = Duration::from_millis(DW_MSHC_POLL_TIMEOUT_MS);
        loop {
            let raw = self.regs.raw_interrupts();
            let relevant = raw & (RawInterrupt::COMMAND_DONE | RawInterrupt::COMMAND_ERRORS);
            if !relevant.is_empty() {
                // Acknowledge only the completion/error bits observed for this
                // phase; RINTSTS is W1C and must not use read-modify-write.
                self.regs.acknowledge(relevant);
                if raw.contains(RawInterrupt::RESPONSE_TIMEOUT) {
                    return Err(MmcHostError::ResponseTimeout);
                }
                if raw.contains(RawInterrupt::RESPONSE_CRC) {
                    return Err(MmcHostError::ResponseCrc);
                }
                if raw.contains(RawInterrupt::HARDWARE_LOCKED) {
                    return Err(MmcHostError::HardwareLocked);
                }
                if raw.intersects(RawInterrupt::RESPONSE_ERROR) {
                    return Err(MmcHostError::ResponseFraming);
                }
                return Ok(());
            }
            if start.elapsed() >= timeout {
                self.log_timeout("command completion");
                return Err(MmcHostError::CommandTimeout);
            }
            core::hint::spin_loop();
        }
    }

    fn read_response(&self, command: &mut MmcCommand) {
        // RESP3 is the most-significant word for a long response. Normalize it
        // here so protocol code never depends on controller register ordering.
        command.response = match command.response_type {
            MmcResponseType::None => [0; 4],
            MmcResponseType::R2 => [
                self.regs.read(Register::Response3),
                self.regs.read(Register::Response2),
                self.regs.read(Register::Response1),
                self.regs.read(Register::Response0),
            ],
            _ => [self.regs.read(Register::Response0), 0, 0, 0],
        };
    }

    fn read_data(&self, buffer: &mut [u8]) -> Result<(), MmcHostError> {
        let width = self.layout.fifo_width.bytes();
        let start = Instant::now();
        let timeout = Duration::from_millis(DW_MSHC_POLL_TIMEOUT_MS);
        let mut offset = 0;
        while offset < buffer.len() {
            // step 6.1: Reject data-path errors before consuming FIFO contents.
            let raw = self.regs.raw_interrupts();
            self.check_data_errors(raw)?;
            let available = self.regs.status().fifo_count() as usize;
            if available != 0 {
                // step 6.2: Drain only complete synthesized-width words that
                // fit in the caller's validated buffer.
                let words = available.min((buffer.len() - offset) / width);
                for _ in 0..words {
                    let value = self.regs.read_fifo(self.layout).to_ne_bytes();
                    buffer[offset..offset + width].copy_from_slice(&value[..width]);
                    offset += width;
                }
                self.regs.acknowledge(raw & RawInterrupt::RX_READY);
                continue;
            }
            if raw.contains(RawInterrupt::DATA_OVER) {
                // step 6.3: DATA_OVER before the requested byte count is a
                // short transfer, never successful EOF semantics.
                self.regs.acknowledge(RawInterrupt::DATA_OVER);
                return Err(MmcHostError::ShortTransfer);
            }
            if start.elapsed() >= timeout {
                self.log_timeout("PIO read");
                return Err(MmcHostError::DataTimeout);
            }
            core::hint::spin_loop();
        }
        // step 6.4: After the final word, require the hardware data-completion
        // event under the original transfer deadline.
        self.wait_data_over(start)
    }

    fn write_data(&self, buffer: &[u8]) -> Result<(), MmcHostError> {
        let width = self.layout.fifo_width.bytes();
        let start = Instant::now();
        let timeout = Duration::from_millis(DW_MSHC_POLL_TIMEOUT_MS);
        let mut offset = 0;
        while offset < buffer.len() {
            // step 6.1: Reject data-path errors before adding more FIFO data.
            let raw = self.regs.raw_interrupts();
            self.check_data_errors(raw)?;
            if let Err(error) = reject_early_write_completion(raw) {
                // DATA_OVER is W1C. Clear the observed premature completion
                // before the caller latches recovery for this short transfer.
                self.regs.acknowledge(RawInterrupt::DATA_OVER);
                return Err(error);
            }
            let used = self.regs.status().fifo_count();
            if used > self.layout.fifo_depth {
                return Err(MmcHostError::FifoRun);
            }
            let available = (self.layout.fifo_depth - used) as usize;
            if available != 0 {
                // step 6.2: Fill only currently free FIFO words; hardware owns
                // consumption after each volatile write.
                let words = available.min((buffer.len() - offset) / width);
                for _ in 0..words {
                    let mut bytes = [0u8; 8];
                    bytes[..width].copy_from_slice(&buffer[offset..offset + width]);
                    self.regs.write_fifo(self.layout, u64::from_ne_bytes(bytes));
                    offset += width;
                }
                self.regs.acknowledge(raw & RawInterrupt::TX_READY);
                continue;
            }
            if start.elapsed() >= timeout {
                self.log_timeout("PIO write");
                return Err(MmcHostError::DataTimeout);
            }
            core::hint::spin_loop();
        }
        // step 6.3: The final FIFO write is not complete until DATA_OVER.
        self.wait_data_over(start)
    }

    fn wait_data_over(&self, start: Instant) -> Result<(), MmcHostError> {
        let timeout = Duration::from_millis(DW_MSHC_POLL_TIMEOUT_MS);
        loop {
            let raw = self.regs.raw_interrupts();
            self.check_data_errors(raw)?;
            if raw.contains(RawInterrupt::DATA_OVER) {
                self.regs.acknowledge(RawInterrupt::DATA_OVER);
                return Ok(());
            }
            if start.elapsed() >= timeout {
                self.log_timeout("data completion");
                return Err(MmcHostError::DataTimeout);
            }
            core::hint::spin_loop();
        }
    }

    fn check_data_errors(&self, raw: RawInterrupt) -> Result<(), MmcHostError> {
        let Some(error) = classify_data_error(raw) else {
            return Ok(());
        };
        let errors = raw & RawInterrupt::DATA_ERRORS;
        // Clear all observed data error causes before returning the selected
        // typed error, preventing a stale cause from poisoning recovery.
        self.regs.acknowledge(errors);
        Err(error)
    }
}

fn reject_early_write_completion(raw: RawInterrupt) -> Result<(), MmcHostError> {
    if raw.contains(RawInterrupt::DATA_OVER) {
        Err(MmcHostError::ShortTransfer)
    } else {
        Ok(())
    }
}

fn classify_data_error(raw: RawInterrupt) -> Option<MmcHostError> {
    let errors = raw & RawInterrupt::DATA_ERRORS;
    if errors.is_empty() {
        return None;
    }
    if errors.contains(RawInterrupt::HARDWARE_LOCKED) {
        return Some(MmcHostError::HardwareLocked);
    }
    if errors.intersects(RawInterrupt::DATA_TIMEOUT | RawInterrupt::HOST_TIMEOUT) {
        return Some(MmcHostError::DataTimeout);
    }
    if errors.contains(RawInterrupt::DATA_CRC) {
        return Some(MmcHostError::DataCrc);
    }
    if errors.contains(RawInterrupt::FIFO_RUN) {
        return Some(MmcHostError::FifoRun);
    }
    Some(MmcHostError::ResponseFraming)
}

fn calculate_clock(source_hz: u32, requested_hz: u32) -> Option<ClockSetting> {
    if source_hz == 0 || requested_hz == 0 {
        return None;
    }
    if requested_hz >= source_hz {
        return Some(ClockSetting {
            divider: 0,
            actual_hz: source_hz,
        });
    }

    // Hardware integer division rounds the produced clock down. Solve
    // floor(source / (2 * divider)) <= requested directly; rational ceiling
    // division would incorrectly reject the achievable divider-255 boundary.
    let denominator = 2u64 * (requested_hz as u64 + 1);
    let divider = source_hz as u64 / denominator + 1;
    if divider == 0 || divider > u8::MAX as u64 {
        return None;
    }
    Some(ClockSetting {
        divider: divider as u8,
        actual_hz: source_hz / (2 * divider as u32),
    })
}

fn command_flags(command: &MmcCommand, data: Option<&MmcData<'_>>) -> Command {
    // Protocol code supplies response framing explicitly. Opcode-only
    // inference would leak SD/MMC/SDIO state-machine policy into this driver.
    let mut flags = Command::START | Command::USE_HOLD_REGISTER;
    match command.response_type {
        MmcResponseType::None => {},
        MmcResponseType::R2 => {
            flags |= Command::RESPONSE_EXPECTED | Command::RESPONSE_LONG | Command::RESPONSE_CRC;
        },
        MmcResponseType::R3 | MmcResponseType::R4 => {
            flags |= Command::RESPONSE_EXPECTED;
        },
        MmcResponseType::R1
        | MmcResponseType::R1b
        | MmcResponseType::R5
        | MmcResponseType::R6
        | MmcResponseType::R7 => {
            flags |= Command::RESPONSE_EXPECTED | Command::RESPONSE_CRC;
        },
    }
    if let Some(data) = data {
        flags |= Command::DATA_EXPECTED | Command::PREVIOUS_DATA_WAIT;
        if matches!(data, MmcData::Write { .. }) {
            flags |= Command::DATA_WRITE;
        }
    }
    if command
        .flags
        .contains(MmcCommandFlags::INITIALIZATION_CLOCKS)
    {
        flags |= Command::INITIALIZATION_CLOCKS;
    }
    if command.flags.contains(MmcCommandFlags::STOP_ABORT) {
        flags |= Command::STOP_ABORT;
    }
    flags
}

#[kunit]
fn clock_divider_never_overclocks() {
    let setting = calculate_clock(50_000_000, 400_000).unwrap();
    assert_eq!(setting.divider, 63);
    assert!(setting.actual_hz <= 400_000);
    assert_eq!(calculate_clock(50_000_000, 50_000_000).unwrap().divider, 0);
    let minimum = calculate_clock(50_000_000, 98_039).unwrap();
    assert_eq!(minimum.divider, 255);
    assert_eq!(minimum.actual_hz, 98_039);
    assert_eq!(calculate_clock(50_000_000, 98_038), None);
    assert_eq!(calculate_clock(50_000_000, 1), None);
}

#[kunit]
fn response_and_data_flags_are_protocol_neutral() {
    let command = MmcCommand::new(17, 0, MmcResponseType::R1);
    let buffer = [0u8; 512];
    let data = MmcData::Write {
        block_size: 512,
        blocks: 1,
        buffer: &buffer,
    };
    let flags = command_flags(&command, Some(&data));
    assert!(flags.contains(Command::RESPONSE_EXPECTED));
    assert!(flags.contains(Command::RESPONSE_CRC));
    assert!(flags.contains(Command::DATA_EXPECTED));
    assert!(flags.contains(Command::DATA_WRITE));
}

#[kunit]
fn data_phase_rejects_hardware_lock_and_early_write_completion() {
    assert_eq!(
        classify_data_error(RawInterrupt::HARDWARE_LOCKED),
        Some(MmcHostError::HardwareLocked)
    );
    assert_eq!(
        classify_data_error(RawInterrupt::HARDWARE_LOCKED | RawInterrupt::DATA_OVER),
        Some(MmcHostError::HardwareLocked)
    );
    assert_eq!(
        reject_early_write_completion(RawInterrupt::DATA_OVER),
        Err(MmcHostError::ShortTransfer)
    );
    assert_eq!(
        reject_early_write_completion(RawInterrupt::TX_READY),
        Ok(())
    );
}
