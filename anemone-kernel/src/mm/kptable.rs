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

use crate::{exception::broadcast_ipi, mm::layout::KernelLayoutTrait, prelude::*};

static KERNEL_PTABLE: KPTable = KPTable::new();

/// Nothing more than [crate::PageTable]. Just provides some specialized methods
/// for mapping kernel memory regions, and serves as a marker type for the
/// kernel's root page directory.
#[derive(Debug)]
struct KPTable {
    ptable: Lazy<SpinLock<PageTable>>,
}

impl KPTable {
    pub const fn new() -> Self {
        Self {
            ptable: Lazy::new(|| SpinLock::new(PageTable::new())),
        }
    }

    unsafe fn map_kvirt(&self) {
        use arch::link_symbols::*;
        let mut kptable = self.ptable.lock_irqsave();
        let mut mapper = kptable.mapper();

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

                    unsafe {
                        mapper.map_overwrite(Mapping {
                            vpn: svpn,
                            ppn: sppn,
                            flags: $flags,
                            npages: npages as usize,
                            huge_pages: true,
                        }).expect(concat!("failed to map kernel ", stringify!($name), " segment"));
                    }
                }
            }};
        }
        map_elf_segment!(text, PteFlags::READ | PteFlags::EXECUTE);
        map_elf_segment!(rodata, PteFlags::READ);
        map_elf_segment!(data, PteFlags::READ | PteFlags::WRITE);
        map_elf_segment!(bss, PteFlags::READ | PteFlags::WRITE);
    }

    unsafe fn map_hhdm(&self) {
        let mut kptable = self.ptable.lock_irqsave();
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
                            flags: PteFlags::READ | PteFlags::WRITE,
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
                    PteFlags::READ | PteFlags::WRITE,
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
                                flags: PteFlags::READ | PteFlags::WRITE,
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

    unsafe fn init_virtual_ranges(&self) {
        let mut kpgdir = self.ptable.lock_irqsave();

        unsafe {
            kinfoln!(
                "preallocate pgdirs for remap region: [{:#x}, {:#x})",
                KernelLayout::REMAP_REGION.start().get(),
                KernelLayout::REMAP_REGION.end().get()
            );
            PagingArch::prealloc_pgdirs_for_region(&mut kpgdir, KernelLayout::REMAP_REGION);
        }
    }

    unsafe fn kmap(&self, mapping: Mapping) -> Result<(), MmError> {
        let mut kpgdir = self.ptable.lock_irqsave();
        let mut mapper = kpgdir.mapper();
        mapper.map(mapping)
    }

    unsafe fn kunmap(&self, unmapping: Unmapping) {
        let mut kpgdir = self.ptable.lock_irqsave();
        let mut mapper = kpgdir.mapper();
        unsafe{
            mapper.try_unmap(unmapping);
        }
    }
}

/// Initialize the kernel mapping, i.e., map all the necessary kernel memory
/// regions to the kernel's root page directory, including:
/// - HHDM region / direct mapping region
/// - kernel image region / kvirt region
/// - vmalloc region
///
/// Note that, after this function is called, all top-level pgdirs will not be
/// changed at runtime, So we can safely share the same pgdir for all processes.
/// (TLB shootdowns are still needed when leaf mappings are changed, but that's
/// a different story.)
pub fn init_kernel_mapping() {
    unsafe {
        kdebugln!("mapping kernel image segments...");
        KERNEL_PTABLE.map_kvirt();
        kdebugln!("mapping direct mapping region...");
        KERNEL_PTABLE.map_hhdm();
        kdebugln!("preallocating pgdirs for remap and vmalloc region...");
        KERNEL_PTABLE.init_virtual_ranges();
    }
}

/// Switch to kernel mapping.
pub unsafe fn activate_kernel_mapping() {
    unsafe {
        let kpgdir = KERNEL_PTABLE.ptable.lock_irqsave();
        PagingArch::activate_addr_space(&kpgdir);
    }
}

/// Do a mapping in the global kernel page table.
///
/// This is always a non-overwrite mapping, thus the operation itself is
/// safe.
///
/// However, this mapping occurs in the global kernel page table, which is
/// shared by all processes, so it might cause many many potential issues if
/// not used carefully. So we mark this function as unsafe.
pub unsafe fn kmap(mapping: Mapping) -> Result<(), MmError> {
    unsafe {
        KERNEL_PTABLE.kmap(mapping)?;
    }
    broadcast_ipi_async(IpiPayload::TlbShootdown { vaddr: None })
        .expect("failed to send TLB shootdown IPI");
    Ok(())
}

/// Do an unmapping in the global kernel page table. See [kmap] for details and
/// safety concerns.
///
/// Though an unmapping always succeeds, this function should not be overused,
/// as it will cause TLB shootdowns to all cores, which is very expensive. So we
/// mark this function as unsafe.
pub unsafe fn kunmap(unmapping: Unmapping) {
    unsafe {
        KERNEL_PTABLE.kunmap(unmapping);
    }
    broadcast_ipi_async(IpiPayload::TlbShootdown { vaddr: None })
        .expect("failed to send TLB shootdown IPI");
}
