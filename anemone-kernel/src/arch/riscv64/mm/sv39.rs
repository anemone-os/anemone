use riscv::register::satp;

use crate::{mm::layout::KernelLayoutTrait, prelude::*};

declare_perf_metrics! {
    counter RISCV64_TLB_RANGE_PAGES {
        name: "arch.riscv64.tlb.range_pages",
        unit: Events,
    }
    counter RISCV64_TLB_PAGE_FLUSHES {
        name: "arch.riscv64.tlb.page_flushes",
        unit: Events,
    }
    counter RISCV64_TLB_FULL_FLUSHES {
        name: "arch.riscv64.tlb.full_flushes",
        unit: Events,
    }
}

pub struct Sv39PagingArch;

const fn use_full_tlb_flush(npages: u64) -> bool {
    npages > RISCV64_TLB_FLUSH_ALL_THRESHOLD_PAGES as u64
}

impl PagingArchTrait for Sv39PagingArch {
    type PgDir = super::RiscV64PgDir;

    const MAX_HUGE_PAGE_LEVEL: usize = 2;

    const PAGE_LEVELS: usize = 3;

    const MAX_PPN_BITS: usize = 44;

    const PAGE_SIZE_BYTES: usize = super::PAGE_SIZE_BYTES;

    fn setup_direct_mapping_region(pgtbl: &mut PageTable) {
        let mut mapper = pgtbl.mapper();

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
                    "mapped direct mapping region:\n\tvirtual page number {} ~ {},\n\tphysical page number {} ~ {}",
                    range.start().to_hhdm(),
                    range.end().to_hhdm(),
                    range.start(),
                    range.end(),
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
            }});
        }
    }

    unsafe fn activate_addr_space(root_ppn: PhysPageNum) {
        let satp_val = ((satp::Mode::Sv39 as usize) << 60) | (root_ppn.get() as usize);
        unsafe {
            core::arch::asm!(
                "csrw   satp, {satp_value}",
                "sfence.vma",
                satp_value = in(reg) satp_val,
            )
        }
    }

    fn tlb_shootdown(vpn: VirtPageNum) {
        riscv::asm::sfence_vma(0, vpn.to_virt_addr().get() as usize);
    }

    fn tlb_shootdown_range(range: VirtPageRange) {
        let npages = range.npages();
        perf_counter_add!(RISCV64_TLB_RANGE_PAGES, npages);
        if use_full_tlb_flush(npages) {
            perf_counter_inc!(RISCV64_TLB_FULL_FLUSHES);
            Self::tlb_shootdown_all();
        } else {
            perf_counter_add!(RISCV64_TLB_PAGE_FLUSHES, npages);
            for offset in 0..npages {
                Self::tlb_shootdown(range.start() + offset);
            }
        }
    }

    fn tlb_shootdown_all() {
        riscv::asm::sfence_vma_all();
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn range_policy_uses_full_flush_only_above_threshold() {
        let threshold = RISCV64_TLB_FLUSH_ALL_THRESHOLD_PAGES as u64;
        assert!(!use_full_tlb_flush(threshold));
        if let Some(above) = threshold.checked_add(1) {
            assert!(use_full_tlb_flush(above));
        }
    }
}

pub struct Sv39KernelLayout;

impl KernelLayoutTrait<Sv39PagingArch> for Sv39KernelLayout {
    const USPACE_TOP_VPN: VirtPageNum =
        VirtPageNum::new(1 << (Sv39PagingArch::PAGE_LEVELS * Sv39PagingArch::PGDIR_IDX_BITS) >> 1);

    const FREE_SPACE_VPN: VirtPageNum = VirtPageNum::new(
        (Self::KSPACE_VPN.to_virt_addr().get() + PHYS_RAM_START.get() + MAX_PHYS_RAM_SIZE)
            >> Sv39PagingArch::PAGE_SIZE_BITS,
    );

    const KERNEL_VA_BASE_VPN: VirtPageNum =
        VirtPageNum::new(KERNEL_VA_BASE.get() >> Sv39PagingArch::PAGE_SIZE_BITS);

    const KERNEL_LA_BASE_VPN: PhysPageNum =
        PhysPageNum::new(KERNEL_LA_BASE.get() >> Sv39PagingArch::PAGE_SIZE_BITS);

    const DIRECT_MAPPING_VPN: VirtPageNum = Self::KSPACE_VPN;
}
