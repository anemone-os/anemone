//! Kernel Root Page Directory.
//!
//! All regions in the kernel virtual address space (upper half) are
//! pre-calculated and mapped during kernel initialization, and all page
//! directories won't be modified at runtime. **Leaf mappings might be changed
//! however.**
//!
//! All processes's upper half virtual address space is identical to the
//! kernel's upper half.

use spin::Lazy;

use crate::prelude::*;

static KERNEL_PGDIR: KPgDir = KPgDir::new();

/// Nothing more than [crate::PageTable]. Just provides some specialized methods
/// for mapping kernel memory regions, and serves as a marker type for the
/// kernel's root page directory.
#[derive(Debug)]
pub struct KPgDir {
    pgdir: Lazy<SpinLock<PageTable>>,
}

impl KPgDir {
    pub const fn new() -> Self {
        Self {
            pgdir: Lazy::new(|| SpinLock::new(PageTable::new())),
        }
    }

    unsafe fn map_kvirt(&self) {
        use arch::link_symbols::*;
        let mut kpgdir = self.pgdir.lock_irqsave();
        let mut mapper = kpgdir.mapper();

        macro_rules! map_elf_segment {
            ($name:ident, $flags:expr) => {{
                paste::paste! {
                    let vstart = [<__s $name>] as *const () as usize;
                    let vend = [<__e $name>] as *const () as usize;
                    let svpn = VirtPageNum::new((align_down_power_of_2!(
                        vstart as u64,
                        PagingArch::PAGE_SIZE_BYTES) / PagingArch::PAGE_SIZE_BYTES) as u64
                    );
                    let evpn = VirtPageNum::new((align_up_power_of_2!(
                        vend as u64,
                        PagingArch::PAGE_SIZE_BYTES) / PagingArch::PAGE_SIZE_BYTES) as u64
                    );
                    let npages = evpn - svpn;

                    let pstart = vstart - KERNEL_VA_BASE as usize + KERNEL_LA_BASE as usize;
                    let sppn = PhysPageNum::new((align_down_power_of_2!(
                        pstart as u64,
                        PagingArch::PAGE_SIZE_BYTES) / PagingArch::PAGE_SIZE_BYTES) as u64
                    );

                    mapper.map(Mapping {
                        vpn: svpn,
                        ppn: sppn,
                        flags: $flags,
                        npages: npages as usize,
                        overwrite: false,
                    }).expect(concat!("failed to map kernel ", stringify!($name), " segment"));
                }
            }};
        }

        map_elf_segment!(text, PteFlags::READ | PteFlags::EXECUTE);
        map_elf_segment!(rodata, PteFlags::READ);
        map_elf_segment!(data, PteFlags::READ | PteFlags::WRITE);
        map_elf_segment!(bss, PteFlags::READ | PteFlags::WRITE);
    }

    unsafe fn map_hhdm(&self) {
        let mut kpgdir = self.pgdir.lock_irqsave();
        let mut mapper = kpgdir.mapper();

        // TODO: switch to huge page mapping.
        // currently we use normal page mapping for hhdm region, which is tooooooo
        // slow...
        {
            let avail_mem_zones = AVAIL_MEM_ZONES.lock_irqsave();
            for zone in avail_mem_zones.iter() {
                let range = zone.range();

                mapper
                    .map(Mapping {
                        vpn: range.start().to_hhdm(),
                        ppn: range.start(),
                        flags: PteFlags::READ | PteFlags::WRITE,
                        npages: range.npages() as usize,
                        overwrite: false,
                    })
                    .expect("failed to map direct mapping region");

                kdebugln!(
                    "mapped direct mapping region:\n\tvirtual page number {} ~ {},\n\tphysical page number {} ~ {},\n\tflags = {:?}",
                    range.start().to_hhdm(),
                    range.end().to_hhdm(),
                    range.start(),
                    range.end(),
                    PteFlags::READ | PteFlags::WRITE,
                );
            }
        }

        // reserved memory regions
        {
            let rsv_mem_zones = RSV_MEM_ZONES.lock_irqsave();
            for zone in rsv_mem_zones.iter() {
                if zone.flags().is_mappable() {
                    let range = zone.range();

                    mapper
                        .map(Mapping {
                            vpn: range.start().to_hhdm(),
                            ppn: range.start(),
                            // TODO: for kvirt region, we may want to map with more fine-grained
                            // permissions.
                            flags: PteFlags::READ | PteFlags::WRITE,
                            npages: range.npages() as usize,
                            overwrite: false,
                        })
                        .expect("failed to map reserved memory region");

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
        }
    }
}

/// Initialize the kernel mapping, i.e., map all the necessary kernel memory
/// regions to the kernel's root page directory, including:
/// - HHDM region / direct mapping region
/// - kernel image region / kvirt region
/// - TODO: vmalloc region, vmemmap region, etc.
///
/// Note that, after this function is called, all pgdirs will not be changed at
/// runtime, only leaf mappings might be changed. So we can safely share the
/// same pgdir for all processes. (IPI shooting and TLB shootdowns are still
/// needed when leaf mappings are changed, but that's a different story.)
pub unsafe fn init_kernel_mapping() {
    unsafe {
        KERNEL_PGDIR.map_kvirt();
        KERNEL_PGDIR.map_hhdm();
    }
}

pub unsafe fn activate_kernel_mapping() {
    unsafe {
        let kpgdir = KERNEL_PGDIR.pgdir.lock_irqsave();
        let root_ppn = kpgdir.root_ppn();
        PagingArch::activate_addr_space(root_ppn);
    }
}
