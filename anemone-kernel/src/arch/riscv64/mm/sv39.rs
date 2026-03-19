use riscv::register::satp;

use crate::{
    arch::riscv64::mm::{RiscV64PgDir, RiscV64Pte, RiscV64PteFlags},
    mm::layout::KernelLayoutTrait,
    prelude::*,
};

pub struct Sv39PagingArch;

impl PagingArchTrait for Sv39PagingArch {
    type PgDir = super::RiscV64PgDir;

    const MAX_HUGE_PAGE_LEVEL: usize = 2;

    const PAGE_LEVELS: usize = 3;

    const MAX_PPN_BITS: usize = 44;

    const PAGE_SIZE_BYTES: usize = super::PAGE_SIZE_BYTES;

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

    fn setup_direct_mapping_region(kptable: &mut PageTable) {
        let mut mapper = kptable.mapper();

        {
            sys_mem_zones().with_avail_zones(|avail_mem_zones| {
            for zone in avail_mem_zones.iter() {
                let range = zone.range();

                unsafe {
                    mapper
                        .map_overwrite(Mapping {
                            vpn: range.start().to_hhdm(),
                            ppn: range.start(),
                            flags: PteFlags::READ | PteFlags::WRITE | PteFlags::GLOBAL,
                            npages: range.npages() as usize,
                            huge_pages: true,
                        })
                        .expect("failed to map direct mapping region");
                }
                kdebugln!(
                    "mapped direct mapping region:\n\tvirtual page number {} ~ {},\n\tphysical page number {} ~ {},\n\tflags = {:?}",
                    range.start().to_hhdm(),
                    range.end().to_hhdm(),
                    range.start(),
                    range.end(),
                    PteFlags::READ | PteFlags::WRITE | PteFlags::GLOBAL,
                );
            }
        });
        }

        // reserved memory regions
        {
            sys_mem_zones().with_rsv_zones(|rsv_mem_zones| {
                for zone in rsv_mem_zones.iter() {
                    if zone.flags().is_mappable() {
                        let range = zone.range();
                        unsafe {
                            mapper
                                .map_overwrite(Mapping {
                                    vpn: range.start().to_hhdm(),
                                    ppn: range.start(),
                                    // TODO: for kvirt region, we may want to map with more fine-grained
                                    // permissions.
                                    flags: PteFlags::READ | PteFlags::WRITE | PteFlags::GLOBAL,
                                    npages: range.npages() as usize,
                                    huge_pages: true,
                                }).expect("failed to map reserved memory region");
                        }
                        kdebugln!(
                            "mapped reserved memory region to hhdm:\n\tvirtual page number {} ~ {},\n\tphysical page number {} ~ {},\n\tflags = {:?}",
                            range.start().to_hhdm(),
                            range.end().to_hhdm(),
                            range.start(),
                            range.end(),
                            zone.flags(),
                        );
                    }
                }
            });
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
    const USPACE_TOP_VPN: VirtPageNum = VirtPageNum::new(
        (1 << (Sv39PagingArch::PAGE_LEVELS * Sv39PagingArch::PGDIR_IDX_BITS) >> 1),
    );

    const FREE_SPACE_VPN: VirtPageNum = VirtPageNum::new(
        (Self::KSPACE_VPN.to_virt_addr().get() + PHYS_RAM_START + MAX_PHYS_RAM_SIZE)
            >> Sv39PagingArch::PAGE_SIZE_BITS,
    );

    const KERNEL_VA_BASE_VPN: VirtPageNum =
        VirtPageNum::new(KERNEL_VA_BASE >> Sv39PagingArch::PAGE_SIZE_BITS);

    const KERNEL_LA_BASE_VPN: PhysPageNum =
        PhysPageNum::new(KERNEL_LA_BASE >> Sv39PagingArch::PAGE_SIZE_BITS);

    const DIRECT_MAPPING_VPN: VirtPageNum = Self::KSPACE_VPN;
}
