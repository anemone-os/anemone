use super::registry::MmcHostId;
use crate::{
    device::kobject::{KObjectBase, KObjectOps},
    prelude::*,
};

bitflags! {
    /// Card protocol families firmware permits a future MMC core to probe.
    ///
    /// These bits are capabilities, not detected card identity.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct MmcCardKinds: u8 {
        const SD_MEMORY = 1 << 0;
        const MMC = 1 << 1;
        const SDIO = 1 << 2;
    }
}

bitflags! {
    /// Bus widths that the board wiring and current host implementation can
    /// actually provide. Wider widths include the narrower fallback modes.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct MmcBusWidths: u8 {
        const ONE = 1 << 0;
        const FOUR = 1 << 1;
        const EIGHT = 1 << 2;
    }
}

bitflags! {
    /// Signal voltages the complete host/board integration can produce.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct MmcSignalVoltages: u8 {
        const V3_3 = 1 << 0;
        const V1_8 = 1 << 1;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Effective host limits after intersecting firmware configuration,
/// synthesized controller capability, and implemented driver behavior.
///
/// This is not a detected-card description. In particular, `allowed_kinds`
/// only constrains which protocol families a future discovery owner may try.
pub struct MmcHostCaps {
    pub allowed_kinds: MmcCardKinds,
    pub bus_widths: MmcBusWidths,
    pub min_clock_hz: u32,
    pub max_clock_hz: u32,
    pub signal_voltages: MmcSignalVoltages,
    pub max_block_size: u32,
    pub max_block_count: u32,
    pub max_request_bytes: usize,
    pub removable: bool,
    /// A future protocol owner must wait this long after `PowerMode::Up`
    /// before issuing card commands. `set_ios` only applies host registers;
    /// the caller performs the delay after the controller operation returns.
    pub post_power_on_delay: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MmcPowerMode {
    /// Card power and clock are disabled.
    Off,
    /// Initial power has been applied; protocol commands must wait for the
    /// advertised post-power-on delay before use.
    Up,
    /// The host is configured for normal command/data transfers.
    On,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MmcBusWidth {
    One,
    Four,
    Eight,
}

impl MmcBusWidth {
    pub const fn capability(self) -> MmcBusWidths {
        match self {
            Self::One => MmcBusWidths::ONE,
            Self::Four => MmcBusWidths::FOUR,
            Self::Eight => MmcBusWidths::EIGHT,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MmcSignalVoltage {
    V3_3,
    V1_8,
}

impl MmcSignalVoltage {
    pub const fn capability(self) -> MmcSignalVoltages {
        match self {
            Self::V3_3 => MmcSignalVoltages::V3_3,
            Self::V1_8 => MmcSignalVoltages::V1_8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Host timing mode. Stage 1 exposes future variants in the contract but the
/// DW-MSHC implementation accepts only `Legacy` until switching is implemented.
pub enum MmcTiming {
    Legacy,
    SdHighSpeed,
    MmcHighSpeed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MmcIos {
    pub power_mode: MmcPowerMode,
    pub clock_hz: u32,
    pub bus_width: MmcBusWidth,
    pub signal_voltage: MmcSignalVoltage,
    pub timing: MmcTiming,
}

impl MmcIos {
    pub const OFF: Self = Self {
        power_mode: MmcPowerMode::Off,
        clock_hz: 0,
        bus_width: MmcBusWidth::One,
        signal_voltage: MmcSignalVoltage::V3_3,
        timing: MmcTiming::Legacy,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Controller-level response framing selected by the protocol owner.
///
/// The host translates framing and CRC requirements but does not interpret
/// OCR, card status, RCA, CID, or CSD contents.
pub enum MmcResponseType {
    None,
    R1,
    R1b,
    R2,
    R3,
    R4,
    R5,
    R6,
    R7,
}

bitflags! {
    /// Protocol-provided command framing that cannot be inferred from an
    /// opcode without leaking card semantics into a host controller driver.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct MmcCommandFlags: u8 {
        const INITIALIZATION_CLOCKS = 1 << 0;
        const STOP_ABORT = 1 << 1;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MmcCommand {
    pub opcode: u8,
    pub argument: u32,
    pub response_type: MmcResponseType,
    pub flags: MmcCommandFlags,
    /// Canonical response order: `[127:96]`, `[95:64]`, `[63:32]`,
    /// `[31:0]`. Short responses use element zero and clear the remainder.
    pub response: [u32; 4],
}

impl MmcCommand {
    pub const fn new(opcode: u8, argument: u32, response_type: MmcResponseType) -> Self {
        Self {
            opcode,
            argument,
            response_type,
            flags: MmcCommandFlags::empty(),
            response: [0; 4],
        }
    }
}

#[derive(Debug)]
/// Borrowed data phase for one synchronous request.
///
/// The host may access these buffers only until `MmcHost::execute` returns;
/// retaining their addresses for later work is forbidden by this lifetime.
pub enum MmcData<'a> {
    Read {
        block_size: u32,
        blocks: u32,
        buffer: &'a mut [u8],
    },
    Write {
        block_size: u32,
        blocks: u32,
        buffer: &'a [u8],
    },
}

impl MmcData<'_> {
    pub const fn block_size(&self) -> u32 {
        match self {
            Self::Read { block_size, .. } | Self::Write { block_size, .. } => *block_size,
        }
    }

    pub const fn blocks(&self) -> u32 {
        match self {
            Self::Read { blocks, .. } | Self::Write { blocks, .. } => *blocks,
        }
    }

    pub const fn buffer_len(&self) -> usize {
        match self {
            Self::Read { buffer, .. } => buffer.len(),
            Self::Write { buffer, .. } => buffer.len(),
        }
    }
}

#[derive(Debug)]
pub struct MmcRequest<'a> {
    pub command: MmcCommand,
    pub data: Option<MmcData<'a>>,
    /// Explicit stop commands are represented now so the protocol layer does
    /// not need a later request ABI change. A host may reject them when its
    /// advertised `max_block_count` is one.
    pub stop: Option<MmcCommand>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Errors at the host-controller boundary. Card-session policy such as retry,
/// power cycling, or re-enumeration belongs to the future MMC core.
pub enum MmcHostError {
    UnsupportedIos,
    UnsupportedRequest,
    InvalidRequest,
    CommandTimeout,
    ResponseCrc,
    ResponseFraming,
    DataTimeout,
    DataCrc,
    FifoRun,
    HardwareLocked,
    ShortTransfer,
    ControllerOffline,
}

/// A protocol-neutral host/slot facade.
///
/// Implementations serialize access to their underlying controller. The
/// request and its borrowed data buffers may not be retained after `execute`
/// returns.
pub trait MmcHost: Send + Sync {
    fn caps(&self) -> MmcHostCaps;

    /// Apply host-side I/O settings and return the settings actually produced
    /// by the hardware (notably the rounded clock frequency).
    fn set_ios(&self, ios: MmcIos) -> Result<MmcIos, MmcHostError>;

    fn execute(&self, request: &mut MmcRequest<'_>) -> Result<(), MmcHostError>;

    /// Restore the host controller to a state that can accept another
    /// request. Card-session recovery remains a future MMC-core decision.
    fn recover(&self) -> Result<(), MmcHostError>;
}

/// Device-model representation of one published MMC host/slot.
///
/// The concrete host implementation owns controller-facing behavior. This
/// wrapper owns the stable device identity and parent relationship used by
/// future card devices.
#[derive(KObject, Device)]
pub struct MmcHostDevice {
    #[kobject]
    kobj_base: KObjectBase,
    #[device]
    dev_base: DeviceBase,
    /// Stable kernel-local identity; it is neither a firmware alias nor a
    /// future block-device number.
    id: MmcHostId,
    /// Concrete controller facade. The device wrapper owns identity and
    /// hierarchy only; it does not duplicate controller state.
    ops: Arc<dyn MmcHost>,
}

impl KObjectOps for MmcHostDevice {}

impl DeviceOps for MmcHostDevice {}

impl MmcHostDevice {
    pub(crate) fn new(
        kobj_base: KObjectBase,
        dev_base: DeviceBase,
        id: MmcHostId,
        ops: Arc<dyn MmcHost>,
    ) -> Self {
        Self {
            kobj_base,
            dev_base,
            id,
            ops,
        }
    }

    pub const fn id(&self) -> MmcHostId {
        self.id
    }
}

impl MmcHost for MmcHostDevice {
    fn caps(&self) -> MmcHostCaps {
        self.ops.caps()
    }

    fn set_ios(&self, ios: MmcIos) -> Result<MmcIos, MmcHostError> {
        self.ops.set_ios(ios)
    }

    fn execute(&self, request: &mut MmcRequest<'_>) -> Result<(), MmcHostError> {
        self.ops.execute(request)
    }

    fn recover(&self) -> Result<(), MmcHostError> {
        self.ops.recover()
    }
}
