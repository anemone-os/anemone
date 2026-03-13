//! RiscV Platform-Level Interrupt Controller (PLIC) driver.
//!
//! Reference:
//! - https://github.com/riscv/riscv-plic-spec

use crate::{
    device::discovery::fwnode::FwNode,
    mm::remap::{IoRemap, ioremap},
    prelude::*,
    utils::prv_data::PrvData,
};

#[derive(Debug, PrvData)]
struct PlicState {
    base: PhysAddr,
    remap: IoRemap,
}

/// RiscV PLIC driver.
#[derive(Debug)]
pub struct Plic;

impl IrqChip for Plic {
    fn startup(&self) {
        todo!()
    }

    fn shutdown(&self) {
        todo!()
    }

    fn mask(&self, irq: HwIrq) {
        todo!()
    }

    fn unmask(&self, irq: HwIrq) {
        todo!()
    }

    fn ack(&self, irq: HwIrq) {
        todo!()
    }

    fn eoi(&self, irq: HwIrq) {
        todo!()
    }

    fn xlate(&self, raw: &[u8]) -> Option<InterruptInfo> {
        todo!()
    }
}

impl CoreIrqChip for Plic {
    fn init(&self, fwnode: Arc<dyn FwNode>) -> Box<dyn PrvData> {
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

            kdebugln!("plic: base = {:#x}, len = {:#x}", base.get(), len);

            let base_ppn = base.page_down();
            let npages = align_up_power_of_2!(len, PagingArch::PAGE_SIZE_BYTES)
                / PagingArch::PAGE_SIZE_BYTES;
            let remap = unsafe { ioremap(PhysPageRange::new(base_ppn, npages as u64)) }
                .expect("failed to remap plic registers");

            let state = PlicState { base, remap };
            Box::new(state)
        } else {
            unimplemented!("only open firmware node is supported for now");
        }
    }
}
