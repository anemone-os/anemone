use riscv::register::satp;

use crate::{
    arch::riscv64::mm::{RiscV64PgDir, RiscV64Pte, RiscV64PteFlags},
    mm::layout::KernelLayoutTrait,
    prelude::*,
};

pub struct Sv39PagingArch;

impl PagingArchTrait for Sv39PagingArch {
    type PgDir = super::RiscV64PgDir;

    /// On RiscV64, maximum length of physical addresses is 56 bits.
    const MAX_PPN_BITS: usize = 44;

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

    fn prealloc_pgdirs_for_region(pgtbl: &mut PageTable, range: VirtPageRange) {
        let svpn = range.start();
        let npages = range.npages() as usize;
        kdebugln!(
            "range: [{:#x}, {:#x})",
            range.start().get(),
            range.end().get()
        );
        debug_assert!(
            svpn.get()
                .is_multiple_of(Sv39PagingArch::NPAGES_PER_GB as u64)
        );
        debug_assert!(npages.is_multiple_of(Sv39PagingArch::NPAGES_PER_GB));

        // in sv39, a lv.2 pgdir can map 1GB of virtual memory, so we only need
        // to preallocate lv.2 pgdirs for the given region

        let ngigabytes = npages / Sv39PagingArch::NPAGES_PER_GB;

        let root_kpgdir = unsafe {
            pgtbl
                .root_ppn()
                .to_phys_addr()
                .to_hhdm()
                .as_ptr_mut::<RiscV64PgDir>()
                .as_mut()
                .expect("pgdir ppn should not be null")
        };

        // Sv39 root pgdir index is VPN[2], i.e. the highest 9 bits in VPN.
        let root_idx_base = ((svpn.get()
            >> ((Sv39PagingArch::PAGE_LEVELS - 1) * Sv39PagingArch::PGDIR_IDX_BITS))
            & (Sv39PagingArch::PTE_PER_PGDIR as u64 - 1)) as usize;

        for i in 0..ngigabytes {
            let pgdir_idx = root_idx_base + i;

            let pgdir_ppn = unsafe {
                alloc_frame_zeroed()
                    .expect("failed to allocate frame for preallocating page directory")
                    .leak()
            };

            unsafe {
                // a single V bit is enough
                debug_assert!(root_kpgdir[pgdir_idx].is_empty());
                root_kpgdir[pgdir_idx] = RiscV64Pte::arch_new(pgdir_ppn, RiscV64PteFlags::VALID);
            }
        }
    }

    fn tlb_shootdown(vaddr: VirtAddr) {
        riscv::asm::sfence_vma(0, vaddr.get() as usize);
    }

    fn tlb_shootdown_all() {
        riscv::asm::sfence_vma_all();
    }
}

pub struct Sv39KernelLayout;

impl KernelLayoutTrait<Sv39PagingArch> for Sv39KernelLayout {
    const DIRECT_MAPPING_ADDR: u64 = 0xffff_ffc0_0000_0000;

    const KERNEL_VA_BASE: u64 = KERNEL_VA_BASE;

    const KERNEL_LA_BASE: u64 = KERNEL_LA_BASE;
}
