//! SiFive RiscV PLIC (Platform-Level Interrupt Controller) driver.
//!
//! References:
//! - https://github.com/riscv/riscv-plic-spec
//! - https://www.kernel.org/doc/Documentation/devicetree/bindings/interrupt-controller/sifive%2Cplic-1.0.0.yaml
//! - https://starfivetech.com/uploads/sifive-interrupt-cookbook-v1p2.pdf

use crate::{
    device::discovery::fwnode::FwNode,
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
}

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
            let plic = Self { remap, ndev };

            {
                // following are a temporary initialization. proper initialization should be
                // resolve "interrupts-extended" property and set up contexts accordingly, which
                // is left for future work.

                let regs = plic.regs();

                for i in 0..CpuArch::ncpus() {
                    let ctx = SiFivePlic::s_ctx_for(i);

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
        let claimed =
            self.regs()
                .claim(SiFivePlic::s_ctx_for(CpuArch::cur_cpu_id().get())) as usize;
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

    fn valid_hwirq(&self, hwirq: usize) -> bool {
        hwirq != 0 && hwirq <= self.ndev
    }

    fn enable_words(&self) -> usize {
        (self.ndev + 32) / 32
    }

    fn s_ctx_for(cpuid: usize) -> usize {
        cpuid * 2 + 1
    }

    fn current_s_context(&self) -> usize {
        Self::s_ctx_for(CpuArch::cur_cpu_id().get())
    }
}
