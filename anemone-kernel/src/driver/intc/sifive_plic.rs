//! SiFive RiscV PLIC (Platform-Level Interrupt Controller) driver.
//!
//! References:
//! - https://github.com/riscv/riscv-plic-spec
//! - https://www.kernel.org/doc/Documentation/devicetree/bindings/interrupt-controller/sifive%2Cplic-1.0.0.yaml
//! - https://starfivetech.com/uploads/sifive-interrupt-cookbook-v1p2.pdf

use crate::{
    device::discovery::{
        fwnode::FwNode,
        open_firmware::of_with_node_by_phandle,
    },
    mm::remap::{IoRemap, ioremap},
    prelude::*,
};

mod driver_core {

    const PRIORITY_BASE: usize = 0x0000;
    const ENABLE_BASE: usize = 0x2000;
    const ENABLE_STRIDE: usize = 0x80;
    const CONTEXT_BASE: usize = 0x20_0000;
    const CONTEXT_STRIDE: usize = 0x1000;

    const CONTEXT_THRESHOLD: usize = 0x0;
    const CONTEXT_CLAIM_COMPLETE: usize = 0x4;

    #[repr(C)]
    pub struct SiFivePlicRegisters {
        base: *mut u8,
    }

    impl SiFivePlicRegisters {
        pub unsafe fn from_raw(base: *mut u8) -> Self {
            Self { base }
        }

        fn reg_ptr(&self, offset: usize) -> *mut u32 {
            unsafe { self.base.add(offset).cast() }
        }

        fn read_u32(&self, offset: usize) -> u32 {
            unsafe { core::ptr::read_volatile(self.reg_ptr(offset)) }
        }

        fn write_u32(&self, offset: usize, val: u32) {
            unsafe { core::ptr::write_volatile(self.reg_ptr(offset), val) }
        }

        pub fn set_priority(&self, irq: usize, prio: u32) {
            self.write_u32(PRIORITY_BASE + irq * 4, prio);
        }

        pub fn set_enable(&self, context: usize, irq: usize, enable: bool) {
            let word = irq / 32;
            let bit = irq % 32;
            let off = ENABLE_BASE + context * ENABLE_STRIDE + word * 4;
            let mut val = self.read_u32(off);
            if enable {
                val |= 1 << bit;
            } else {
                val &= !(1 << bit);
            }
            self.write_u32(off, val);
        }

        pub fn clear_enable_words(&self, context: usize, nwords: usize) {
            let base = ENABLE_BASE + context * ENABLE_STRIDE;
            for i in 0..nwords {
                self.write_u32(base + i * 4, 0);
            }
        }

        pub fn set_threshold(&self, context: usize, threshold: u32) {
            let off = CONTEXT_BASE + context * CONTEXT_STRIDE + CONTEXT_THRESHOLD;
            self.write_u32(off, threshold);
        }

        pub fn claim(&self, context: usize) -> u32 {
            let off = CONTEXT_BASE + context * CONTEXT_STRIDE + CONTEXT_CLAIM_COMPLETE;
            self.read_u32(off)
        }

        pub fn complete(&self, context: usize, irq: usize) {
            let off = CONTEXT_BASE + context * CONTEXT_STRIDE + CONTEXT_CLAIM_COMPLETE;
            self.write_u32(off, irq as u32);
        }
    }
}
use driver_core::*;

#[derive(Debug)]
pub struct SiFivePlic {
    remap: IoRemap,
    ndev: usize,
    /// S-mode contexts parsed from `interrupts-extended`, indexed by logical
    /// CPU ID. The immutable device tree is the source of truth; this snapshot
    /// avoids phandle traversal in the interrupt path and must never go stale.
    s_contexts: Vec<usize>,
}

pub static COMPATIBLE_STRS: &[&str] = &["sifive,plic-1.0.0", "riscv,plic0", "starfive,jh7110-plic"];

impl IrqChip for SiFivePlic {
    fn mask(&self, irq: HwIrq) {
        let hwirq = irq.get();
        if !self.valid_hwirq(hwirq) {
            kwarningln!("sifive-plic: mask invalid hwirq {}", hwirq);
            return;
        }

        self.regs()
            .set_enable(self.current_s_context(), hwirq, false);
    }

    fn unmask(&self, irq: HwIrq) {
        let hwirq = irq.get();
        if !self.valid_hwirq(hwirq) {
            kwarningln!("sifive-plic: unmask invalid hwirq {}", hwirq);
            return;
        }

        let regs = self.regs();
        regs.set_priority(hwirq, 1);
        regs.set_enable(self.current_s_context(), hwirq, true);
    }

    fn ack(&self, _irq: HwIrq) {
        // claim already performs the acknowledge step on PLIC.
    }

    fn eoi(&self, irq: HwIrq) {
        let hwirq = irq.get();
        if !self.valid_hwirq(hwirq) {
            kwarningln!("sifive-plic: eoi invalid hwirq {}", hwirq);
            return;
        }
        self.regs().complete(self.current_s_context(), hwirq);
    }

    fn xlate(&self, spec: InterruptSpecifier<'_>) -> Option<InterruptInfo> {
        // #interrupt-cells = 1, which is the hardware IRQ number.
        if spec.raw.len() != 4 {
            kwarningln!(
                "sifive-plic: invalid interrupt specifier length: {}",
                spec.raw.len()
            );
            return None;
        }
        let hwirq = HwIrq::new(u32::from_be_bytes(spec.raw.try_into().ok()?) as usize);
        if !self.valid_hwirq(hwirq.get()) {
            kwarningln!("sifive-plic: invalid hwirq {}", hwirq.get());
            return None;
        }

        Some(InterruptInfo {
            hwirq,
            // refer to PLIC's gateway mechanism, which ensures that kernel always perceives an
            // effect equivalent to level-triggered interrupts.
            //
            // ...?🤔
            trigger: IrqTriggerType::Level,
        })
    }

    fn as_core_irq_chip(&self) -> Option<&dyn CoreIrqChip> {
        Some(self)
    }
}

impl CoreIrqChip for SiFivePlic {
    fn init(fwnode: &dyn FwNode) -> Box<dyn CoreIrqChip>
    where
        Self: Sized,
    {
        if let Some(ofnode) = fwnode.as_of_node() {
            let reg = ofnode
                .node()
                .reg()
                .expect("failed to read reg property from plic node");
            let (base, len) = {
                let mut it = reg.iter();
                let first = it
                    .next()
                    .expect("plic node must have exactly one reg region");
                if it.next().is_some() {
                    panic!("plic node must have exactly one reg region");
                }
                (PhysAddr::new(first.0), first.1)
            };
            let ndev = ofnode
                .prop_read_u32("riscv,ndev")
                .expect("failed to read riscv,ndev property from plic node")
                as usize;

            kdebugln!(
                "plic: base = {:#x}, len = {:#x}, ndev = {}",
                base.get(),
                len,
                ndev
            );

            let remap =
                unsafe { ioremap(base, len as usize) }.expect("failed to remap plic registers");
            let s_contexts = Self::parse_s_contexts(fwnode)
                .unwrap_or_else(|error| panic!("failed to parse PLIC contexts: {error:?}"));
            let plic = Self {
                remap,
                ndev,
                s_contexts,
            };

            {
                let regs = plic.regs();

                for &ctx in &plic.s_contexts {
                    // keep all sources masked by default, individual lines are unmasked by
                    // request path later.
                    regs.clear_enable_words(ctx, plic.enable_words());
                    // allow all priorities.
                    regs.set_threshold(ctx, 0);
                }

                // initialize all source priorities to 1 so enabled sources can pass
                // threshold filtering.
                for irq in 1..=plic.ndev {
                    regs.set_priority(irq, 1);
                }
            }

            Box::new(plic)
        } else {
            unimplemented!("only open firmware node is supported for now");
        }
    }

    fn claim(&self) -> Option<HwIrq> {
        let claimed = self.regs().claim(self.current_s_context()) as usize;
        if claimed == 0 {
            return None;
        }

        if !self.valid_hwirq(claimed) {
            kwarningln!("sifive-plic: claim invalid hwirq {}", claimed);
            return None;
        }

        Some(HwIrq::new(claimed))
    }
}

impl SiFivePlic {
    fn regs(&self) -> SiFivePlicRegisters {
        unsafe { SiFivePlicRegisters::from_raw(self.remap.as_ptr().as_ptr().cast()) }
    }

    fn parse_s_contexts(fwnode: &dyn FwNode) -> Result<Vec<usize>, SysError> {
        // `riscv,cpu-intc` uses the RISC-V interrupt cause encoding; source 9
        // is the supervisor external interrupt chained to the PLIC.
        const SUPERVISOR_EXTERNAL: u32 = 9;

        macro_rules! parse_fail {
            ($error:expr, $($args:tt)*) => {{
                kerrln!($($args)*);
                return Err($error);
            }};
        }

        let raw = match fwnode.prop_read_raw("interrupts-extended") {
            Some(raw) => raw,
            None => parse_fail!(
                SysError::FwNodeLookupFailed,
                "plic: missing interrupts-extended property"
            ),
        };
        if raw.is_empty() {
            parse_fail!(
                SysError::InvalidInterruptInfo,
                "plic: interrupts-extended property is empty"
            );
        }

        let mut offset = 0;
        let mut context = 0;
        let mut parsed_s_contexts = Vec::new();

        while offset < raw.len() {
            let phandle_end = match offset.checked_add(4) {
                Some(end) => end,
                None => parse_fail!(
                    SysError::InvalidInterruptInfo,
                    "plic: interrupts-extended phandle offset overflow at {}",
                    offset
                ),
            };
            let phandle_raw = match raw.get(offset..phandle_end) {
                Some(raw) => raw,
                None => parse_fail!(
                    SysError::InvalidInterruptInfo,
                    "plic: truncated parent phandle at interrupts-extended offset {}",
                    offset
                ),
            };
            let phandle = u32::from_be_bytes(phandle_raw.try_into().unwrap());
            offset = phandle_end;

            let parent = of_with_node_by_phandle(phandle, |node| {
                let is_cpu_intc = node.compatible().map_or(false, |mut compatibles| {
                    compatibles.any(|compatible| compatible == "riscv,cpu-intc")
                });
                if !is_cpu_intc {
                    parse_fail!(
                        SysError::InvalidInterruptInfo,
                        "plic: context phandle {:#x} does not refer to riscv,cpu-intc",
                        phandle
                    );
                }

                let interrupt_cells = match node.interrupt_cells() {
                    Some(cells) => cells as usize,
                    None => parse_fail!(
                        SysError::FwNodeLookupFailed,
                        "plic: context phandle {:#x} has no #interrupt-cells",
                        phandle
                    ),
                };
                let cpu = match node.parent() {
                    Some(cpu) => cpu,
                    None => parse_fail!(
                        SysError::FwNodeLookupFailed,
                        "plic: context phandle {:#x} has no parent CPU node",
                        phandle
                    ),
                };
                let reg = match cpu.reg() {
                    Some(reg) => reg,
                    None => parse_fail!(
                        SysError::FwNodeLookupFailed,
                        "plic: CPU parent of context phandle {:#x} has no reg",
                        phandle
                    ),
                };
                let mut reg = reg.iter();
                let (hart, _) = match reg.next() {
                    Some(hart) => hart,
                    None => parse_fail!(
                        SysError::FwNodeLookupFailed,
                        "plic: CPU parent of context phandle {:#x} has an empty reg",
                        phandle
                    ),
                };
                if reg.next().is_some() {
                    parse_fail!(
                        SysError::InvalidInterruptInfo,
                        "plic: CPU parent of context phandle {:#x} has multiple reg entries",
                        phandle
                    );
                }
                let physical_id = match usize::try_from(hart) {
                    Ok(hart) => PhysCpuId::new(hart),
                    Err(_) => parse_fail!(
                        SysError::InvalidInterruptInfo,
                        "plic: hart id {} from context phandle {:#x} does not fit usize",
                        hart,
                        phandle
                    ),
                };
                Ok((physical_id, interrupt_cells))
            })
            .ok()
            .unwrap_or_else(|| {
                parse_fail!(
                    SysError::FwNodeLookupFailed,
                    "plic: context phandle {:#x} was not found",
                    phandle
                )
            });
            let (physical_id, interrupt_cells) = match parent {
                Ok(parent) => parent,
                Err(error) => parse_fail!(
                    error,
                    "plic: failed to read context phandle {:#x}: {:?}",
                    phandle,
                    error
                ),
            };

            let specifier_len = match interrupt_cells.checked_mul(4) {
                Some(len) => len,
                None => parse_fail!(
                    SysError::InvalidInterruptInfo,
                    "plic: context {} specifier length overflows for #interrupt-cells={}",
                    context,
                    interrupt_cells
                ),
            };
            let specifier_end = match offset.checked_add(specifier_len) {
                Some(end) => end,
                None => parse_fail!(
                    SysError::InvalidInterruptInfo,
                    "plic: context {} specifier offset overflows at {}",
                    context,
                    offset
                ),
            };
            let specifier = match raw.get(offset..specifier_end) {
                Some(specifier) => specifier,
                None => parse_fail!(
                    SysError::InvalidInterruptInfo,
                    "plic: truncated specifier for context {} at offset {} ({} cells)",
                    context,
                    offset,
                    interrupt_cells
                ),
            };
            offset = specifier_end;

            // The PLIC binding requires each context parent to be a
            // riscv,cpu-intc, whose binding defines a one-cell interrupt cause.
            // The entry boundary above still comes from the parent's
            // #interrupt-cells as required by the generic DT specification.
            if interrupt_cells != 1 {
                parse_fail!(
                    SysError::InvalidInterruptInfo,
                    "plic: context {} uses unsupported riscv,cpu-intc #interrupt-cells={}",
                    context,
                    interrupt_cells
                );
            }
            let interrupt = u32::from_be_bytes(specifier.try_into().unwrap());

            if interrupt == SUPERVISOR_EXTERNAL {
                if parsed_s_contexts
                    .iter()
                    .any(|&(existing, _)| existing == physical_id)
                {
                    parse_fail!(
                        SysError::InvalidInterruptInfo,
                        "plic: duplicate S-mode context for physical CPU {} at context {}",
                        physical_id,
                        context
                    );
                }
                parsed_s_contexts.push((physical_id, context));
            }

            context += 1;
        }

        let mut s_contexts = Vec::with_capacity(ncpus());
        for logical_id in 0..ncpus() {
            let physical_id = CpuId::new(logical_id).physical_id();
            let context = match parsed_s_contexts
                .iter()
                .find_map(|&(candidate, context)| (candidate == physical_id).then_some(context))
            {
                Some(context) => context,
                None => parse_fail!(
                    SysError::InvalidInterruptInfo,
                    "plic: no S-mode context found for active physical CPU {}",
                    physical_id
                ),
            };
            kinfoln!("plic: {} uses S-mode context {}", physical_id, context);
            s_contexts.push(context);
        }
        Ok(s_contexts)
    }

    fn valid_hwirq(&self, hwirq: usize) -> bool {
        hwirq != 0 && hwirq <= self.ndev
    }

    fn enable_words(&self) -> usize {
        (self.ndev + 32) / 32
    }

    fn current_s_context(&self) -> usize {
        self.s_contexts[cur_cpu_id().logical_id()]
    }
}
