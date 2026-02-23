use riscv::register::satp;

use crate::{mm::layout::KernelLayoutTrait, prelude::*};

pub struct Sv39PagingArch;

impl PagingArchTrait for Sv39PagingArch {
    type PgDir = super::RiscV64PgDir;

    const PAGE_SIZE_BYTES: usize = super::PAGE_SIZE_BYTES;

    const PAGE_LEVELS: usize = 3;

    const PTE_FLAGS_BITS: usize = super::PTE_FLAGS_BITS;

    unsafe fn activate_addr_space(pgtbl: &PageTable) {
        let satp_val = ((satp::Mode::Sv39 as usize) << 60) | (pgtbl.root_ppn().get() as usize);
        unsafe {
            core::arch::asm!(
                "csrw   satp, {satp_value}",
                "sfence.vma",
                satp_value = in(reg) satp_val,
            )
        }
    }
}

pub struct Sv39KernelLayout;

impl KernelLayoutTrait<Sv39PagingArch> for Sv39KernelLayout {
    const DIRECT_MAPPING_ADDR: u64 = 0xffff_ffc0_0000_0000;

    const KERNEL_VA_BASE: u64 = KERNEL_VA_BASE;

    const KERNEL_LA_BASE: u64 = KERNEL_LA_BASE;
}
