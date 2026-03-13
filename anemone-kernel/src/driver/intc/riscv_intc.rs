//! /cpus/cpu@[x]/interrupt-controller drivers.
//!
//! initialized during early system boot process.

use crate::prelude::*;

#[derive(Debug)]
pub struct RiscvIntc;

impl IrqChip for RiscvIntc {
    fn startup(&self) {
        unsafe {
            riscv::register::sstatus::set_sie();
        }
    }

    fn shutdown(&self) {
        unsafe {
            riscv::register::sstatus::clear_sie();
        }
    }

    fn mask(&self, irq: HwIrq) {
        // possble values of `irq` here can only be 1, 5 and 9, which
        // correspond to software, timer and external interrupts under
        // supervisor mode, respectively.

        unsafe {
            match irq.get() {
                1 => riscv::register::sie::clear_ssoft(),
                5 => riscv::register::sie::clear_stimer(),
                9 => riscv::register::sie::clear_sext(),
                _ => panic!("invalid irq {}", irq.get()),
            }
        }
    }

    fn unmask(&self, irq: HwIrq) {
        unsafe {
            match irq.get() {
                1 => riscv::register::sie::set_ssoft(),
                5 => riscv::register::sie::set_stimer(),
                9 => riscv::register::sie::set_sext(),
                _ => panic!("invalid irq {}", irq.get()),
            }
        }
    }

    fn ack(&self, irq: HwIrq) {
        // in s-mode, the only permission kernel has is to clear the pending
        // state of software interrupts, while timer and external interrupts
        // must be delegated to m-mode software.

        unsafe {
            match irq.get() {
                1 => riscv::register::sip::clear_ssoft(),
                _ => panic!("invalid irq {}", irq.get()),
            }
        }
    }

    fn eoi(&self, _irq: HwIrq) {
        // nothing to do.
    }

    fn xlate(&self, raw: &[u8]) -> Option<InterruptInfo> {
        unreachable!()
    }
}
