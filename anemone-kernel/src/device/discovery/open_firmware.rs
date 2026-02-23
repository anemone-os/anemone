//! Device Tree as a platform discovery mechanism

use crate::{device::discovery::PlatformDiscovery, prelude::*};

mod early {
    use core::marker::PhantomData;

    use fdt::nodes::cpus::CpuStatus;

    use super::*;

    #[derive(Debug)]
    pub struct EarlyMemoryScanner<'a> {
        _lifetime: PhantomData<&'a ()>,

        avail_set: rangemap::RangeSet<u64>, // ppn, not addr
        rsv_map: rangemap::RangeMap<u64, RsvMemFlags>, // ppn, not addr
    }

    impl EarlyMemoryScanner<'_> {
        /// Scan the memory layout (including reserved memory regions) from the
        /// device tree, and return an [EarlyMemoryScanner] instance that holds
        /// the scanned information for later use.
        ///
        /// # Safety
        ///
        /// **Obvious.**
        ///
        /// If some unsupported features are used in the device tree (e.g.
        /// hotpluggable memory), we'll throw a panic immediately.
        pub unsafe fn new(fdt_va: VirtAddr) -> Self {
            unsafe {
                let fdt = fdt::Fdt::from_ptr(fdt_va.as_ptr()).expect("failed to parse device tree");

                // The momory regions are allowed to overlap in DeviceTree format,
                // so we have to handle them.
                let mut avail_set = rangemap::RangeSet::new();

                // For those reserved memory regions with the same flags but different names,
                // we can safely merge them together.
                let mut rsv_map = rangemap::RangeMap::new();

                if fdt.root().memory().hotpluggable() {
                    panic!("hotpluggable memory is not supported");
                }
                if fdt.root().memory().initial_mapped_area().is_some() {
                    panic!("initial mapped area is not supported");
                }

                for region in fdt
                    .root()
                    .memory()
                    .reg()
                    .iter::<u64, u64>()
                    .map(|reg| reg.expect("failed to parse memory reg property"))
                {
                    // page align the memory regions, and add them to the available set.
                    let sppn = (align_down_power_of_2!(region.address, PagingArch::PAGE_SIZE_BYTES)
                        >> PagingArch::PAGE_SIZE_BITS) as u64;
                    let eppn = (align_up_power_of_2!(
                        region.address + region.len,
                        PagingArch::PAGE_SIZE_BYTES
                    ) >> PagingArch::PAGE_SIZE_BITS) as u64;
                    if sppn >= eppn {
                        continue;
                    }

                    kinfoln!(
                        "EarlyMemoryScanner: found memory region: {:#x} - {:#x}",
                        region.address,
                        region.address + region.len
                    );
                    //avail_set.insert(region.address..region.address + region.len);
                    avail_set.insert(sppn..eppn);
                }

                for rsv_mem in fdt.root().reserved_memory().children() {
                    if let Some(reg) = rsv_mem.reg() {
                        for region in reg
                            .iter::<u64, u64>()
                            .map(|reg| reg.expect("failed to parse reserved memory reg property"))
                        {
                            // for reserved memory we take a subset instead of a superset, which is
                            // different from the available memory.

                            let sppn =
                                (align_up_power_of_2!(region.address, PagingArch::PAGE_SIZE_BYTES)
                                    >> PagingArch::PAGE_SIZE_BITS)
                                    as u64;
                            let eppn = (align_down_power_of_2!(
                                region.address + region.len,
                                PagingArch::PAGE_SIZE_BYTES
                            ) >> PagingArch::PAGE_SIZE_BITS)
                                as u64;
                            avail_set.remove(sppn..eppn);

                            let mut rsv_flags = RsvMemFlags::empty();
                            if rsv_mem.no_map() {
                                rsv_flags |= RsvMemFlags::NOMAP;
                            }
                            if rsv_mem.reusable() {
                                rsv_flags |= RsvMemFlags::REUSABLE;
                            }
                            rsv_map.insert(sppn..eppn, rsv_flags);
                            kinfoln!(
                                "EarlyMemoryScanner: found reserved memory region: {:#x} - {:#x}, flags: {:?}",
                                region.address,
                                region.address + region.len,
                                rsv_flags
                            );
                        }
                    }
                }

                // add kernel image as a mappable reserved memory region.
                let __skernel = align_down_power_of_2!(
                    link_symbols::__skernel as *const () as u64 - KERNEL_VA_BASE + KERNEL_LA_BASE,
                    PagingArch::PAGE_SIZE_BYTES
                ) as u64;
                let __ekernel = align_up_power_of_2!(
                    link_symbols::__ekernel as *const () as u64 - KERNEL_VA_BASE + KERNEL_LA_BASE,
                    PagingArch::PAGE_SIZE_BYTES
                ) as u64;

                let skernel_ppn = __skernel >> PagingArch::PAGE_SIZE_BITS;
                let ekernel_ppn = __ekernel >> PagingArch::PAGE_SIZE_BITS;
                assert!(skernel_ppn < ekernel_ppn);

                avail_set.remove(skernel_ppn..ekernel_ppn);
                rsv_map.insert(skernel_ppn..ekernel_ppn, RsvMemFlags::KVIRT);
                kinfoln!(
                    "EarlyMemoryScanner: found kernel image region: {:#x} - {:#x}",
                    __skernel,
                    __ekernel
                );

                Self {
                    _lifetime: PhantomData,
                    avail_set,
                    rsv_map,
                }
            }
        }

        /// Directly carve out the required number of frames from the available
        /// memory regions, and register them as a [RsvMemZone] with
        /// `EARLY_ALLOC` flag.
        ///
        /// Return the starting physical page number of the allocated region on
        /// success, or panic on failure.
        ///
        /// The allocated frames will never be deallocated. A.k.a. they are
        /// leaked.
        ///
        /// # Safety
        ///
        /// **Obvious.**
        pub fn early_alloc_folio(&mut self, npages: u64) -> PhysPageNum {
            let mut allocated_ppn = None;

            for avail_region in self.avail_set.iter() {
                let region_npages = avail_region.end - avail_region.start;
                if region_npages >= npages {
                    allocated_ppn = Some(avail_region.start);
                    self.avail_set
                        .remove(avail_region.start..avail_region.start + npages);
                    self.rsv_map.insert(
                        allocated_ppn.unwrap()..allocated_ppn.unwrap() + npages,
                        RsvMemFlags::EARLY_ALLOC,
                    );
                    break;
                }
            }

            if let Some(allocated_ppn) = allocated_ppn {
                kinfoln!(
                    "EarlyMemoryScanner: allocated folio: {:#x} - {:#x}",
                    allocated_ppn << PagingArch::PAGE_SIZE_BITS,
                    (allocated_ppn + npages) << PagingArch::PAGE_SIZE_BITS
                );
                PhysPageNum::new(allocated_ppn)
            } else {
                panic!("failed to allocate folio with {} pages", npages);
            }
        }

        /// Register the scanned memory regions to physical memory management
        /// subsystem.
        pub fn commit_to_pmm(self) {
            unsafe {
                for avail_region in self.avail_set.iter() {
                    let sppn = PhysPageNum::new(avail_region.start);
                    let eppn = PhysPageNum::new(avail_region.end);

                    let npages = eppn.get() - sppn.get();
                    add_mem_zone(MemZone::Avail(AvailMemZone::new(sppn, npages)));
                }
                for (rsv_region, rsv_flags) in self.rsv_map.iter() {
                    let sppn = PhysPageNum::new(rsv_region.start);
                    let eppn = PhysPageNum::new(rsv_region.end);

                    let npages = eppn.get() - sppn.get();
                    add_mem_zone(MemZone::Rsv(RsvMemZone::new(sppn, npages, *rsv_flags)));
                }
            }
        }
    }

    /// Scan the clock frequency from the device tree.
    ///
    /// # Safety
    ///
    /// - The caller must ensure that the provided `fdt` is valid.
    /// - For those platforms that have multiple CPUs with different clock
    ///   frequencies, this function will panic. Such platforms are rare, and we
    ///   must rewrite the whole timer HAL to support them. For now, Anemone
    ///   just doesn't support them.
    pub unsafe fn early_scan_clock_freq(fdt: VirtAddr) -> u64 {
        let fdt = unsafe { fdt::Fdt::from_ptr(fdt.as_ptr()) }.expect("failed to parse device tree");
        fdt.root()
            .cpus()
            .common_timebase_frequency()
            .expect("no timebase-frequency property found in device tree")
    }

    /// Scan the CPU count from the device tree.
    ///
    /// Mostly used for waking up APs in SMP initialization.
    ///
    /// # Safety
    ///
    /// - The caller must ensure that the provided `fdt` is valid.
    pub unsafe fn early_scan_cpu_count(fdt: VirtAddr) -> usize {
        let fdt = unsafe { fdt::Fdt::from_ptr(fdt.as_ptr()) }.expect("failed to parse device tree");

        let mut ncpus = 0;

        for cpu in fdt.root().cpus().iter() {
            match cpu.status() {
                Some(CpuStatus::OKAY) => ncpus += 1,
                _ => panic!("unsupported CPU status: {:?}", cpu.status()),
            }
        }

        ncpus
    }
}
pub use early::*;

/// Unflattened device tree. In-memory representation. Runtime-modifiable.
#[derive(Debug)]
pub struct DeviceTree {
    // todo
}

impl PlatformDiscovery for DeviceTree {}

// static DEVICE_TREE: ... = ...;

pub unsafe fn unflatten_device_tree(_fdt: VirtAddr) {
    todo!()
}
