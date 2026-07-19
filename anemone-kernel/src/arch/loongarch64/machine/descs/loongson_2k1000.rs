//! Loongson 2K1000LA machine operations.

use core::arch::asm;

use crate::{
    arch::loongarch64::{
        machine::{MachineDesc, MachineIpi},
        mm::LA64KernelLayout,
    },
    device::discovery::open_firmware::{
        get_of_node, of_with_node_by_compatible,
    },
    driver::intc::loongson_2k1000::Loongson2K1000Intc,
    mm::remap::{IoRemap, ioremap},
    prelude::*,
    utils::identity::GeneralIdentity,
};

use machine_consts::*;

mod machine_consts {
    pub(super) const CORE_COUNT: usize = 2;
    pub(super) const CORE0_IPI_BASE: u64 = 0x1fe0_1000;
    pub(super) const CORE_LOCAL_STRIDE: usize = 0x100;
    pub(super) const IPI_REGISTER_BYTES: usize = 0x40;

    pub(super) const fn ipi_base(physical_id: usize) -> u64 {
        CORE0_IPI_BASE + (physical_id * CORE_LOCAL_STRIDE) as u64
    }
}

/// Loongson 2K1000LA SoC machine description.
#[derive(Debug)]
pub struct Loongson2K1000;

impl MachineDesc for Loongson2K1000 {
    fn compatible(&self) -> &[&str] {
        &["loongson,2k1000"]
    }

    fn ipi(&self) -> &dyn MachineIpi {
        self
    }

    unsafe fn early_init_intc(&self) {
        kinfoln!("initializing Loongson 2K1000 interrupt controller");

        let intc = of_with_node_by_compatible("loongson,2k1000-icu", |node| node.handle())
            .map(get_of_node)
            .unwrap_or_else(|_| panic!("failed to find loongson,2k1000-icu in device tree"));
        intc.mark_populated();

        let intc_ops = Loongson2K1000Intc::init(intc.as_ref());
        unsafe {
            register_root_irq_domain(
                GeneralIdentity::try_from(intc.node().full_name()).unwrap(),
                intc_ops,
                intc,
            );
        }
    }

    unsafe fn early_init_timer(&self) {
        // The stable counter and timer CSRs are architected per-CPU resources.
    }
}

impl MachineIpi for Loongson2K1000 {
    unsafe fn init_runtime(&self) {
        let cores = core::array::from_fn(|physical_id| {
            unsafe { ioremap(PhysAddr::new(ipi_base(physical_id)), IPI_REGISTER_BYTES) }
                .expect("failed to remap 2K1000 IPI registers")
        });

        let mut mappings = IPI_MAPPINGS.lock_irqsave();
        assert!(mappings.is_none(), "2K1000 IPI registers initialized twice");
        *mappings = Some(IpiMappings { cores });
    }

    fn send_ipi(&self, target: PhysCpuId) {
        // Publish all normal-memory writes, including the target IPI queue,
        // before the device observes the interrupt-set write.
        device_barrier();
        let mappings = IPI_MAPPINGS.lock_irqsave();
        match mappings.as_ref() {
            Some(mappings) => mappings.regs(target),
            // `ioremap()` may request a TLB shootdown while the runtime IPI
            // mapping itself is being installed. DMW2 is still valid then.
            None => early_ipi_regs(target),
        }
        .write_vectors(IpiVectorRegister::Set, IpiVectorSet::KERNEL);
    }

    unsafe fn claim_ipi(&self) {
        let mappings = IPI_MAPPINGS.lock_irqsave();
        mappings
            .as_ref()
            .expect("2K1000 runtime IPI registers are not initialized")
            .regs(cur_cpu_id().physical_id())
            .write_vectors(IpiVectorRegister::Clear, IpiVectorSet::ALL);
    }

    unsafe fn init_local_ipi(&self) {
        let mappings = IPI_MAPPINGS.lock_irqsave();
        let regs = mappings
            .as_ref()
            .expect("2K1000 runtime IPI registers are not initialized")
            .regs(cur_cpu_id().physical_id());
        // Firmware may leave boot vectors pending. Retire them before exposing
        // the kernel-owned vector so stale bootstrap state cannot retrigger.
        regs.write_vectors(IpiVectorRegister::Clear, IpiVectorSet::ALL);
        regs.write_vectors(IpiVectorRegister::Enable, IpiVectorSet::KERNEL);
    }

    fn wake_secondary(&self, target: PhysCpuId, entry: PhysAddr) {
        let regs = early_ipi_regs(target);

        // The 2K1000 reset value of IPIEN is zero. Program the waiting target
        // explicitly, publish its 64-bit boot entry through BUF0 using the
        // manual-mandated uncached alias, then assert the same vector used for
        // runtime kernel IPIs.
        regs.write_vectors(IpiVectorRegister::Clear, IpiVectorSet::ALL);
        regs.write_vectors(IpiVectorRegister::Enable, IpiVectorSet::KERNEL);
        regs.write_secondary_entry(SecondaryEntry::new(entry));
        device_barrier();
        regs.write_vectors(IpiVectorRegister::Set, IpiVectorSet::KERNEL);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
enum IpiVectorRegister {
    Enable = 0x04,
    Set = 0x08,
    Clear = 0x0c,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
enum IpiMailboxRegister {
    BootEntry = 0x20,
}

/// Bit set carried by the IPI enable, set, and clear registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
struct IpiVectorSet(u32);

impl IpiVectorSet {
    const KERNEL: Self = Self(1);
    const ALL: Self = Self(u32::MAX);

    const fn bits(self) -> u32 {
        self.0
    }
}

/// Physical secondary entry encoded in mailbox 0 during AP startup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
struct SecondaryEntry(u64);

impl SecondaryEntry {
    const fn new(entry: PhysAddr) -> Self {
        Self(entry.get())
    }

    const fn bits(self) -> u64 {
        self.0
    }
}

/// Persistent per-core IPI mappings installed before the boot-only DMW2
/// window is removed. `None` is the bootstrap phase; the value is never taken
/// once published, so register pointers remain valid for the kernel lifetime.
static IPI_MAPPINGS: SpinLock<Option<IpiMappings>> = SpinLock::new(None);

#[derive(Debug)]
struct IpiMappings {
    cores: [IoRemap; CORE_COUNT],
}

impl IpiMappings {
    fn regs(&self, cpu: PhysCpuId) -> IpiRegisters {
        let physical_id = checked_physical_id(cpu);
        IpiRegisters {
            base: self.cores[physical_id].as_ptr().as_ptr().cast(),
        }
    }
}

#[derive(Clone, Copy)]
struct IpiRegisters {
    base: *mut u8,
}

impl IpiRegisters {
    fn write_vectors(self, register: IpiVectorRegister, vectors: IpiVectorSet) {
        unsafe {
            core::ptr::write_volatile(
                self.base.add(register as usize).cast(),
                vectors.bits(),
            );
        }
    }

    fn write_secondary_entry(self, entry: SecondaryEntry) {
        unsafe {
            core::ptr::write_volatile(
                self.base.add(IpiMailboxRegister::BootEntry as usize).cast(),
                entry.bits(),
            );
        }
    }
}

fn checked_physical_id(cpu: PhysCpuId) -> usize {
    let physical_id = cpu.get();
    assert!(
        physical_id < CORE_COUNT,
        "2K1000 IPI target {} is outside the two-core hardware domain",
        cpu
    );
    physical_id
}

fn early_ipi_regs(cpu: PhysCpuId) -> IpiRegisters {
    let physical_id = checked_physical_id(cpu);
    IpiRegisters {
        base: (LA64KernelLayout::TEMPORARY_IO_ADDR + ipi_base(physical_id)) as *mut u8,
    }
}

#[inline(always)]
fn device_barrier() {
    unsafe {
        asm!("dbar 0", options(nostack));
    }
}
