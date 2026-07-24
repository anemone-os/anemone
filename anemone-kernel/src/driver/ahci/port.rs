use super::{
    ata::{AtaIdentity, SECTOR_BYTES, parse_identify},
    dma::AhciPortDma,
    fis::{COMMAND_FIS_BYTES, identify_fis, read_dma_ext_fis, write_dma_ext_fis},
    platform::AhciPlatformConfig,
    regs::{
        AhciRegs, CommandIssue, GlobalControl, HostCapabilities, HostRegister, PortCommand,
        PortInterrupt, PortRegister,
    },
};
use crate::{mm::remap::IoRemap, prelude::*};

/// The ATA device signature returned by a SATA disk after link setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
enum DeviceSignature {
    Ata = 0x0000_0101,
}

/// Minimum COMRESET assertion interval required by the SATA protocol.
const COMRESET_ASSERT_MS: u64 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Driver-owned lifecycle of the single active port.
enum PortReadiness {
    Probing,
    Ready,
    Recovering,
    Offline,
}

#[derive(Clone, Copy, Debug)]
/// Stable command identity retained only for read-timeout diagnostics.
struct AtaReadCommand {
    lba: u64,
    sectors: u16,
}

/// Watchdog state for a synchronous ATA read.
struct AtaReadWatch {
    command: AtaReadCommand,
    start: Instant,
    warned: bool,
}

impl AtaReadWatch {
    /// Starts timing a read for diagnostic timeout reporting.
    fn new(command: AtaReadCommand) -> Self {
        Self {
            command,
            start: Instant::now(),
            warned: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Errors produced by the generic AHCI port state machine.
pub(super) enum AhciError {
    UnsupportedController,
    HbaResetTimeout,
    EngineStopTimeout,
    FisStopTimeout,
    LinkTimeout,
    DeviceBusyTimeout,
    CommandTimeout,
    TaskFile,
    HostBusFatal,
    HostBusData,
    InterfaceFatal,
    InterfaceNonFatal,
    Overflow,
    UnexpectedFis,
    ShortTransfer,
    LinkLost,
    PortOffline,
}

#[derive(Clone, Copy, Debug)]
/// Controller facts collected during probe for the registration log.
pub(super) struct AhciControllerInfo {
    pub resource_base: PhysAddr,
    pub resource_len: usize,
    pub capabilities: u32,
    pub version: u32,
    pub ports_implemented: u32,
    pub port: usize,
    pub command_slots: u8,
    pub link_speed: u8,
    pub effective_dma_mask: u64,
    pub available_physical_address_top: u64,
}

/// Serialized controller facade exposed to the ATA block device.
pub(super) struct AhciController {
    // Stage-1 synchronous serialization: local interrupts remain disabled
    // across one bounded command/DMA polling transaction. Ordinary commands
    // use AHCI_COMMAND_TIMEOUT_MS; ATA reads deliberately panic at
    // AHCI_READ_TIMEOUT_MS for hang diagnosis. Replace this owner before adding
    // IRQ completion, async or multiple outstanding requests, or hotplug.
    inner: SpinLock<AhciPort>,
}

/// Mutable state owned by the controller's synchronous transaction lock.
struct AhciPort {
    regs: AhciRegs,
    dma: AhciPortDma,
    capabilities: HostCapabilities,
    port: usize,
    readiness: PortReadiness,
}

impl AhciController {
    /// Probes one firmware-described AHCI MMIO resource and its supported port.
    pub(super) fn probe(
        remap: IoRemap,
        platform: AhciPlatformConfig,
    ) -> Result<(Arc<Self>, AtaIdentity, AhciControllerInfo), SysError> {
        let regs = AhciRegs::new(remap)?;
        let capabilities = HostCapabilities::new(regs.read_host(HostRegister::Capabilities));
        let version = regs.read_host(HostRegister::Version);
        let ports_implemented = regs.read_host(HostRegister::PortsImplemented);

        if version >> 16 != 1 || ports_implemented.count_ones() != 1 {
            kerrln!(
                "ahci: unsupported baseline layout: cap={:#x} vs={:#x} pi={:#x}; current implementation requires one AHCI 1.x port",
                capabilities.raw(),
                version,
                ports_implemented
            );
            return Err(SysError::DriverIncompatible);
        }
        let port_index = ports_implemented.trailing_zeros() as usize;
        if port_index >= capabilities.ports() as usize || regs.validate_port(port_index).is_err() {
            kerrln!(
                "ahci: implemented port is outside CAP/MMIO window: port={} cap_ports={} mmio_len={:#x}",
                port_index,
                capabilities.ports(),
                regs.size()
            );
            return Err(SysError::DriverIncompatible);
        }

        let dma_window = platform
            .dma_window(capabilities.supports_64_bit())
            .map_err(|_| {
                kerrln!(
                    "ahci: DMA mask cannot cover available memory: top={:#x} dt_mask={:#x} s64a={}",
                    platform.available_physical_address_top,
                    platform.dt_dma_mask,
                    capabilities.supports_64_bit()
                );
                SysError::DriverIncompatible
            })?;
        let dma = AhciPortDma::new(dma_window.effective_mask)?;
        let mut port = AhciPort {
            regs,
            dma,
            capabilities,
            port: port_index,
            readiness: PortReadiness::Probing,
        };
        port.initialize().map_err(|error| {
            port.log_failure("initialize", None, error);
            SysError::ProbeFailed
        })?;

        let mut identify = [0u8; SECTOR_BYTES];
        port.execute_read(&identify_fis(), &mut identify, None)
            .map_err(|error| {
                port.log_failure("identify", None, error);
                SysError::ProbeFailed
            })?;
        let identity = parse_identify(&identify)?;
        port.readiness = PortReadiness::Ready;

        let info = AhciControllerInfo {
            resource_base: port.regs.phys_base(),
            resource_len: port.regs.size(),
            capabilities: capabilities.raw(),
            version,
            ports_implemented,
            port: port_index,
            command_slots: capabilities.command_slots(),
            link_speed: port.regs.sata_status(port_index).speed(),
            effective_dma_mask: dma_window.effective_mask,
            available_physical_address_top: dma_window.available_top,
        };
        Ok((
            Arc::new(Self {
                inner: SpinLock::new(port),
            }),
            identity,
            info,
        ))
    }

    /// Returns the payload limit of the reusable DMA bounce buffer.
    pub(super) fn max_transfer_bytes(&self) -> usize {
        self.inner.lock_irqsave().dma.max_transfer_bytes()
    }

    /// Executes one sector-aligned DMA read after checking port readiness.
    pub(super) fn read(&self, lba: u64, sectors: u16, buffer: &mut [u8]) -> Result<(), SysError> {
        let mut port = self.inner.lock_irqsave();
        port.require_ready().map_err(map_io_error)?;
        let result = port.execute_read(
            &read_dma_ext_fis(lba, sectors),
            buffer,
            Some(AtaReadCommand { lba, sectors }),
        );
        port.finish_request("read", Some(lba), result)
    }

    /// Executes one sector-aligned DMA write after checking port readiness.
    pub(super) fn write(&self, lba: u64, sectors: u16, buffer: &[u8]) -> Result<(), SysError> {
        let mut port = self.inner.lock_irqsave();
        port.require_ready().map_err(map_io_error)?;
        let result = port.execute_write(&write_dma_ext_fis(lba, sectors), buffer);
        port.finish_request("write", Some(lba), result)
    }

    /// Stops the port command engines and prevents later I/O.
    pub(super) fn quiesce(&self) {
        let mut port = self.inner.lock_irqsave();
        if port.stop_engines().is_err() {
            port.log_failure("shutdown", None, AhciError::EngineStopTimeout);
        }
        port.readiness = PortReadiness::Offline;
    }
}

impl AhciPort {
    /// Resets the HBA, configures slot-zero DMA storage, and starts the port.
    fn initialize(&mut self) -> Result<(), AhciError> {
        self.regs.update_host(
            HostRegister::GlobalControl,
            GlobalControl::INTERRUPT_ENABLE.bits(),
            GlobalControl::AHCI_ENABLE.bits(),
        );
        let _ = self.regs.read_host(HostRegister::GlobalControl);
        self.regs
            .update_host(HostRegister::GlobalControl, 0, GlobalControl::RESET.bits());
        let _ = self.regs.read_host(HostRegister::GlobalControl);
        if !self.poll_until(AHCI_HBA_RESET_TIMEOUT_MS, || {
            self.regs.read_host(HostRegister::GlobalControl) & GlobalControl::RESET.bits() == 0
        }) {
            return Err(AhciError::HbaResetTimeout);
        }
        self.regs.update_host(
            HostRegister::GlobalControl,
            GlobalControl::INTERRUPT_ENABLE.bits(),
            GlobalControl::AHCI_ENABLE.bits(),
        );
        self.stop_engines()?;
        self.regs
            .write_port(self.port, PortRegister::InterruptEnable, 0);
        self.program_dma_bases();
        self.clear_status();

        let mut command = PortCommand::FIS_RECEIVE_ENABLE;
        if self.capabilities.staggered_spin_up() {
            command |= PortCommand::SPIN_UP;
        }
        self.regs
            .update_port(self.port, PortRegister::Command, 0, command.bits());
        if !self.poll_until(AHCI_ENGINE_TIMEOUT_MS, || {
            self.regs.read_port(self.port, PortRegister::Command)
                & PortCommand::FIS_RECEIVE_RUNNING.bits()
                != 0
        }) {
            return Err(AhciError::FisStopTimeout);
        }

        if !self.regs.sata_status(self.port).device_present() {
            self.comreset()?;
        }
        if !self.poll_until(AHCI_PORT_TIMEOUT_MS, || {
            self.regs.task_file(self.port).ready()
        }) {
            return Err(AhciError::DeviceBusyTimeout);
        }
        if self.regs.read_port(self.port, PortRegister::Signature) != DeviceSignature::Ata as u32 {
            return Err(AhciError::UnsupportedController);
        }

        self.regs.update_port(
            self.port,
            PortRegister::Command,
            0,
            PortCommand::START.bits(),
        );
        Ok(())
    }

    /// Programs the lower and upper halves of the port DMA base addresses.
    fn program_dma_bases(&self) {
        let command_list = self.dma.command_list_phys();
        let received_fis = self.dma.received_fis_phys();
        self.regs.write_port(
            self.port,
            PortRegister::CommandListBase,
            command_list as u32,
        );
        self.regs.write_port(
            self.port,
            PortRegister::CommandListBaseUpper,
            (command_list >> 32) as u32,
        );
        self.regs.write_port(
            self.port,
            PortRegister::ReceivedFisBase,
            received_fis as u32,
        );
        self.regs.write_port(
            self.port,
            PortRegister::ReceivedFisBaseUpper,
            (received_fis >> 32) as u32,
        );
    }

    /// Stops command-list processing before disabling FIS reception.
    fn stop_engines(&self) -> Result<(), AhciError> {
        self.regs.update_port(
            self.port,
            PortRegister::Command,
            PortCommand::START.bits(),
            0,
        );
        if !self.poll_until(AHCI_ENGINE_TIMEOUT_MS, || {
            self.regs.read_port(self.port, PortRegister::Command)
                & PortCommand::COMMAND_LIST_RUNNING.bits()
                == 0
        }) {
            return Err(AhciError::EngineStopTimeout);
        }

        self.regs.update_port(
            self.port,
            PortRegister::Command,
            PortCommand::FIS_RECEIVE_ENABLE.bits(),
            0,
        );
        if !self.poll_until(AHCI_ENGINE_TIMEOUT_MS, || {
            self.regs.read_port(self.port, PortRegister::Command)
                & PortCommand::FIS_RECEIVE_RUNNING.bits()
                == 0
        }) {
            return Err(AhciError::FisStopTimeout);
        }
        Ok(())
    }

    /// Performs the AHCI SControl COMRESET sequence and waits for link
    /// recovery.
    fn comreset(&self) -> Result<(), AhciError> {
        let baseline = self.regs.read_port(self.port, PortRegister::SataControl) & !0x0f;
        self.regs
            .write_port(self.port, PortRegister::SataControl, baseline | 1);
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(COMRESET_ASSERT_MS) {
            core::hint::spin_loop();
        }
        self.regs
            .write_port(self.port, PortRegister::SataControl, baseline);
        if !self.poll_until(AHCI_PORT_TIMEOUT_MS, || {
            self.regs.sata_status(self.port).device_present()
        }) {
            return Err(AhciError::LinkTimeout);
        }
        self.regs.acknowledge_sata_errors(
            self.port,
            self.regs.read_port(self.port, PortRegister::SataError),
        );
        Ok(())
    }

    /// Prepares, issues, and copies back one DMA read command.
    fn execute_read(
        &mut self,
        command_fis: &[u8; COMMAND_FIS_BYTES],
        destination: &mut [u8],
        read_command: Option<AtaReadCommand>,
    ) -> Result<(), AhciError> {
        // crate::device::console::output("<");
        self.dma.prepare(command_fis, destination.len(), false);
        self.dma.sync_for_device(true);
        let result = self.issue_and_wait(destination.len(), read_command);
        if result.is_ok() {
            self.dma.copy_from_bounce(destination);
        }
        // crate::device::console::output(">");
        result
    }

    /// Copies, prepares, and issues one DMA write command.
    fn execute_write(
        &mut self,
        command_fis: &[u8; COMMAND_FIS_BYTES],
        source: &[u8],
    ) -> Result<(), AhciError> {
        // crate::device::console::output("{");
        self.dma.copy_to_bounce(source);
        self.dma.prepare(command_fis, source.len(), true);
        self.dma.sync_for_device(true);
        let result = self.issue_and_wait(source.len(), None);
        // crate::device::console::output("}");
        result
    }

    /// Issues slot zero and polls its interrupt, task-file, and transfer
    /// status.
    fn issue_and_wait(
        &mut self,
        data_len: usize,
        read_command: Option<AtaReadCommand>,
    ) -> Result<(), AhciError> {
        if CommandIssue::from_bits_retain(
            self.regs.read_port(self.port, PortRegister::CommandIssue),
        )
        .contains(CommandIssue::SLOT_ZERO)
        {
            return Err(AhciError::CommandTimeout);
        }
        let mut read_watch = read_command.map(AtaReadWatch::new);
        if let Some(watch) = read_watch.as_mut() {
            while self.regs.task_file(self.port).busy_or_drq() {
                self.check_read_watch(watch, "device-ready");
                core::hint::spin_loop();
            }
        } else if !self.poll_until(AHCI_PORT_TIMEOUT_MS, || {
            !self.regs.task_file(self.port).busy_or_drq()
        }) {
            return Err(AhciError::DeviceBusyTimeout);
        }

        self.clear_status();
        core::sync::atomic::fence(Ordering::SeqCst);
        self.regs.write_port(
            self.port,
            PortRegister::CommandIssue,
            CommandIssue::SLOT_ZERO.bits(),
        );

        let start = Instant::now();
        let timeout = Duration::from_millis(AHCI_COMMAND_TIMEOUT_MS);
        let completion = loop {
            let interrupts = PortInterrupt::from_bits_retain(
                self.regs
                    .read_port(self.port, PortRegister::InterruptStatus),
            );
            let completed = !CommandIssue::from_bits_retain(
                self.regs.read_port(self.port, PortRegister::CommandIssue),
            )
            .contains(CommandIssue::SLOT_ZERO);
            if let Some(watch) = read_watch.as_mut() {
                self.check_read_watch(watch, "command-completion");
            }
            if interrupts.intersects(PortInterrupt::ERRORS) {
                break Err(classify_interrupt_error(interrupts));
            }
            if completed {
                break Ok(());
            }
            if read_watch.is_none() && start.elapsed() >= timeout {
                break Err(AhciError::CommandTimeout);
            }
            core::hint::spin_loop();
        };

        if let Err(error) = completion {
            return Err(error);
        }

        self.dma.sync_for_cpu(data_len != 0);
        let interrupts = PortInterrupt::from_bits_retain(
            self.regs
                .read_port(self.port, PortRegister::InterruptStatus),
        );
        let task_file = self.regs.task_file(self.port);
        let link_present = self.regs.sata_status(self.port).device_present();
        let transferred = self.dma.transferred_bytes();
        self.clear_status();

        if !link_present {
            return Err(AhciError::LinkLost);
        }
        if interrupts.intersects(PortInterrupt::ERRORS) {
            return Err(classify_interrupt_error(interrupts));
        }
        if task_file.error() {
            return Err(AhciError::TaskFile);
        }
        if data_len != 0 && transferred != data_len {
            return Err(AhciError::ShortTransfer);
        }
        Ok(())
    }

    /// Emits a slow-read warning and preserves a fail-stop timeout diagnostic.
    fn check_read_watch(&self, watch: &mut AtaReadWatch, stage: &'static str) {
        let elapsed = watch.start.elapsed();
        if !watch.warned && elapsed >= Duration::from_millis(AHCI_READ_WARN_MS) {
            watch.warned = true;
            kwarningln!(
                "ahci: slow ATA read port={} lba={} sectors={} stage={} elapsed_ms={} ghc={:#x} is={:#x} pxcmd={:#x} pxis={:#x} pxtfd={:#x} pxsig={:#x} pxssts={:#x} pxserr={:#x} pxci={:#x}",
                self.port,
                watch.command.lba,
                watch.command.sectors,
                stage,
                elapsed.as_millis(),
                self.regs.read_host(HostRegister::GlobalControl),
                self.regs.read_host(HostRegister::InterruptStatus),
                self.regs.read_port(self.port, PortRegister::Command),
                self.regs
                    .read_port(self.port, PortRegister::InterruptStatus),
                self.regs.task_file(self.port).raw(),
                self.regs.read_port(self.port, PortRegister::Signature),
                self.regs.sata_status(self.port).raw(),
                self.regs.read_port(self.port, PortRegister::SataError),
                self.regs.read_port(self.port, PortRegister::CommandIssue),
            );
        }
        if elapsed >= Duration::from_millis(AHCI_READ_TIMEOUT_MS) {
            // This fail-stop policy is intentional diagnostic instrumentation:
            // returning an ordinary I/O error would lose the execution context
            // of a reproducible controller command hang. Remove it when the
            // synchronous polling path gains equivalent post-failure capture.
            panic!(
                "ahci: ATA read timeout port={} lba={} sectors={} stage={} elapsed_ms={} ghc={:#x} is={:#x} pxcmd={:#x} pxis={:#x} pxtfd={:#x} pxsig={:#x} pxssts={:#x} pxserr={:#x} pxci={:#x}",
                self.port,
                watch.command.lba,
                watch.command.sectors,
                stage,
                elapsed.as_millis(),
                self.regs.read_host(HostRegister::GlobalControl),
                self.regs.read_host(HostRegister::InterruptStatus),
                self.regs.read_port(self.port, PortRegister::Command),
                self.regs
                    .read_port(self.port, PortRegister::InterruptStatus),
                self.regs.task_file(self.port).raw(),
                self.regs.read_port(self.port, PortRegister::Signature),
                self.regs.sata_status(self.port).raw(),
                self.regs.read_port(self.port, PortRegister::SataError),
                self.regs.read_port(self.port, PortRegister::CommandIssue),
            );
        }
    }

    /// Logs a failed request, attempts recovery, and maps it to the block errno
    /// domain.
    fn finish_request(
        &mut self,
        phase: &str,
        lba: Option<u64>,
        result: Result<(), AhciError>,
    ) -> Result<(), SysError> {
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                self.log_failure(phase, lba, error);
                self.recover_after_error(error);
                Err(map_io_error(error))
            },
        }
    }

    /// Restarts a still-connected port or leaves it permanently offline.
    fn recover_after_error(&mut self, error: AhciError) {
        if matches!(error, AhciError::LinkLost)
            || !self.regs.sata_status(self.port).device_present()
        {
            self.readiness = PortReadiness::Offline;
            return;
        }
        self.readiness = PortReadiness::Recovering;
        if self.stop_engines().is_err() || self.regs.task_file(self.port).busy_or_drq() {
            self.readiness = PortReadiness::Offline;
            return;
        }
        self.clear_status();
        self.program_dma_bases();
        self.regs.update_port(
            self.port,
            PortRegister::Command,
            0,
            PortCommand::FIS_RECEIVE_ENABLE.bits(),
        );
        if !self.poll_until(AHCI_ENGINE_TIMEOUT_MS, || {
            self.regs.read_port(self.port, PortRegister::Command)
                & PortCommand::FIS_RECEIVE_RUNNING.bits()
                != 0
        }) {
            self.readiness = PortReadiness::Offline;
            return;
        }
        self.regs.update_port(
            self.port,
            PortRegister::Command,
            0,
            PortCommand::START.bits(),
        );
        self.readiness = PortReadiness::Ready;
    }

    /// Clears latched host, port, and SATA error status before a command.
    fn clear_status(&self) {
        self.regs
            .acknowledge_port_interrupts(self.port, PortInterrupt::from_bits_retain(u32::MAX));
        self.regs.acknowledge_port_host_interrupt(self.port);
        let sata_error = self.regs.read_port(self.port, PortRegister::SataError);
        if sata_error != 0 {
            self.regs.acknowledge_sata_errors(self.port, sata_error);
        }
    }

    /// Rejects external I/O unless initialization or recovery completed.
    fn require_ready(&self) -> Result<(), AhciError> {
        match self.readiness {
            PortReadiness::Ready => Ok(()),
            PortReadiness::Probing | PortReadiness::Recovering | PortReadiness::Offline => {
                Err(AhciError::PortOffline)
            },
        }
    }

    /// Busy-polls a controller predicate for a configured bounded interval.
    fn poll_until(&self, timeout_ms: u64, mut predicate: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        let timeout = Duration::from_millis(timeout_ms);
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

    /// Captures the controller register set needed to diagnose an I/O failure.
    fn log_failure(&self, phase: &str, lba: Option<u64>, error: AhciError) {
        kerrln!(
            "ahci: phase={} lba={:?} error={:?} ghc={:#x} is={:#x} pxcmd={:#x} pxis={:#x} pxtfd={:#x} pxsig={:#x} pxssts={:#x} pxserr={:#x} pxci={:#x}",
            phase,
            lba,
            error,
            self.regs.read_host(HostRegister::GlobalControl),
            self.regs.read_host(HostRegister::InterruptStatus),
            self.regs.read_port(self.port, PortRegister::Command),
            self.regs
                .read_port(self.port, PortRegister::InterruptStatus),
            self.regs.task_file(self.port).raw(),
            self.regs.read_port(self.port, PortRegister::Signature),
            self.regs.sata_status(self.port).raw(),
            self.regs.read_port(self.port, PortRegister::SataError),
            self.regs.read_port(self.port, PortRegister::CommandIssue),
        );
    }
}

/// Maps simultaneous port interrupt bits using a stable severity order.
fn classify_interrupt_error(interrupts: PortInterrupt) -> AhciError {
    if interrupts.contains(PortInterrupt::HOST_BUS_FATAL) {
        AhciError::HostBusFatal
    } else if interrupts.contains(PortInterrupt::HOST_BUS_DATA) {
        AhciError::HostBusData
    } else if interrupts.contains(PortInterrupt::INTERFACE_FATAL) {
        AhciError::InterfaceFatal
    } else if interrupts.contains(PortInterrupt::TASK_FILE_ERROR) {
        AhciError::TaskFile
    } else if interrupts.contains(PortInterrupt::OVERFLOW) {
        AhciError::Overflow
    } else if interrupts.contains(PortInterrupt::INTERFACE_NON_FATAL) {
        AhciError::InterfaceNonFatal
    } else {
        AhciError::UnexpectedFis
    }
}

/// Maps internal controller failures into the kernel I/O error domain.
fn map_io_error(error: AhciError) -> SysError {
    match error {
        AhciError::UnsupportedController => SysError::DriverIncompatible,
        AhciError::HbaResetTimeout
        | AhciError::EngineStopTimeout
        | AhciError::FisStopTimeout
        | AhciError::LinkTimeout
        | AhciError::DeviceBusyTimeout
        | AhciError::CommandTimeout => SysError::Timeout,
        AhciError::TaskFile
        | AhciError::HostBusFatal
        | AhciError::HostBusData
        | AhciError::InterfaceFatal
        | AhciError::InterfaceNonFatal
        | AhciError::Overflow
        | AhciError::UnexpectedFis
        | AhciError::ShortTransfer
        | AhciError::LinkLost
        | AhciError::PortOffline => SysError::IO,
    }
}

#[kunit]
/// Locks down error priority when hardware reports several bits together.
fn fatal_interrupts_have_stable_error_priority() {
    assert_eq!(
        classify_interrupt_error(PortInterrupt::HOST_BUS_FATAL | PortInterrupt::TASK_FILE_ERROR),
        AhciError::HostBusFatal
    );
    assert_eq!(
        classify_interrupt_error(PortInterrupt::TASK_FILE_ERROR),
        AhciError::TaskFile
    );
}
