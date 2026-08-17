//! Loongson 2K1000LA integrated interrupt controller.
//!
//! The controller owns 64 SoC interrupt sources. Each source has one routing
//! byte, global enable/trigger state, and a per-core routed pending bit. This
//! is a different hardware owner from the LA7A PCH interrupt controller.

use crate::{
    device::discovery::fwnode::FwNode,
    mm::remap::{IoRemap, ioremap},
    prelude::*,
};

const SOURCE_COUNT: usize = 64;
const SOURCES_PER_BANK: usize = u32::BITS as usize;
const CORE_COUNT: usize = 2;
const CORE_PENDING_STRIDE: usize = 0x100;
const PER_CPU_PENDING_BYTES: usize = 0x10;
const REQUIRED_CONTROLLER_BYTES: usize =
    InterruptBank::High as usize + BankRegister::Auto as usize + core::mem::size_of::<u32>();
const REQUIRED_PER_CPU_BYTES: usize =
    CORE_PENDING_STRIDE * (CORE_COUNT - 1) + PER_CPU_PENDING_BYTES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
enum InterruptBank {
    Low = 0x20,
    High = 0x60,
}

impl InterruptBank {
    const ALL: [Self; 2] = [Self::Low, Self::High];

    const fn for_irq(irq: usize) -> Self {
        if irq < SOURCES_PER_BANK {
            Self::Low
        } else {
            Self::High
        }
    }

    const fn shift(self) -> usize {
        match self {
            Self::Low => 0,
            Self::High => SOURCES_PER_BANK,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
enum BankRegister {
    Enable = 0x04,
    EnableSet = 0x08,
    EnableClear = 0x0c,
    Polarity = 0x10,
    Edge = 0x14,
    Bounce = 0x18,
    Auto = 0x1c,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
enum RouteRegister {
    Low = 0x00,
    High = 0x40,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
enum PendingRegister {
    Low = 0x00,
    High = 0x08,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
enum CpuInterruptPin {
    Int3 = 3,
}

/// One bit per ICU source, matching a pair of low/high 32-bit registers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
struct InterruptBits(u64);

impl InterruptBits {
    const NONE: Self = Self(0);
    const ALL: Self = Self(u64::MAX);

    fn single(irq: usize) -> Self {
        assert!(valid_irq(irq), "invalid 2K1000 interrupt source {}", irq);
        Self(1u64 << irq)
    }

    const fn from_banks(low: u32, high: u32) -> Self {
        Self(low as u64 | ((high as u64) << SOURCES_PER_BANK))
    }

    const fn bank(self, bank: InterruptBank) -> u32 {
        (self.0 >> bank.shift()) as u32
    }

    const fn intersection(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    const fn first(self) -> Option<usize> {
        if self.0 == 0 {
            None
        } else {
            Some(self.0.trailing_zeros() as usize)
        }
    }
}

/// One route-entry byte: low nibble selects cores, high nibble selects INT0-3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
struct InterruptRoute(u8);

impl InterruptRoute {
    fn fixed(target: PhysCpuId, pin: CpuInterruptPin) -> Self {
        assert!(
            target.get() < CORE_COUNT,
            "2K1000 interrupt route target {} is outside the hardware domain",
            target
        );
        Self((1u8 << (pin as usize + 4)) | (1u8 << target.get()))
    }

    const fn bits(self) -> u8 {
        self.0
    }
}

/// Register values programmed once before the ICU is published.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InitialControllerConfig {
    route: InterruptRoute,
    polarity: InterruptBits,
    edge: InterruptBits,
    bounce: InterruptBits,
    auto: InterruptBits,
}

impl InitialControllerConfig {
    fn fixed_to(target: PhysCpuId) -> Self {
        let mut polarity = InterruptBits::NONE;
        let mut edge = InterruptBits::NONE;
        for (irq, sense) in SOURCE_SENSE.iter().copied().enumerate() {
            let Some(sense) = sense else {
                continue;
            };
            if sense.is_low() {
                polarity = polarity.union(InterruptBits::single(irq));
            }
            if sense.is_edge() {
                edge = edge.union(InterruptBits::single(irq));
            }
        }
        Self {
            route: InterruptRoute::fixed(target, CpuInterruptPin::Int3),
            polarity,
            edge,
            bounce: InterruptBits::NONE,
            auto: InterruptBits::NONE,
        }
    }
}

#[derive(Debug)]
pub struct Loongson2K1000Intc {
    controller: IoRemap,
    per_cpu_pending: [IoRemap; CORE_COUNT],
}

struct Registers {
    controller: *mut u8,
    per_cpu_pending: [*mut u8; CORE_COUNT],
}

impl Registers {
    fn controller_ptr(&self, bank: InterruptBank, register: BankRegister) -> *mut u32 {
        unsafe {
            self.controller
                .add(bank as usize + register as usize)
                .cast()
        }
    }

    fn pending_ptr(&self, cpu: PhysCpuId, register: PendingRegister) -> *mut u32 {
        assert!(
            cpu.get() < CORE_COUNT,
            "2K1000 interrupt status requested for invalid {}",
            cpu
        );
        unsafe {
            self.per_cpu_pending[cpu.get()]
                .add(register as usize)
                .cast()
        }
    }

    fn read_controller(&self, bank: InterruptBank, register: BankRegister) -> u32 {
        unsafe { core::ptr::read_volatile(self.controller_ptr(bank, register)) }
    }

    fn write_controller(&self, bank: InterruptBank, register: BankRegister, value: u32) {
        unsafe {
            core::ptr::write_volatile(self.controller_ptr(bank, register), value);
        }
    }

    fn write_route(&self, irq: usize, route: InterruptRoute) {
        let offset = if irq < SOURCES_PER_BANK {
            RouteRegister::Low as usize + irq
        } else {
            RouteRegister::High as usize + irq - SOURCES_PER_BANK
        };
        unsafe {
            core::ptr::write_volatile(self.controller.add(offset), route.bits());
        }
    }

    fn write_irq_bit(&self, irq: usize, register: BankRegister) {
        let bank = InterruptBank::for_irq(irq);
        self.write_controller(bank, register, InterruptBits::single(irq).bank(bank));
    }

    fn read_bank_pair(&self, register: BankRegister) -> InterruptBits {
        InterruptBits::from_banks(
            self.read_controller(InterruptBank::Low, register),
            self.read_controller(InterruptBank::High, register),
        )
    }

    fn write_bank_pair(&self, register: BankRegister, value: InterruptBits) {
        for bank in InterruptBank::ALL {
            self.write_controller(bank, register, value.bank(bank));
        }
    }

    fn pending(&self, cpu: PhysCpuId) -> InterruptBits {
        let low = unsafe { core::ptr::read_volatile(self.pending_ptr(cpu, PendingRegister::Low)) };
        let high =
            unsafe { core::ptr::read_volatile(self.pending_ptr(cpu, PendingRegister::High)) };
        InterruptBits::from_banks(low, high)
    }
}

impl IrqChip for Loongson2K1000Intc {
    fn mask(&self, irq: HwIrq) {
        let irq = irq.get();
        if source_sense(irq).is_none() {
            kwarningln!("2k1000-icu: refusing to mask unsupported hwirq {}", irq);
            return;
        }
        self.regs().write_irq_bit(irq, BankRegister::EnableClear);
    }

    fn unmask(&self, irq: HwIrq) {
        let irq = irq.get();
        let Some(sense) = source_sense(irq) else {
            kwarningln!("2k1000-icu: refusing to unmask unsupported hwirq {}", irq);
            return;
        };
        let regs = self.regs();
        if sense == IrqSense::LevelLow {
            // Diagnostic-only Gate 3 evidence: sample the controller-owned
            // pending window immediately before each level-source unmask.
            // Keep this trace until Gate 3 hardware acceptance closes; any
            // removal or log-level reduction belongs to the later closure
            // review, not to this implementation boundary.
            // This must stay in the concrete irqchip; DWMAC does not gain a
            // controller API or a second pending-state owner.
            let pending = regs.pending(cur_cpu_id().physical_id());
            let enabled = regs.read_bank_pair(BankRegister::Enable);
            kdebugln!(
                "2k1000-icu: pending-before-unmask hwirq={} pending={:#x} enabled={:#x}",
                irq,
                pending.0,
                enabled.0,
            );
        }
        regs.write_irq_bit(irq, BankRegister::EnableSet);
    }

    fn ack(&self, irq: HwIrq) {
        let irq = irq.get();
        let Some(sense) = source_sense(irq) else {
            kwarningln!(
                "2k1000-icu: refusing to acknowledge unsupported hwirq {}",
                irq
            );
            return;
        };
        if sense.is_edge() {
            let regs = self.regs();
            // INTENCLR also retires the recorded pulse. Re-enable immediately
            // so a pulse arriving during the handler remains observable.
            regs.write_irq_bit(irq, BankRegister::EnableClear);
            regs.write_irq_bit(irq, BankRegister::EnableSet);
        }
    }

    fn eoi(&self, _irq: HwIrq) {
        // Level devices deassert at their owning device. LevelFlow masks before
        // the handler and unmasks after this no-op.
    }

    fn xlate(&self, spec: InterruptSpecifier<'_>) -> Option<InterruptInfo> {
        if spec.raw.len() != 4 {
            kwarningln!(
                "2k1000-icu: invalid interrupt specifier length: {}",
                spec.raw.len()
            );
            return None;
        }
        let irq = u32::from_be_bytes(spec.raw.try_into().ok()?) as usize;
        let Some(sense) = source_sense(irq) else {
            kwarningln!("2k1000-icu: unsupported hwirq {}", irq);
            return None;
        };
        Some(InterruptInfo {
            hwirq: HwIrq::new(irq),
            sense,
            flow: if sense.is_edge() {
                IrqFlowType::EdgeAck
            } else {
                IrqFlowType::LevelMaskEoi
            },
        })
    }

    fn as_core_irq_chip(&self) -> Option<&dyn CoreIrqChip> {
        Some(self)
    }
}

impl CoreIrqChip for Loongson2K1000Intc {
    fn init(fwnode: &dyn FwNode) -> Box<dyn CoreIrqChip> {
        let ofnode = fwnode
            .as_of_node()
            .expect("2k1000-icu requires an Open Firmware node");
        if ofnode.node().interrupt_cells() != Some(1) {
            panic!("2k1000-icu requires #interrupt-cells = <1>");
        }

        let regions = ofnode.node().reg().expect("2k1000-icu node is missing reg");
        let mut regions = regions.iter();
        let (controller_base, controller_len) = regions
            .next()
            .expect("2k1000-icu requires a controller register region");
        let (pending_base, pending_len) = regions
            .next()
            .expect("2k1000-icu requires a per-CPU pending register region");
        if regions.next().is_some() {
            panic!("2k1000-icu accepts exactly two register regions");
        }
        assert!(
            controller_len as usize >= REQUIRED_CONTROLLER_BYTES,
            "2k1000-icu controller region is too short: {:#x}",
            controller_len
        );
        assert!(
            pending_len as usize >= REQUIRED_PER_CPU_BYTES,
            "2k1000-icu per-CPU region is too short: {:#x}",
            pending_len
        );

        let controller =
            unsafe { ioremap(PhysAddr::new(controller_base), controller_len as usize) }
                .expect("failed to remap 2k1000-icu controller registers");
        // Only the two status windows are owned by the ICU driver. The gaps in
        // the DT span include each core's IPI/mailbox registers, which are
        // mapped separately by the machine IPI implementation.
        let per_cpu_pending = core::array::from_fn(|physical_id| {
            let base = pending_base + (physical_id * CORE_PENDING_STRIDE) as u64;
            unsafe { ioremap(PhysAddr::new(base), PER_CPU_PENDING_BYTES) }
                .expect("failed to remap 2k1000-icu per-CPU pending registers")
        });
        let intc = Self {
            controller,
            per_cpu_pending,
        };
        intc.initialize_hardware(cur_cpu_id().physical_id());

        kinfoln!(
            "2k1000-icu: controller={:#x}/{:#x}, pending={:#x}/{:#x}",
            controller_base,
            controller_len,
            pending_base,
            pending_len
        );
        Box::new(intc)
    }

    fn claim(&self) -> Option<HwIrq> {
        let regs = self.regs();
        let pending = regs.pending(cur_cpu_id().physical_id());
        let enabled = regs.read_bank_pair(BankRegister::Enable);
        pending.intersection(enabled).first().map(HwIrq::new)
    }
}

impl Loongson2K1000Intc {
    fn regs(&self) -> Registers {
        Registers {
            controller: self.controller.as_ptr().as_ptr().cast(),
            per_cpu_pending: core::array::from_fn(|physical_id| {
                self.per_cpu_pending[physical_id].as_ptr().as_ptr().cast()
            }),
        }
    }

    fn initialize_hardware(&self, target: PhysCpuId) {
        assert!(
            target.get() < CORE_COUNT,
            "2k1000-icu cannot route interrupts to {}",
            target
        );
        let regs = self.regs();
        let config = InitialControllerConfig::fixed_to(target);

        // Keep routing stable for the lifetime of the controller. The manual
        // forbids changing AUTO/BOUNCE after configuration, and Anemone does
        // not yet expose IRQ affinity, so all SoC IRQs use fixed BSP/INT3
        // routing. The low nibbles are one-hot CPU vectors and the high
        // nibbles are one-hot INT0..INT3 vectors.
        for irq in 0..SOURCE_COUNT {
            regs.write_route(irq, config.route);
        }

        regs.write_bank_pair(BankRegister::EnableClear, InterruptBits::ALL);
        regs.write_bank_pair(BankRegister::Polarity, config.polarity);
        regs.write_bank_pair(BankRegister::Edge, config.edge);
        regs.write_bank_pair(BankRegister::Bounce, config.bounce);
        regs.write_bank_pair(BankRegister::Auto, config.auto);
        let polarity = regs.read_bank_pair(BankRegister::Polarity);
        let edge = regs.read_bank_pair(BankRegister::Edge);
        assert_eq!(
            polarity, config.polarity,
            "2k1000-icu polarity readback mismatch"
        );
        assert_eq!(edge, config.edge, "2k1000-icu edge readback mismatch");
        kinfoln!(
            "2k1000-icu: IrqSense readback EDGE={:#x} POL={:#x}",
            edge.0,
            polarity.0
        );
    }
}

fn valid_irq(irq: usize) -> bool {
    irq < SOURCE_COUNT
}

// The board DT uses one-cell interrupt specifiers, so it cannot carry Linux
// trigger flags. This owner-local table is the sole source for electrical
// sense, EDGE/POL programming, and flow until firmware moves to two cells.
// Reserved sources, PCIe/MSI sources without an allocation owner, and GPIO
// sources whose trigger is configurable remain unavailable rather than being
// assigned the controller reset default as an accidental contract.
const SOURCE_SENSE: [Option<IrqSense>; SOURCE_COUNT] = {
    let mut senses = [Some(IrqSense::LevelHigh); SOURCE_COUNT];
    senses[6] = None;
    senses[11] = None;
    // GMAC0/1 macirq and wake lines are active-high. LIOINTC POL=0 selects
    // active-high; programming these inputs low leaves device RI/NIS asserted
    // without ever publishing a controller pending bit.
    senses[12] = Some(IrqSense::LevelHigh);
    senses[13] = Some(IrqSense::LevelHigh);
    senses[14] = Some(IrqSense::LevelHigh);
    senses[15] = Some(IrqSense::LevelHigh);
    let mut irq = 32;
    while irq <= 37 {
        senses[irq] = None;
        irq += 1;
    }
    irq = 44;
    while irq <= 48 {
        senses[irq] = Some(IrqSense::EdgeRising);
        irq += 1;
    }
    irq = 58;
    while irq < SOURCE_COUNT {
        senses[irq] = None;
        irq += 1;
    }
    senses
};

fn source_sense(irq: usize) -> Option<IrqSense> {
    SOURCE_SENSE.get(irq).copied().flatten()
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn source_table_owns_sense_and_controller_bits() {
        assert_eq!(source_sense(12), Some(IrqSense::LevelHigh));
        assert_eq!(source_sense(15), Some(IrqSense::LevelHigh));
        assert_eq!(source_sense(44), Some(IrqSense::EdgeRising));
        assert_eq!(source_sense(48), Some(IrqSense::EdgeRising));
        assert_eq!(source_sense(6), None);
        assert_eq!(source_sense(32), None);
        assert_eq!(source_sense(58), None);
        assert_eq!(source_sense(SOURCE_COUNT), None);

        let config = InitialControllerConfig::fixed_to(PhysCpuId::new(0));
        for irq in [12, 13, 14, 15] {
            assert_eq!(
                config.edge.intersection(InterruptBits::single(irq)),
                InterruptBits::NONE
            );
            assert_eq!(
                config.polarity.intersection(InterruptBits::single(irq)),
                InterruptBits::NONE
            );
        }
        for irq in 44..=48 {
            assert_eq!(
                config.edge.intersection(InterruptBits::single(irq)),
                InterruptBits::single(irq)
            );
            assert_eq!(
                config.polarity.intersection(InterruptBits::single(irq)),
                InterruptBits::NONE
            );
        }
    }
}
