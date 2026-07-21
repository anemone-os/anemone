//! SD Memory cold attach and shared command/status framing.
//!
//! Protocol fields follow SD Physical Layer Simplified Specification v9.10:
//! identification in Sections 4.2/4.2.3, memory access in Sections
//! 4.3.3/4.3.4/4.3.14, card status in Section 4.10.1, CSD in Section 5.3,
//! and command definitions in Section 7.3.3.

use super::MmcDiscoveryError;
use crate::{device::mmc::*, prelude::*};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum SdCommand {
    GoIdle = 0,
    AllSendCid = 2,
    SendRelativeAddress = 3,
    SelectCard = 7,
    SendInterfaceCondition = 8,
    SendCsd = 9,
    SendStatus = 13,
    SetBlockLength = 16,
    ReadSingleBlock = 17,
    WriteBlock = 24,
    AppCommand = 55,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum SdApplicationCommand {
    SendOperatingCondition = 41,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum SdExtensionProbe {
    IoOperatingCondition = 5,
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) struct SdR1Flags: u32 {
        const OUT_OF_RANGE = 1 << 31;
        const ADDRESS_ERROR = 1 << 30;
        const BLOCK_LEN_ERROR = 1 << 29;
        const ERASE_SEQ_ERROR = 1 << 28;
        const ERASE_PARAM = 1 << 27;
        const WP_VIOLATION = 1 << 26;
        const CARD_IS_LOCKED = 1 << 25;
        const LOCK_UNLOCK_FAILED = 1 << 24;
        const COM_CRC_ERROR = 1 << 23;
        const ILLEGAL_COMMAND = 1 << 22;
        const CARD_ECC_FAILED = 1 << 21;
        const CC_ERROR = 1 << 20;
        const ERROR = 1 << 19;
        const UNDERRUN = 1 << 18;
        const OVERRUN = 1 << 17;
        const CSD_OVERWRITE = 1 << 16;
        const WP_ERASE_SKIP = 1 << 15;
        const CARD_ECC_DISABLED = 1 << 14;
        const ERASE_RESET = 1 << 13;
        const READY_FOR_DATA = 1 << 8;
        const SWITCH_ERROR = 1 << 7;
        const FX_EVENT = 1 << 6;
        const APP_COMMAND = 1 << 5;
        const AKE_SEQ_ERROR = 1 << 3;
    }

    /// Status field returned in the low 16 bits of an R6 response.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct SdR6Flags: u32 {
        const COM_CRC_ERROR = 1 << 15;
        const ILLEGAL_COMMAND = 1 << 14;
        const ERROR = 1 << 13;
        const READY_FOR_DATA = 1 << 8;
        const APP_COMMAND = 1 << 5;
        const AKE_SEQ_ERROR = 1 << 3;
    }
}

impl SdR1Flags {
    const ERRORS: Self = Self::OUT_OF_RANGE
        .union(Self::ADDRESS_ERROR)
        .union(Self::BLOCK_LEN_ERROR)
        .union(Self::ERASE_SEQ_ERROR)
        .union(Self::ERASE_PARAM)
        .union(Self::WP_VIOLATION)
        .union(Self::CARD_IS_LOCKED)
        .union(Self::LOCK_UNLOCK_FAILED)
        .union(Self::COM_CRC_ERROR)
        .union(Self::ILLEGAL_COMMAND)
        .union(Self::CARD_ECC_FAILED)
        .union(Self::CC_ERROR)
        .union(Self::ERROR)
        .union(Self::UNDERRUN)
        .union(Self::OVERRUN)
        .union(Self::CSD_OVERWRITE)
        .union(Self::WP_ERASE_SKIP)
        .union(Self::SWITCH_ERROR)
        .union(Self::AKE_SEQ_ERROR);
}

impl SdR6Flags {
    const ERRORS: Self = Self::COM_CRC_ERROR
        .union(Self::ILLEGAL_COMMAND)
        .union(Self::ERROR)
        .union(Self::AKE_SEQ_ERROR);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum SdCardState {
    Idle = 0,
    Ready = 1,
    Identification = 2,
    Standby = 3,
    Transfer = 4,
    SendingData = 5,
    ReceiveData = 6,
    Programming = 7,
    Disconnected = 8,
}

impl SdCardState {
    pub const fn response_bits(self) -> u32 {
        (self as u32) << 9
    }

    const fn decode(response: u32) -> Option<Self> {
        match (response >> 9) & 0xf {
            0 => Some(Self::Idle),
            1 => Some(Self::Ready),
            2 => Some(Self::Identification),
            3 => Some(Self::Standby),
            4 => Some(Self::Transfer),
            5 => Some(Self::SendingData),
            6 => Some(Self::ReceiveData),
            7 => Some(Self::Programming),
            8 => Some(Self::Disconnected),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SdR1Response {
    raw: u32,
    pub flags: SdR1Flags,
    pub card_state: Option<SdCardState>,
}

impl SdR1Response {
    pub fn decode(raw: u32) -> Self {
        Self {
            raw,
            flags: SdR1Flags::from_bits_retain(raw),
            card_state: SdCardState::decode(raw),
        }
    }

    pub fn check(self) -> Result<(), SdProtocolError> {
        if self.flags.intersects(SdR1Flags::ERRORS) {
            Err(SdProtocolError::CardStatus(self.raw))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SdR6Response {
    raw: u32,
    rca: SdRelativeAddress,
    flags: SdR6Flags,
    card_state: Option<SdCardState>,
}

impl SdR6Response {
    fn decode(raw: u32) -> Self {
        Self {
            raw,
            rca: SdRelativeAddress::from_raw((raw >> 16) as u16),
            flags: SdR6Flags::from_bits_retain(raw),
            card_state: SdCardState::decode(raw),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum SdAcceptedVoltage {
    V2_7To3_6 = 1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum SdCsdStructure {
    Version1 = 0,
    Version2 = 1,
    Version3 = 2,
}

impl SdCsdStructure {
    const fn decode(encoded: u32) -> Option<Self> {
        match encoded {
            0 => Some(Self::Version1),
            1 => Some(Self::Version2),
            2 => Some(Self::Version3),
            _ => None,
        }
    }
}

impl SdAcceptedVoltage {
    const fn decode(encoded: u8) -> Option<Self> {
        match encoded {
            1 => Some(Self::V2_7To3_6),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SdInterfaceCondition {
    accepted_voltage: SdAcceptedVoltage,
    check_pattern: u8,
}

impl SdInterfaceCondition {
    const SUPPORTED: Self = Self {
        accepted_voltage: SdAcceptedVoltage::V2_7To3_6,
        check_pattern: 0xaa,
    };

    const fn argument(self) -> u32 {
        ((self.accepted_voltage as u32) << 8) | self.check_pattern as u32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SdR7Response {
    raw: u32,
    accepted_voltage: Option<SdAcceptedVoltage>,
    check_pattern: u8,
}

impl SdR7Response {
    fn decode(raw: u32) -> Self {
        Self {
            raw,
            accepted_voltage: SdAcceptedVoltage::decode(((raw >> 8) & 0xf) as u8),
            check_pattern: raw as u8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SdProtocolError {
    CardStatus(u32),
    NotReady(u32),
    AddressOverflow,
}

pub(super) fn attach(host: &MmcHostDevice) -> Result<SdCardIdentity, MmcDiscoveryError> {
    // step 1: Apply the powered, 1-bit, identification-clock baseline before
    // any protocol command can reach the card.
    establish_initial_ios(host)?;

    // step 2: Put every card on the bus into the SD idle state.
    send_go_idle(host)?;

    // step 3: Reject SDIO/combo responders before committing to SD Memory.
    // Physical Layer v9.10 Section 4.6.1 states that CMD5 is undefined for an
    // SD Memory card and an illegal command receives no response. We use only
    // that negative fact to prevent an SDIO/combo responder from being
    // downgraded to SdMemory. R4 contents remain uninterpreted until a fixed
    // SDIO specification is available.
    if host.caps().allowed_kinds.contains(MmcCardKinds::SDIO) {
        let mut io_probe: MmcCommand = MmcCommand::new(
            SdExtensionProbe::IoOperatingCondition as u8,
            0,
            MmcResponseType::R4,
        );
        match execute(host, &mut io_probe) {
            Err(MmcHostError::ResponseTimeout) => {
                restart_identification_after_probe_timeout(host, io_probe.opcode)?;
            },
            Ok(()) => {
                kerrln!("mmc: TODO(stage 3+): SDIO/SDIO-combo attach is currently not supported");
                return Err(MmcDiscoveryError::ProtocolUnavailable(MmcCardKind::Sdio));
            },
            Err(error) => {
                log_command_error(host, &io_probe, error);
                return Err(MmcDiscoveryError::Transport(error));
            },
        }
    }

    // step 4: Validate the SD v2 interface condition; a timeout selects the
    // legacy SD path after transport/session restart.
    let version_two = match send_interface_condition(host) {
        Ok(()) => true,
        Err(MmcHostError::ResponseTimeout) => {
            restart_identification_after_probe_timeout(
                host,
                SdCommand::SendInterfaceCondition as u8,
            )?;
            false
        },
        Err(MmcHostError::ResponseFraming) => {
            return Err(MmcDiscoveryError::ProtocolRejected(MmcCardKind::SdMemory));
        },
        Err(error) => {
            let condition = SdInterfaceCondition::SUPPORTED;
            let command = MmcCommand::new(
                SdCommand::SendInterfaceCondition as u8,
                condition.argument(),
                MmcResponseType::R7,
            );
            log_command_error(host, &command, error);
            return Err(MmcDiscoveryError::Transport(error));
        },
    };

    // step 5: Use CMD55/ACMD41 to negotiate voltage and wait for card power-up.
    let operating_conditions = negotiate_ocr(host, version_two)?;

    // step 6.1: Read immutable card identification before assigning identity.
    let mut cid_command = MmcCommand::new(SdCommand::AllSendCid as u8, 0, MmcResponseType::R2);
    execute_required(host, &mut cid_command).map_err(MmcDiscoveryError::Transport)?;
    let cid = SdCid::from_response(cid_command.response);

    // step 6.2: Obtain and validate the card-assigned relative address.
    let mut rca_command =
        MmcCommand::new(SdCommand::SendRelativeAddress as u8, 0, MmcResponseType::R6);
    execute_required(host, &mut rca_command).map_err(MmcDiscoveryError::Transport)?;
    let rca_response = SdR6Response::decode(rca_command.response[0]);
    if rca_response.flags.intersects(SdR6Flags::ERRORS) {
        kerrln!(
            "mmc host{}: SD CMD3 rejected: response={:#010x}, error_bits={:#06x}, state={:?}",
            host.id().get(),
            rca_response.raw,
            rca_response.flags.bits() & SdR6Flags::ERRORS.bits(),
            rca_response.card_state
        );
        return Err(MmcDiscoveryError::InvalidIdentity);
    }
    let rca = rca_response.rca;
    if rca.get() == 0 {
        kerrln!(
            "mmc host{}: SD CMD3 returned an invalid zero RCA: response={:#010x}",
            host.id().get(),
            rca_response.raw
        );
        return Err(MmcDiscoveryError::InvalidIdentity);
    }

    // step 6.3: Decode addressing mode and capacity from the card's CSD.
    let mut csd_command = MmcCommand::new(
        SdCommand::SendCsd as u8,
        rca.command_argument(),
        MmcResponseType::R2,
    );
    execute_required(host, &mut csd_command).map_err(MmcDiscoveryError::Transport)?;
    let csd = SdCsd::from_response(csd_command.response);
    let (addressing, capacity_bytes) = decode_csd(operating_conditions, csd)?;

    // step 7: Select the identified card and wait for its busy response to end.
    let mut select = MmcCommand::new(
        SdCommand::SelectCard as u8,
        rca.command_argument(),
        MmcResponseType::R1b,
    );
    execute_required(host, &mut select).map_err(MmcDiscoveryError::Transport)?;
    if let Err(error) = SdR1Response::decode(select.response[0]).check() {
        kerrln!(
            "mmc host{}: SD CMD7 rejected: response={:#010x}, error={:?}",
            host.id().get(),
            select.response[0],
            error
        );
        return Err(MmcDiscoveryError::InvalidIdentity);
    }

    // step 8.1: Standard-capacity cards require an explicit 512-byte block
    // length; high-capacity cards already use fixed 512-byte logical blocks.
    if addressing == SdAddressing::Byte {
        let mut set_block_len =
            MmcCommand::new(SdCommand::SetBlockLength as u8, 512, MmcResponseType::R1);
        execute_required(host, &mut set_block_len).map_err(MmcDiscoveryError::Transport)?;
        if let Err(error) = SdR1Response::decode(set_block_len.response[0]).check() {
            kerrln!(
                "mmc host{}: SD CMD16 rejected: response={:#010x}, error={:?}",
                host.id().get(),
                set_block_len.response[0],
                error
            );
            return Err(MmcDiscoveryError::InvalidIdentity);
        }
    }

    // step 8.2: Raise only the clock after attach; Stage 2 deliberately keeps
    // 1-bit, 3.3 V, legacy timing.
    let caps = host.caps();
    let clock_hz = MMC_SD_DATA_CLOCK_HZ.clamp(caps.min_clock_hz, caps.max_clock_hz);
    host.set_ios(MmcIos {
        power_mode: MmcPowerMode::On,
        clock_hz,
        bus_width: MmcBusWidth::One,
        signal_voltage: MmcSignalVoltage::V3_3,
        timing: MmcTiming::Legacy,
    })
    .map_err(MmcDiscoveryError::Transport)?;
    Ok(SdCardIdentity {
        operating_conditions,
        rca,
        cid,
        csd,
        addressing,
        capacity_bytes,
    })
}

fn establish_initial_ios(host: &MmcHostDevice) -> Result<(), MmcDiscoveryError> {
    // step 1.1: Remove card clock/power before establishing a fresh baseline.
    host.set_ios(MmcIos::OFF)
        .map_err(MmcDiscoveryError::Transport)?;
    let caps = host.caps();
    let clock_hz = MMC_IDENTIFICATION_CLOCK_HZ.clamp(caps.min_clock_hz, caps.max_clock_hz);
    let initial = MmcIos {
        power_mode: MmcPowerMode::Up,
        clock_hz,
        bus_width: MmcBusWidth::One,
        signal_voltage: MmcSignalVoltage::V3_3,
        timing: MmcTiming::Legacy,
    };

    // step 1.2: Apply power and identification clock, then honor the
    // firmware-provided stabilization delay outside the controller lock.
    host.set_ios(initial)
        .map_err(MmcDiscoveryError::Transport)?;
    busy_delay(caps.post_power_on_delay);

    // step 1.3: Commit normal command transfer mode at the same safe clock.
    host.set_ios(MmcIos {
        power_mode: MmcPowerMode::On,
        ..initial
    })
    .map_err(MmcDiscoveryError::Transport)?;
    Ok(())
}

fn send_interface_condition(host: &MmcHostDevice) -> Result<(), MmcHostError> {
    // step 4.1: Send the supported-voltage/check-pattern tuple so an SD v2
    // card can echo it through R7.
    let condition = SdInterfaceCondition::SUPPORTED;
    let mut command = MmcCommand::new(
        SdCommand::SendInterfaceCondition as u8,
        condition.argument(),
        MmcResponseType::R7,
    );
    execute(host, &mut command)?;

    // step 4.2: Accept v2 only when every defined R7 echo bit matches the
    // request; a stale or malformed short response must not select SDHC mode.
    let response = SdR7Response::decode(command.response[0]);
    if response.accepted_voltage != Some(condition.accepted_voltage)
        || response.check_pattern != condition.check_pattern
    {
        kerrln!(
            "mmc host{}: SD CMD8 rejected: response={:#010x}, voltage={:?}, check_pattern={:#04x}",
            host.id().get(),
            response.raw,
            response.accepted_voltage,
            response.check_pattern
        );
        return Err(MmcHostError::ResponseFraming);
    }
    Ok(())
}

fn negotiate_ocr(
    host: &MmcHostDevice,
    version_two: bool,
) -> Result<SdOperatingConditions, MmcDiscoveryError> {
    let mut request = SdOperatingConditions::VOLTAGE_2_7_TO_3_6;
    if version_two {
        request |= SdOperatingConditions::CAPACITY_STATUS;
    }
    let start = Instant::now();
    let timeout = Duration::from_millis(MMC_CARD_INIT_TIMEOUT_MS);

    loop {
        // step 5.1: Prefix every ACMD41 attempt with CMD55 and require APP_CMD
        // acceptance from the card.
        let mut app = MmcCommand::new(SdCommand::AppCommand as u8, 0, MmcResponseType::R1);
        match execute_required(host, &mut app) {
            Ok(()) => {},
            Err(MmcHostError::ResponseTimeout) => return Err(MmcDiscoveryError::NoCard),
            Err(error) => return Err(MmcDiscoveryError::Transport(error)),
        }
        let response = SdR1Response::decode(app.response[0]);
        if response.flags.intersects(SdR1Flags::ERRORS) {
            kerrln!(
                "mmc host{}: SD CMD55 rejected: response={:#010x}, error_bits={:#010x}",
                host.id().get(),
                app.response[0],
                response.flags.bits() & SdR1Flags::ERRORS.bits()
            );
            return Err(MmcDiscoveryError::ProtocolRejected(MmcCardKind::SdMemory));
        }
        if !response.flags.contains(SdR1Flags::APP_COMMAND) {
            kerrln!(
                "mmc host{}: SD CMD55 rejected: APP_CMD is clear, response={:#010x}",
                host.id().get(),
                app.response[0]
            );
            return Err(MmcDiscoveryError::ProtocolRejected(MmcCardKind::SdMemory));
        }

        // step 5.2: Request the supported voltage window and HCS only for a
        // card that passed the SD v2 interface-condition check.
        let mut operating_condition = MmcCommand::new(
            SdApplicationCommand::SendOperatingCondition as u8,
            request.bits(),
            MmcResponseType::R3,
        );
        match execute_required(host, &mut operating_condition) {
            Ok(()) => {},
            Err(MmcHostError::ResponseTimeout) => return Err(MmcDiscoveryError::NoCard),
            Err(error) => return Err(MmcDiscoveryError::Transport(error)),
        }
        let response = SdOperatingConditions::from_bits_retain(operating_condition.response[0]);

        // step 5.3: Commit OCR only after power-up completion and voltage
        // compatibility; otherwise retry under the configured total deadline.
        if response.contains(SdOperatingConditions::POWER_UP_COMPLETE) {
            if !response.intersects(SdOperatingConditions::VOLTAGE_2_7_TO_3_6) {
                kerrln!(
                    "mmc host{}: SD ACMD41 returned an incompatible OCR: response={:#010x}",
                    host.id().get(),
                    operating_condition.response[0]
                );
                return Err(MmcDiscoveryError::InvalidIdentity);
            }
            return Ok(response);
        }
        if start.elapsed() >= timeout {
            kerrln!(
                "mmc host{}: SD ACMD41 initialization timed out: last_response={:#010x}",
                host.id().get(),
                operating_condition.response[0]
            );
            return Err(MmcDiscoveryError::InitializationTimeout);
        }
        busy_delay(Duration::from_millis(MMC_CARD_INIT_POLL_INTERVAL_MS));
    }
}

fn decode_csd(
    operating_conditions: SdOperatingConditions,
    csd: SdCsd,
) -> Result<(SdAddressing, u64), MmcDiscoveryError> {
    let csd = csd.words();
    let structure = SdCsdStructure::decode(response_bits(csd, 127, 126))
        .ok_or(MmcDiscoveryError::InvalidIdentity)?;
    let high_capacity = operating_conditions.contains(SdOperatingConditions::CAPACITY_STATUS);
    let (addressing, capacity_bytes) = match (structure, high_capacity) {
        (SdCsdStructure::Version1, false) => {
            let read_block_len = response_bits(csd, 83, 80);
            if !(9..=11).contains(&read_block_len) {
                return Err(MmcDiscoveryError::InvalidIdentity);
            }
            let c_size = response_bits(csd, 73, 62) as u64;
            let c_size_mult = response_bits(csd, 49, 47);
            let block_len = 1u64
                .checked_shl(read_block_len)
                .ok_or(MmcDiscoveryError::InvalidIdentity)?;
            let multiplier = 1u64
                .checked_shl(c_size_mult + 2)
                .ok_or(MmcDiscoveryError::InvalidIdentity)?;
            let blocks = (c_size + 1)
                .checked_mul(multiplier)
                .ok_or(MmcDiscoveryError::InvalidIdentity)?;
            let capacity = blocks
                .checked_mul(block_len)
                .ok_or(MmcDiscoveryError::InvalidIdentity)?;
            (SdAddressing::Byte, capacity)
        },
        (SdCsdStructure::Version2, true) => {
            let c_size = response_bits(csd, 69, 48) as u64;
            let capacity = (c_size + 1)
                .checked_mul(512 * 1024)
                .ok_or(MmcDiscoveryError::InvalidIdentity)?;
            (SdAddressing::Block, capacity)
        },
        (SdCsdStructure::Version3, _) => {
            kerrln!(
                "mmc: TODO(stage 3+): SDUC addressing is currently not supported: csd_structure={}",
                structure as u8
            );
            return Err(MmcDiscoveryError::UnsupportedCard);
        },
        _ => return Err(MmcDiscoveryError::InvalidIdentity),
    };
    if capacity_bytes == 0 || !capacity_bytes.is_multiple_of(512) {
        return Err(MmcDiscoveryError::InvalidIdentity);
    }
    if addressing == SdAddressing::Byte && !sdsc_capacity_is_addressable(capacity_bytes) {
        return Err(MmcDiscoveryError::InvalidIdentity);
    }
    Ok((addressing, capacity_bytes))
}

fn sdsc_capacity_is_addressable(capacity_bytes: u64) -> bool {
    // SDSC commands carry the start byte in a u32 argument. Because committed
    // capacities are 512-byte aligned, 2^32 bytes is the largest geometry whose
    // final logical block still has an encodable start address.
    capacity_bytes <= u32::MAX as u64 + 1
}

fn response_bits(response: [u32; 4], msb: u32, lsb: u32) -> u32 {
    assert!(msb < 128 && lsb <= msb);
    let width = msb - lsb + 1;
    assert!(width <= 32);
    let response = ((response[0] as u128) << 96)
        | ((response[1] as u128) << 64)
        | ((response[2] as u128) << 32)
        | response[3] as u128;
    let mask = if width == 32 {
        u32::MAX as u128
    } else {
        (1u128 << width) - 1
    };
    ((response >> lsb) & mask) as u32
}

fn execute(host: &MmcHostDevice, command: &mut MmcCommand) -> Result<(), MmcHostError> {
    let mut request = MmcRequest {
        command: *command,
        data: None,
        stop: None,
    };
    host.execute(&mut request)?;
    *command = request.command;
    Ok(())
}

fn send_go_idle(host: &MmcHostDevice) -> Result<(), MmcDiscoveryError> {
    let mut command = MmcCommand::new(SdCommand::GoIdle as u8, 0, MmcResponseType::None);
    command.flags = MmcCommandFlags::INITIALIZATION_CLOCKS | MmcCommandFlags::STOP_ABORT;
    execute_required(host, &mut command).map_err(MmcDiscoveryError::Transport)
}

/// Recover transport and restart identification after an optional protocol
/// probe was rejected by response timeout.
///
/// `MmcHost` deliberately separates transport recovery from card-session
/// policy. A timeout is useful candidate evidence, but no later command may
/// reuse that session until the transport is recovered and CMD0 establishes a
/// new protocol baseline. Cold discovery is the sole caller before card
/// publication, so no block endpoint can enter between recovery and CMD0.
fn restart_identification_after_probe_timeout(
    host: &MmcHostDevice,
    probe_opcode: u8,
) -> Result<(), MmcDiscoveryError> {
    kdebugln!(
        "mmc host{}: SD CMD{} response timeout rejected an optional probe; recovering transport before continuing",
        host.id().get(),
        probe_opcode
    );
    host.recover_transport()
        .map_err(MmcDiscoveryError::Transport)?;
    busy_delay(host.caps().post_power_on_delay);
    send_go_idle(host)
}

/// Execute a command whose failure is fatal to the active SD attach attempt.
/// Optional discriminator commands use `execute` directly because a response
/// timeout is an expected protocol result for those probes.
fn execute_required(host: &MmcHostDevice, command: &mut MmcCommand) -> Result<(), MmcHostError> {
    match execute(host, command) {
        Ok(()) => Ok(()),
        Err(error) => {
            log_command_error(host, command, error);
            Err(error)
        },
    }
}

fn log_command_error(host: &MmcHostDevice, command: &MmcCommand, error: MmcHostError) {
    kerrln!(
        "mmc host{}: SD CMD{} failed: {:?}",
        host.id().get(),
        command.opcode,
        error
    );
}

pub(crate) fn command_argument(
    addressing: SdAddressing,
    lba: usize,
) -> Result<u32, SdProtocolError> {
    let address = match addressing {
        SdAddressing::Byte => lba
            .checked_mul(512)
            .ok_or(SdProtocolError::AddressOverflow)?,
        SdAddressing::Block => lba,
    };
    u32::try_from(address).map_err(|_| SdProtocolError::AddressOverflow)
}

fn busy_delay(duration: Duration) {
    let start = Instant::now();
    while start.elapsed() < duration {
        core::hint::spin_loop();
    }
}

#[kunit]
fn r1_check_rejects_every_card_status_error() {
    let error_flags = [
        SdR1Flags::OUT_OF_RANGE,
        SdR1Flags::ADDRESS_ERROR,
        SdR1Flags::BLOCK_LEN_ERROR,
        SdR1Flags::ERASE_SEQ_ERROR,
        SdR1Flags::ERASE_PARAM,
        SdR1Flags::WP_VIOLATION,
        SdR1Flags::CARD_IS_LOCKED,
        SdR1Flags::LOCK_UNLOCK_FAILED,
        SdR1Flags::COM_CRC_ERROR,
        SdR1Flags::ILLEGAL_COMMAND,
        SdR1Flags::CARD_ECC_FAILED,
        SdR1Flags::CC_ERROR,
        SdR1Flags::ERROR,
        SdR1Flags::UNDERRUN,
        SdR1Flags::OVERRUN,
        SdR1Flags::CSD_OVERWRITE,
        SdR1Flags::WP_ERASE_SKIP,
        SdR1Flags::SWITCH_ERROR,
        SdR1Flags::AKE_SEQ_ERROR,
    ];
    let mut enumerated_errors = SdR1Flags::empty();

    for error in error_flags {
        enumerated_errors |= error;
        assert_eq!(
            SdR1Response::decode(error.bits()).check(),
            Err(SdProtocolError::CardStatus(error.bits()))
        );
    }
    assert_eq!(enumerated_errors, SdR1Flags::ERRORS);

    let ready_for_transfer =
        SdR1Flags::READY_FOR_DATA.bits() | SdCardState::Transfer.response_bits();
    let response = SdR1Response::decode(ready_for_transfer);
    assert_eq!(response.card_state, Some(SdCardState::Transfer));
    assert_eq!(response.check(), Ok(()));
}

#[kunit]
fn csd_v2_capacity_is_checked() {
    // CSD_STRUCTURE=1 and C_SIZE=0x1fff describe 4 GiB.
    let value = (1u128 << 126) | (0x1fffu128 << 48);
    let csd = [
        (value >> 96) as u32,
        (value >> 64) as u32,
        (value >> 32) as u32,
        value as u32,
    ];
    let operating_conditions =
        SdOperatingConditions::POWER_UP_COMPLETE | SdOperatingConditions::CAPACITY_STATUS;
    assert_eq!(
        decode_csd(operating_conditions, SdCsd::from_response(csd)),
        Ok((SdAddressing::Block, 4 * 1024 * 1024 * 1024))
    );
}

#[kunit]
fn csd_v1_capacity_and_block_length_are_checked() {
    let csd_v1 = |read_block_len: u32| {
        let value = ((read_block_len as u128) << 80) | ((0x0fffu128) << 62) | ((7u128) << 47);
        SdCsd::from_response([
            (value >> 96) as u32,
            (value >> 64) as u32,
            (value >> 32) as u32,
            value as u32,
        ])
    };
    let operating_conditions = SdOperatingConditions::POWER_UP_COMPLETE;

    assert_eq!(
        decode_csd(operating_conditions, csd_v1(9)),
        Ok((SdAddressing::Byte, 1024 * 1024 * 1024))
    );
    assert_eq!(
        decode_csd(operating_conditions, csd_v1(12)),
        Err(MmcDiscoveryError::InvalidIdentity)
    );
    assert!(sdsc_capacity_is_addressable(1u64 << 32));
    assert!(!sdsc_capacity_is_addressable((1u64 << 32) + 512));
}

#[kunit]
fn command_address_uses_card_capacity_mode() {
    assert_eq!(command_argument(SdAddressing::Byte, 7), Ok(7 * 512));
    assert_eq!(command_argument(SdAddressing::Block, 7), Ok(7));
}
