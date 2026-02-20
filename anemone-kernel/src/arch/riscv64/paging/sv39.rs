use riscv::register::satp;

use crate::prelude::*;

#[derive(Debug)]
pub struct Sv39Paging;

impl PagingArch for Sv39Paging {
    type PgDir = super::RiscV64PgDir;

    const PAGE_SIZE_BYTES: usize = super::PAGE_SIZE_BYTES;

    const PAGE_LEVELS: usize = 3;

    const PTE_FLAGS_BITS: usize = super::PTE_FLAGS_BITS;

    const DIRECT_MAPPING_ADDR: u64 = 0xffff_ffc0_0000_0000;

    unsafe fn activate_addr_space(pgtbl: &PageTable<Self>) {
        let root_ppn = pgtbl.root_ppn();
        let satp_val = ((satp::Mode::Sv39 as usize) << 60) | (root_ppn.get() as usize);
        unsafe {
            core::arch::asm!(
                "csrw   satp, {satp_value}",
                "sfence.vma",
                satp_value = in(reg) satp_val,
            )
        }
    }
}
