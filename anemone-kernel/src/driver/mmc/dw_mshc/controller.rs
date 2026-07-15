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
    /// A failed transaction is rebuilding the controller baseline.
    Recovering,
    /// Recovery failed; only an explicit recovery attempt may revive it.
    Offline,
}

/// Sole owner of the DW-MSHC register block and mutable request state.
pub(super) struct DwMshcController {
    // Temporary stage-1 serialization: with spin_lock_irqsave enabled this
    // keeps local interrupts disabled across command polling and PIO. The
    // controller worker must become the sole DwMshcInner owner and remove this
    // lock before card/block traffic is enabled.
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
        let regs = DwMshcRegs::new(remap);
        let verid_raw = regs.read(Register::VersionId);
        let hcon = regs.read(Register::HardwareConfiguration);
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
                        "dw-mshc: TODO(stage 1): controller layout is currently not supported: error={:?}, verid={:#x}, hcon={:#x}",
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
                "dw-mshc: TODO(stage 2): multi-slot controller is currently not supported: slots={}",
                layout.slot_count
            );
            return Err(SysError::NotYetImplemented);
        }
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

    fn recover_locked(inner: &mut DwMshcInner) -> Result<(), MmcHostError> {
        // Controller recovery restores transport state only. The future MMC
        // core remains responsible for deciding whether the card session must
        // be power-cycled or rediscovered after the original error.
        inner.readiness = ControllerReadiness::Recovering;
        let previous_ios = inner.applied_ios[0];
        if let Err(error) = inner.initialize_hardware() {
            inner.readiness = ControllerReadiness::Offline;
            return Err(error);
        }
        if let Err(error) = inner.apply_ios(0, previous_ios) {
            inner.readiness = ControllerReadiness::Offline;
            return Err(error);
        }
        inner.readiness = ControllerReadiness::Ready;
        Ok(())
    }
}

impl MmcHost for DwMshcHost {
    fn caps(&self) -> MmcHostCaps {
        self.caps
    }

    fn set_ios(&self, requested: MmcIos) -> Result<MmcIos, MmcHostError> {
        let mut inner = self.controller.inner.lock();
        inner.require_ready()?;
        inner.validate_ios(requested, self.caps)?;
        match inner.apply_ios(self.slot, requested) {
            Ok(applied) => {
                // Hardware-applied IOS becomes visible only after the complete
                // clock/power/bus-width transaction succeeds.
                inner.applied_ios[self.slot as usize] = applied;
                Ok(applied)
            },
            Err(error) => {
                if let Err(recovery_error) = Self::recover_locked(&mut inner) {
                    kerrln!(
                        "dw-mshc: IOS update and recovery failed: update={:?}, recovery={:?}",
                        error,
                        recovery_error
                    );
                    return Err(MmcHostError::ControllerOffline);
                }
                Err(error)
            },
        }
    }

    fn execute(&self, request: &mut MmcRequest<'_>) -> Result<(), MmcHostError> {
        let mut inner = self.controller.inner.lock();
        inner.require_ready()?;
        inner.validate_request(self.slot, request, self.caps)?;
        match inner.execute_request(self.slot, request) {
            Ok(()) => Ok(()),
            Err(error) => {
                if let Err(recovery_error) = Self::recover_locked(&mut inner) {
                    kerrln!(
                        "dw-mshc: request and recovery failed: request={:?}, recovery={:?}",
                        error,
                        recovery_error
                    );
                    return Err(MmcHostError::ControllerOffline);
                }
                Err(error)
            },
        }
    }

    fn recover(&self) -> Result<(), MmcHostError> {
        let mut inner = self.controller.inner.lock();
        Self::recover_locked(&mut inner)
    }
}

impl DwMshcInner {
    fn require_ready(&self) -> Result<(), MmcHostError> {
        match self.readiness {
            ControllerReadiness::Ready => Ok(()),
            ControllerReadiness::Probing
            | ControllerReadiness::Recovering
            | ControllerReadiness::Offline => Err(MmcHostError::ControllerOffline),
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
        self.regs.write(Register::InterruptMask, 0);
        self.regs.update(
            Register::Ctrl,
            (Control::INTERRUPT_ENABLE | Control::DMA_ENABLE | Control::USE_IDMAC).bits(),
            0,
        );
        self.regs.write(Register::IdmacBusMode, 0);
        self.regs
            .acknowledge(RawInterrupt::from_bits_retain(u32::MAX));

        self.regs
            .update(Register::Ctrl, 0, Control::ALL_RESETS.bits());
        if !self.poll_until(|| self.regs.read(Register::Ctrl) & Control::ALL_RESETS.bits() == 0) {
            self.log_timeout("controller reset");
            return Err(MmcHostError::ControllerOffline);
        }

        self.regs.write(Register::Timeout, u32::MAX);
        self.regs.write(
            Register::FifoThreshold,
            FifoThreshold::for_depth(self.layout.fifo_depth).bits(),
        );
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
        self.regs.acknowledge(RawInterrupt::HARDWARE_LOCKED);
        self.regs.write(Register::CommandArgument, 0);
        core::sync::atomic::fence(Ordering::SeqCst);
        let mut command = Command::START | Command::UPDATE_CLOCK;
        if matches!(order, ClockUpdateOrder::AfterPreviousData) {
            command |= Command::PREVIOUS_DATA_WAIT;
        }
        self.regs.write(Register::Command, command.bits());
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
                "dw-mshc: TODO(stage 2): signal voltage {:?} is currently not supported",
                requested.signal_voltage
            );
            return Err(MmcHostError::UnsupportedIos);
        }
        if requested.timing != MmcTiming::Legacy {
            kerrln!(
                "dw-mshc: TODO(stage 2): timing {:?} is currently not supported",
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
            "stage-1 JH7110 integration only publishes slot zero"
        );

        // Clock changes use disable -> update -> divider -> update -> enable ->
        // update ordering. Commit to applied_ios happens only in the caller.
        if requested.power_mode == MmcPowerMode::Off {
            self.configure_clock(slot, 0)?;
            self.regs.update(Register::PowerEnable, 1 << slot, 0);
            return Ok(MmcIos {
                clock_hz: 0,
                ..requested
            });
        }

        self.regs.update(Register::PowerEnable, 0, 1 << slot);
        let card_type = match requested.bus_width {
            MmcBusWidth::One => 0,
            MmcBusWidth::Four => 1 << slot,
            MmcBusWidth::Eight => 1 << (16 + slot),
        };
        self.regs.update(
            Register::CardType,
            (1 << slot) | (1 << (16 + slot)),
            card_type,
        );
        self.regs.write(Register::Uhs, 0);
        let actual_hz = self.configure_clock(slot, requested.clock_hz)?;
        Ok(MmcIos {
            clock_hz: actual_hz,
            ..requested
        })
    }

    fn configure_clock(&self, slot: u8, requested_hz: u32) -> Result<u32, MmcHostError> {
        assert!(slot == 0, "stage-1 divider layout only supports slot zero");
        self.regs.update(Register::ClockEnable, 1 << slot, 0);
        self.regs.write(Register::ClockSource, 0);
        self.update_clock(ClockUpdateOrder::AfterPreviousData)?;
        if requested_hz == 0 {
            return Ok(0);
        }

        let setting =
            calculate_clock(self.ciu_clock_hz, requested_hz).ok_or(MmcHostError::UnsupportedIos)?;
        self.regs
            .write(Register::ClockDivider, setting.divider as u32);
        self.update_clock(ClockUpdateOrder::AfterPreviousData)?;
        self.regs.update(Register::ClockEnable, 0, 1 << slot);
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
                "dw-mshc: TODO(stage 2): explicit stop-command transactions are currently not supported"
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
                "dw-mshc: TODO(stage 2): multi-block request is currently not supported: blocks={}",
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
                "dw-mshc: TODO(stage 2): partial FIFO words are currently not supported: bytes={}, word={}",
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
        if request.data.is_some() && !self.poll_until(|| !self.regs.status().busy()) {
            self.log_timeout("previous data busy");
            return Err(MmcHostError::DataTimeout);
        }

        if let Some(data) = request.data.as_ref() {
            self.reset_fifo()?;
            self.regs.write(Register::BlockSize, data.block_size());
            self.regs
                .write(Register::ByteCount, data.buffer_len() as u32);
        }
        self.regs
            .acknowledge(RawInterrupt::from_bits_retain(u32::MAX));

        let flags = command_flags(&request.command, request.data.as_ref());
        self.regs
            .write(Register::CommandArgument, request.command.argument);
        core::sync::atomic::fence(Ordering::SeqCst);
        self.regs.write(
            Register::Command,
            flags.bits() | request.command.opcode as u32,
        );
        self.wait_command_done()?;
        self.read_response(&mut request.command);

        if let Some(data) = request.data.as_mut() {
            self.transfer_data(data)?;
        }
        if request.command.response_type == MmcResponseType::R1b
            && !self.poll_until(|| !self.regs.status().busy())
        {
            self.log_timeout("busy response");
            return Err(MmcHostError::DataTimeout);
        }
        Ok(())
    }

    fn reset_fifo(&self) -> Result<(), MmcHostError> {
        self.regs
            .update(Register::Ctrl, 0, Control::FIFO_RESET.bits());
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
                    return Err(MmcHostError::CommandTimeout);
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

    fn transfer_data(&self, data: &mut MmcData<'_>) -> Result<(), MmcHostError> {
        match data {
            MmcData::Read { buffer, .. } => self.read_data(buffer),
            MmcData::Write { buffer, .. } => self.write_data(buffer),
        }
    }

    fn read_data(&self, buffer: &mut [u8]) -> Result<(), MmcHostError> {
        let width = self.layout.fifo_width.bytes();
        let start = Instant::now();
        let timeout = Duration::from_millis(DW_MSHC_POLL_TIMEOUT_MS);
        let mut offset = 0;
        while offset < buffer.len() {
            let raw = self.regs.raw_interrupts();
            self.check_data_errors(raw)?;
            let available = self.regs.status().fifo_count() as usize;
            if available != 0 {
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
                self.regs.acknowledge(RawInterrupt::DATA_OVER);
                return Err(MmcHostError::ShortTransfer);
            }
            if start.elapsed() >= timeout {
                self.log_timeout("PIO read");
                return Err(MmcHostError::DataTimeout);
            }
            core::hint::spin_loop();
        }
        self.wait_data_over(start)
    }

    fn write_data(&self, buffer: &[u8]) -> Result<(), MmcHostError> {
        let width = self.layout.fifo_width.bytes();
        let start = Instant::now();
        let timeout = Duration::from_millis(DW_MSHC_POLL_TIMEOUT_MS);
        let mut offset = 0;
        while offset < buffer.len() {
            let raw = self.regs.raw_interrupts();
            self.check_data_errors(raw)?;
            let used = self.regs.status().fifo_count();
            if used > self.layout.fifo_depth {
                return Err(MmcHostError::FifoRun);
            }
            let available = (self.layout.fifo_depth - used) as usize;
            if available != 0 {
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
        let errors = raw & RawInterrupt::DATA_ERRORS;
        if errors.is_empty() {
            return Ok(());
        }
        // Clear all observed data error causes before returning the selected
        // typed error, preventing a stale cause from poisoning recovery.
        self.regs.acknowledge(errors);
        if raw.intersects(RawInterrupt::DATA_TIMEOUT | RawInterrupt::HOST_TIMEOUT) {
            return Err(MmcHostError::DataTimeout);
        }
        if raw.contains(RawInterrupt::DATA_CRC) {
            return Err(MmcHostError::DataCrc);
        }
        if raw.contains(RawInterrupt::FIFO_RUN) {
            return Err(MmcHostError::FifoRun);
        }
        Err(MmcHostError::ResponseFraming)
    }
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

    // Ceiling division selects the smallest divider that cannot exceed the
    // requested card clock.
    let denominator = 2u64 * requested_hz as u64;
    let divider = ((source_hz as u64) + denominator - 1) / denominator;
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
