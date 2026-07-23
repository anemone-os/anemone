use crate::{mm::remap::IoRemap, prelude::*};

/// AHCI exposes at most 32 ports in the Ports Implemented bitmap.
pub(super) const MAX_PORTS: usize = 32;

/// Fixed offsets and stride defined by the AHCI 1.x memory layout.
#[derive(Clone, Copy, Debug)]
#[repr(usize)]
enum RegisterLayout {
    PortBlock = 0x100,
    PortStride = 0x80,
}

#[derive(Clone, Copy, Debug)]
#[repr(usize)]
/// Generic host register offsets from the AHCI MMIO base.
pub(super) enum HostRegister {
    Capabilities = 0x00,
    GlobalControl = 0x04,
    InterruptStatus = 0x08,
    PortsImplemented = 0x0c,
    Version = 0x10,
    ExtendedCapabilities = 0x24,
}

#[derive(Clone, Copy, Debug)]
#[repr(usize)]
/// Per-port register offsets from an AHCI port block.
pub(super) enum PortRegister {
    CommandListBase = 0x00,
    CommandListBaseUpper = 0x04,
    ReceivedFisBase = 0x08,
    ReceivedFisBaseUpper = 0x0c,
    InterruptStatus = 0x10,
    InterruptEnable = 0x14,
    Command = 0x18,
    TaskFileData = 0x20,
    Signature = 0x24,
    SataStatus = 0x28,
    SataControl = 0x2c,
    SataError = 0x30,
    SataActive = 0x34,
    CommandIssue = 0x38,
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    /// Controller capability bits returned by the CAP register.
    pub(super) struct HostCapabilityFlags: u32 {
        /// HBA accepts 64-bit command-list and data addresses.
        const SUPPORTS_64_BIT = 1 << 31;
        /// HBA supports staggered spin-up through PxCMD.SUD.
        const STAGGERED_SPIN_UP = 1 << 27;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    /// Global Host Control register bits.
    pub(super) struct GlobalControl: u32 {
        /// Requests an HBA reset.
        const RESET = 1 << 0;
        /// Enables delivery of HBA interrupts.
        const INTERRUPT_ENABLE = 1 << 1;
        /// Selects AHCI operation rather than a legacy interface.
        const AHCI_ENABLE = 1 << 31;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    /// Per-port command and engine-state bits.
    pub(super) struct PortCommand: u32 {
        /// Starts command-list processing.
        const START = 1 << 0;
        /// Requests device spin-up when CAP.SSS is implemented.
        const SPIN_UP = 1 << 1;
        /// Requests port power; retained for complete register modeling.
        const POWER_ON = 1 << 2;
        /// Overrides a busy task file; retained for complete register modeling.
        const COMMAND_LIST_OVERRIDE = 1 << 3;
        /// Enables DMA reception of device-to-host FISes.
        const FIS_RECEIVE_ENABLE = 1 << 4;
        /// Reports that the FIS receive engine is running.
        const FIS_RECEIVE_RUNNING = 1 << 14;
        /// Reports that the command-list engine is running.
        const COMMAND_LIST_RUNNING = 1 << 15;
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    /// Per-port interrupt status and enable bits.
    pub(super) struct PortInterrupt: u32 {
        /// Device-to-host register FIS received.
        const D2H_REGISTER_FIS = 1 << 0;
        /// PIO setup FIS received.
        const PIO_SETUP_FIS = 1 << 1;
        /// DMA setup FIS received.
        const DMA_SETUP_FIS = 1 << 2;
        /// Set-device-bits FIS received.
        const SET_DEVICE_BITS_FIS = 1 << 3;
        /// Unknown FIS received.
        const UNKNOWN_FIS = 1 << 4;
        /// Physical region descriptor processed.
        const DESCRIPTOR_PROCESSED = 1 << 5;
        /// Port connection status changed.
        const PORT_CONNECT_CHANGE = 1 << 6;
        /// Mechanical-presence switch changed.
        const DEVICE_MECHANICAL_PRESENCE = 1 << 7;
        /// PHY ready state changed.
        const PHY_READY_CHANGE = 1 << 22;
        /// Port multiplier reported an invalid status.
        const BAD_PORT_MULTIPLIER = 1 << 23;
        /// Received-FIS area overflowed.
        const OVERFLOW = 1 << 24;
        /// Interface reported a recoverable error.
        const INTERFACE_NON_FATAL = 1 << 26;
        /// Interface reported a fatal error.
        const INTERFACE_FATAL = 1 << 27;
        /// Host bus data transaction failed.
        const HOST_BUS_DATA = 1 << 28;
        /// Host bus transaction failed fatally.
        const HOST_BUS_FATAL = 1 << 29;
        /// ATA task file reported an error.
        const TASK_FILE_ERROR = 1 << 30;
        /// Cold-presence state changed.
        const COLD_PRESENCE = 1 << 31;
    }
}

impl PortInterrupt {
    /// Interrupts that require stopping the command engine.
    pub(super) const FATAL: Self = Self::INTERFACE_FATAL
        .union(Self::HOST_BUS_DATA)
        .union(Self::HOST_BUS_FATAL)
        .union(Self::TASK_FILE_ERROR);

    /// Interrupts treated as command errors by the polling path.
    pub(super) const ERRORS: Self = Self::FATAL
        .union(Self::INTERFACE_NON_FATAL)
        .union(Self::OVERFLOW)
        .union(Self::UNKNOWN_FIS)
        .union(Self::BAD_PORT_MULTIPLIER);
}

/// Typed view of the HBA capabilities register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HostCapabilities(u32);

impl HostCapabilities {
    /// Wraps the raw CAP register without losing unknown capability bits.
    pub(super) const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the unmodified CAP register value for diagnostics.
    pub(super) const fn raw(self) -> u32 {
        self.0
    }

    /// Returns the number of implemented port slots encoded as N-1.
    pub(super) const fn ports(self) -> u8 {
        ((self.0 & 0x1f) + 1) as u8
    }

    /// Returns the number of command slots encoded as N-1.
    pub(super) const fn command_slots(self) -> u8 {
        (((self.0 >> 8) & 0x1f) + 1) as u8
    }

    /// Reports whether 64-bit command and data addresses are supported.
    pub(super) const fn supports_64_bit(self) -> bool {
        HostCapabilityFlags::from_bits_retain(self.0).contains(HostCapabilityFlags::SUPPORTS_64_BIT)
    }

    /// Reports whether the HBA supports staggered device spin-up.
    pub(super) const fn staggered_spin_up(self) -> bool {
        HostCapabilityFlags::from_bits_retain(self.0)
            .contains(HostCapabilityFlags::STAGGERED_SPIN_UP)
    }
}

/// Task-file status bits sampled while polling a command.
bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct TaskFileStatus: u32 {
        /// ATA status ERR bit.
        const ERROR = 1 << 0;
        /// ATA status DRQ bit.
        const DATA_REQUEST = 1 << 3;
        /// ATA status BSY bit.
        const BUSY = 1 << 7;
    }

    /// Per-port command-issue bits. The current driver owns slot zero only.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) struct CommandIssue: u32 {
        /// Command slot zero is active.
        const SLOT_ZERO = 1 << 0;
    }
}

/// Typed view of the task-file status register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TaskFileData(u32);

impl TaskFileData {
    /// Wraps the raw task-file status register.
    pub(super) const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the unmodified task-file status for diagnostics.
    pub(super) const fn raw(self) -> u32 {
        self.0
    }

    /// Returns true when the device is neither busy, requesting data, nor in error.
    pub(super) const fn ready(self) -> bool {
        !TaskFileStatus::from_bits_retain(self.0)
            .intersects(TaskFileStatus::from_bits_retain(
                TaskFileStatus::BUSY.bits()
                    | TaskFileStatus::DATA_REQUEST.bits()
                    | TaskFileStatus::ERROR.bits(),
            ))
    }

    /// Returns true while the device cannot accept a new command.
    pub(super) const fn busy_or_drq(self) -> bool {
        TaskFileStatus::from_bits_retain(self.0)
            .intersects(TaskFileStatus::from_bits_retain(
                TaskFileStatus::BUSY.bits() | TaskFileStatus::DATA_REQUEST.bits(),
            ))
    }

    /// Returns true when the task-file error bit is set.
    pub(super) const fn error(self) -> bool {
        TaskFileStatus::from_bits_retain(self.0).contains(TaskFileStatus::ERROR)
    }
}

/// Typed view of the SATA status register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SataStatus(u32);

impl SataStatus {
    /// Wraps the raw SATA status register.
    pub(super) const fn new(raw: u32) -> Self {
        Self(raw)
    }

    /// Returns the unmodified SATA status for diagnostics.
    pub(super) const fn raw(self) -> u32 {
        self.0
    }

    /// Reports whether the link has reached the device-present state.
    pub(super) const fn device_present(self) -> bool {
        self.0 & 0x0f == 0x03
    }

    /// Returns the negotiated SATA link speed generation.
    pub(super) const fn speed(self) -> u8 {
        ((self.0 >> 4) & 0x0f) as u8
    }
}

/// Bounds-checked volatile view over one AHCI MMIO resource.
pub(super) struct AhciRegs {
    remap: IoRemap,
}

impl AhciRegs {
    /// Minimum MMIO mapping that covers the host registers used by this driver.
    pub(super) const BASELINE_MAPPING_LEN: usize =
        HostRegister::ExtendedCapabilities as usize + core::mem::size_of::<u32>();

    /// Creates an MMIO register view after validating the required host window.
    pub(super) fn new(remap: IoRemap) -> Result<Self, SysError> {
        if remap.size() < Self::BASELINE_MAPPING_LEN as u64 {
            return Err(SysError::DriverIncompatible);
        }
        Ok(Self { remap })
    }

    /// Returns the physical base supplied by the platform resource.
    pub(super) fn phys_base(&self) -> PhysAddr {
        self.remap.phys_base()
    }

    /// Returns the mapped MMIO length supplied by the platform resource.
    pub(super) fn size(&self) -> usize {
        self.remap.size() as usize
    }

    /// Verifies that one complete port register block fits in the mapping.
    pub(super) fn validate_port(&self, port: usize) -> Result<(), SysError> {
        let end = port_window_end(port).ok_or(SysError::DriverIncompatible)?;
        if end > self.size() {
            return Err(SysError::DriverIncompatible);
        }
        Ok(())
    }

    /// Computes a checked pointer into the controller's MMIO mapping.
    fn ptr_at<T>(&self, offset: usize) -> *mut T {
        let end = offset
            .checked_add(core::mem::size_of::<T>())
            .expect("AHCI MMIO offset overflow");
        assert!(end <= self.size(), "AHCI MMIO access outside mapping");
        assert!(offset.is_multiple_of(core::mem::align_of::<T>()));
        unsafe { self.remap.as_ptr().as_ptr().cast::<u8>().add(offset).cast() }
    }

    /// Computes a port register offset from the AHCI port layout.
    fn port_offset(port: usize, register: PortRegister) -> usize {
        assert!(port < MAX_PORTS);
        RegisterLayout::PortBlock as usize
            + port * RegisterLayout::PortStride as usize
            + register as usize
    }

    /// Reads one host register using volatile MMIO semantics.
    pub(super) fn read_host(&self, register: HostRegister) -> u32 {
        unsafe { core::ptr::read_volatile(self.ptr_at(register as usize)) }
    }

    /// Writes one host register using volatile MMIO semantics.
    pub(super) fn write_host(&self, register: HostRegister, value: u32) {
        unsafe { core::ptr::write_volatile(self.ptr_at(register as usize), value) }
    }

    /// Applies a read-modify-write update to one host register.
    pub(super) fn update_host(&self, register: HostRegister, clear: u32, set: u32) {
        self.write_host(register, (self.read_host(register) & !clear) | set);
    }

    /// Reads one port register using volatile MMIO semantics.
    pub(super) fn read_port(&self, port: usize, register: PortRegister) -> u32 {
        unsafe { core::ptr::read_volatile(self.ptr_at(Self::port_offset(port, register))) }
    }

    /// Writes one port register using volatile MMIO semantics.
    pub(super) fn write_port(&self, port: usize, register: PortRegister, value: u32) {
        unsafe { core::ptr::write_volatile(self.ptr_at(Self::port_offset(port, register)), value) }
    }

    /// Applies a read-modify-write update to one port register.
    pub(super) fn update_port(&self, port: usize, register: PortRegister, clear: u32, set: u32) {
        self.write_port(
            port,
            register,
            (self.read_port(port, register) & !clear) | set,
        );
    }

    /// Acknowledges host interrupt status bits using AHCI write-one-to-clear semantics.
    pub(super) fn acknowledge_host_interrupts(&self, bits: u32) {
        self.write_host(HostRegister::InterruptStatus, bits);
    }

    /// Acknowledges the host interrupt bit corresponding to one port index.
    pub(super) fn acknowledge_port_host_interrupt(&self, port: usize) {
        assert!(port < MAX_PORTS);
        self.acknowledge_host_interrupts(1u32 << port);
    }

    /// Acknowledges port interrupt status bits using AHCI write-one-to-clear semantics.
    pub(super) fn acknowledge_port_interrupts(&self, port: usize, bits: PortInterrupt) {
        self.write_port(port, PortRegister::InterruptStatus, bits.bits());
    }

    /// Acknowledges SATA error bits using AHCI write-one-to-clear semantics.
    pub(super) fn acknowledge_sata_errors(&self, port: usize, bits: u32) {
        self.write_port(port, PortRegister::SataError, bits);
    }

    /// Reads and models the port task-file status register.
    pub(super) fn task_file(&self, port: usize) -> TaskFileData {
        TaskFileData::new(self.read_port(port, PortRegister::TaskFileData))
    }

    /// Reads and models the port SATA status register.
    pub(super) fn sata_status(&self, port: usize) -> SataStatus {
        SataStatus::new(self.read_port(port, PortRegister::SataStatus))
    }
}

/// Returns the exclusive end of a complete port register block.
fn port_window_end(port: usize) -> Option<usize> {
    if port >= MAX_PORTS {
        return None;
    }
    (RegisterLayout::PortBlock as usize)
        .checked_add((port + 1).checked_mul(RegisterLayout::PortStride as usize)?)
}

#[kunit]
/// Checks the AHCI N-1 encoding used by CAP.NP and CAP.NCS.
fn capability_fields_are_encoded_plus_one() {
    let cap = HostCapabilities::new((7 << 8) | 1);
    assert_eq!(cap.command_slots(), 8);
    assert_eq!(cap.ports(), 2);
    assert!(!cap.supports_64_bit());
}

#[kunit]
/// Checks generic register windows without relying on a board MMIO address.
fn mapping_length_and_port_windows_are_generic() {
    assert_eq!(AhciRegs::BASELINE_MAPPING_LEN, 0x28);
    assert_eq!(AhciRegs::port_offset(0, PortRegister::CommandIssue), 0x138);
    assert_eq!(port_window_end(31), Some(0x1100));
    assert_eq!(port_window_end(32), None);
}
