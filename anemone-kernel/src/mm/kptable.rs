//! Kernel Root Page Directory.
//!
//! All regions in the kernel virtual address space (upper half) are
//! pre-calculated and mapped during kernel initialization, and all page
//! directories won't be modified at runtime. **Leaf mappings might be changed
//! however.**
//!
//! All processes's upper half virtual address space is identical to the
//! kernel's upper half.

use crate::{mm::layout::KernelLayoutTrait, prelude::*};

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
        map_elf_segment!(text, PteFlags::READ | PteFlags::EXECUTE | PteFlags::GLOBAL);
        map_elf_segment!(rodata, PteFlags::READ | PteFlags::GLOBAL);
        map_elf_segment!(data, PteFlags::READ | PteFlags::WRITE | PteFlags::GLOBAL);
        map_elf_segment!(bss, PteFlags::READ | PteFlags::WRITE | PteFlags::GLOBAL);
    }

    fn prealloc_rest_pgdirs(&self) {
        let kptable = self.ptable.lock_irqsave();
        let pdir = unsafe {
            kptable
                .root_ppn()
                .to_hhdm()
                .to_virt_addr()
                .as_ptr_mut::<PgDir>()
                .as_mut()
                .expect("root ppn should not be null")
        };
        let mut allocated = 0;
        for index in (KernelLayout::USPACE_TOP_VPN.get()
            >> (PagingArch::PGDIR_IDX_BITS * (PagingArch::PAGE_LEVELS - 1)))
            as usize..PagingArch::PTE_PER_PGDIR
        {
            if pdir[index].is_empty() {
                let ppn = alloc_frame_zeroed()
                    .expect("failed to preallocate frames for kernel space page table.")
                    .leak();
                pdir[index] = Pte::new(
                    ppn,
                    PteFlags::VALID | PteFlags::GLOBAL,
                    PagingArch::PAGE_LEVELS - 1,
                );
                allocated += 1;
            }
        }
        kinfoln!("preallocated {} pgdirs for kernel space", allocated);
    }

    unsafe fn kmap(&self, mapping: Mapping) -> Result<(), MmError> {
        let mut kpgdir = self.ptable.lock_irqsave();
        let mut mapper = kpgdir.mapper();
        mapper.map(mapping)
    }

    unsafe fn kunmap(&self, unmapping: Unmapping) {
        let mut kpgdir = self.ptable.lock_irqsave();
        let mut mapper = kpgdir.mapper();
        unsafe {
            mapper.try_unmap(unmapping);
        }
    }

    fn ktranslate(&self, vpn: VirtPageNum) -> Option<Translated> {
        let mut kpgdir = self.ptable.lock_irqsave();
        let mapper = kpgdir.mapper();
        mapper.translate(vpn)
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
        kdebugln!("setting up direct mapping region...");
        PagingArch::setup_direct_mapping_region(&mut KERNEL_PTABLE.ptable.lock_irqsave());
        kdebugln!("preallocating pgdirs for the rest of kernel space regions...");
        KERNEL_PTABLE.prealloc_rest_pgdirs();
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
        for i in 0..mapping.npages {
            PagingArch::tlb_shootdown((mapping.vpn + i as u64).to_virt_addr());
        }
    }
    if broadcast_ipi_async(IpiPayload::TlbShootdown { vaddr: None }).is_err() {
        kwarningln!("failed to send TLB shootdown IPI after kmap");
    }
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
        for i in 0..unmapping.range.npages() {
            PagingArch::tlb_shootdown((unmapping.range.start() + i as u64).to_virt_addr());
        }
    }
    if broadcast_ipi_async(IpiPayload::TlbShootdown { vaddr: None }).is_err() {
        kwarningln!("failed to send TLB shootdown IPI after kunmap");
    }
}

/// Translate a kernel virtual page number in the global kernel page table.
pub fn ktranslate(vpn: VirtPageNum) -> Option<Translated> {
    KERNEL_PTABLE.ktranslate(vpn)
}
