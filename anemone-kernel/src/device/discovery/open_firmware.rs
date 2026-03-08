//! Device Tree as a platform discovery mechanism
//!
//! Currently, for early scanning we rely on `fdt` crate, and for unflattening
//! fdt and later operations we rely on our own implementation in `device-tree`
//! crate. This is not ideal, and we should extend our own implementation to
//! cover the early scanning as well, so that we can remove the dependency on
//! `fdt` crate. However, this is not a high priority task, and we can do it in
//! future when we have more time.

use core::ptr::NonNull;

use crate::{
    device::{
        bus::{
            ROOT_BUS,
            platform::{self, PlatformDevice},
        },
        discovery::fwnode::FwNode,
        idalloc::alloc_device_id,
        kobject::{KObjIdent, KObject, KObjectBase},
        resource::Resource,
    },
    prelude::*,
    sync::mono::MonoOnce,
};

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

        pub unsafe fn mark_as_reserved(
            &mut self,
            start: PhysPageNum,
            npages: u64,
            flags: RsvMemFlags,
        ) {
            self.avail_set.remove(start.get()..start.get() + npages);
            self.rsv_map
                .insert(start.get()..start.get() + npages, flags);
        }

        /// Register the scanned memory regions to physical memory management
        /// subsystem.
        pub fn commit_to_pmm(self) {
            unsafe {
                for avail_region in self.avail_set.iter() {
                    let sppn = PhysPageNum::new(avail_region.start);
                    let eppn = PhysPageNum::new(avail_region.end);

                    let npages = eppn.get() - sppn.get();
                    sys_mem_zones().add_mem_zone(MemZone::Avail(AvailMemZone::new(sppn, npages)));
                }
                for (rsv_region, rsv_flags) in self.rsv_map.iter() {
                    let sppn = PhysPageNum::new(rsv_region.start);
                    let eppn = PhysPageNum::new(rsv_region.end);

                    let npages = eppn.get() - sppn.get();
                    sys_mem_zones()
                        .add_mem_zone(MemZone::Rsv(RsvMemZone::new(sppn, npages, *rsv_flags)));
                }
            }
        }
    }

    /// Get the size in bytes of the device tree blob by parsing the header of
    /// the FDT.
    ///
    /// # Safety
    ///
    /// - Caller must ensure that the provided `fdt` is valid and points to a
    ///   valid FDT blob.
    pub unsafe fn early_scan_fdt_size(fdt: VirtAddr) -> usize {
        let fdt = unsafe { fdt::Fdt::from_ptr(fdt.as_ptr()) }.expect("failed to parse device tree");
        fdt.total_size()
    }

    /// Scan the clock frequency from the device tree.
    ///
    /// # Safety
    ///
    /// - Caller must ensure that the provided `fdt` is valid.
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
    /// - Caller must ensure that the provided `fdt` is valid.
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
use device_tree::DeviceStatus;
pub use early::*;

/// Unflattened device tree. In-memory representation.
/// TODO:
/// - Runtime-modifiability.
#[derive(Debug)]
struct DeviceTree {
    handle: device_tree::DeviceTreeHandle,
}

/// Initialized by bsp before waking up other cores, so no sync primitive is
/// needed.
static DEVICE_TREE: MonoOnce<Arc<DeviceTree>> = unsafe { MonoOnce::new() };

pub unsafe fn unflatten_device_tree(fdt_va: VirtAddr) {
    let parser = unsafe { device_tree::FdtParser::new(fdt_va.as_ptr()) };
    let handle = parser.parse(|layout| {
        // allocate from frame allocator for efficiency
        let npages = align_up_power_of_2!(layout.size(), PagingArch::PAGE_SIZE_BYTES)
            >> PagingArch::PAGE_SIZE_BITS;
        let sppn = alloc_frames(npages)
            .expect("failed to allocate frames for unflattened device tree")
            .leak()
            .start();

        let ptr = core::ptr::slice_from_raw_parts_mut(
            sppn.to_hhdm().to_virt_addr().as_ptr_mut(),
            layout.size(),
        );
        unsafe { Some(NonNull::new_unchecked(ptr)) }
    });

    DEVICE_TREE.init(|| Arc::new(DeviceTree { handle }));
}

/// Traverse the unflattened device tree and create devices accordingly.
///
/// This function will scan all devices under `simple-bus` compatible nodes.
pub fn of_platform_discovery() {
    fn range_contains(base: u64, size: u64, addr: u64, len: u64) -> bool {
        let Some(range_end) = base.checked_add(size) else {
            return false;
        };
        let Some(addr_end) = addr.checked_add(len) else {
            return false;
        };
        addr >= base && addr_end <= range_end
    }

    fn of_platform_discovery_inner(
        simple_bus_node: &device_tree::DeviceNode,
        simple_bus_dev: &Arc<PlatformDevice>,
        // (simple_bus_addr, simple_bus_cpu_addr, length)
        translated_ranges: &Vec<(u64, u64, u64)>,
    ) {
        for child in simple_bus_node.children() {
            if let Some(mut compatible) = child.compatible() {
                // device found
                if child.status() != DeviceStatus::Okay {
                    continue;
                }

                let kobj_base = KObjectBase::new(KObjIdent::try_from(child.full_name()).unwrap());
                let dev_base = DeviceBase::new(
                    alloc_device_id().unwrap(),
                    Some(Box::new(OpenFirmwareNode { node_ptr: child })),
                );
                let mut pdev = PlatformDevice::new(kobj_base, dev_base);
                // kobj init
                pdev.set_parent(Some(simple_bus_dev.clone()));
                // pdev init.
                for c in compatible.clone() {
                    pdev.add_compatible(c);
                }
                // resource parsing
                // 1. reg
                if let Some(reg) = child.reg() {
                    for (on_bus_address, length) in reg.iter() {
                        if length == 0 {
                            continue;
                        }

                        let mut translated = false;
                        for &(bus_addr, bus_cpu_addr, range_len) in translated_ranges.iter() {
                            if range_contains(bus_addr, range_len, on_bus_address, length) {
                                let translated_addr = bus_cpu_addr + (on_bus_address - bus_addr);
                                pdev.add_resource(Resource::Mmio {
                                    base: PhysAddr::new(translated_addr),
                                    len: length as usize,
                                });
                                translated = true;
                                break;
                            }
                        }
                        if !translated {
                            kerrln!(
                                "of_platform_discovery: failed to parse mmio resource for device {}",
                                child.path()
                            );
                        }
                    }
                }

                let pdev = Arc::new(pdev);
                // physical topology
                simple_bus_dev.add_child(pdev.clone());
                platform::register_device(pdev.clone());

                if compatible.any(|s| s == "simple-bus") {
                    let mut subbus_ranges = vec![];

                    let ranges = child.ranges().expect(&format!(
                        "no ranges property found for simple-bus compatible node {}",
                        child.path()
                    ));
                    let mut is_empty = true;

                    for (subbus_addr, on_simple_bus_addr, length) in ranges.iter() {
                        is_empty = false;

                        if length == 0 {
                            continue;
                        }

                        let mut translated = false;
                        for &(simple_bus_addr, simple_bus_cpu_addr, range_len) in
                            translated_ranges.iter()
                        {
                            if range_contains(
                                simple_bus_addr,
                                range_len,
                                on_simple_bus_addr,
                                length,
                            ) {
                                let translated_subbus_addr =
                                    simple_bus_cpu_addr + (on_simple_bus_addr - simple_bus_addr);
                                subbus_ranges.push((subbus_addr, translated_subbus_addr, length));
                                translated = true;
                                break;
                            }
                        }

                        if !translated {
                            kerrln!(
                                "of_platform_discovery: failed to parse sub bus range for simple-bus node {}",
                                child.path()
                            );
                        }
                    }

                    if is_empty {
                        // empty ranges means identity mapping in DT semantics.
                        subbus_ranges = translated_ranges.clone();
                    }

                    // sub platform bus
                    of_platform_discovery_inner(child, &pdev, &subbus_ranges);
                }
            }
        }
    }

    let device_tree = DEVICE_TREE.get();
    // MMIO resources always reside in lower half of the physical address space.
    let initial_mapping = vec![(0, 0, u64::MAX)];
    of_platform_discovery_inner(device_tree.handle.root(), &ROOT_BUS, &initial_mapping);
}

#[derive(Debug)]
pub struct OpenFirmwareNode {
    /// Pointer to the corresponding node in the unflattened device tree.
    ///
    /// This pointer is always valid since the device tree is immutable and will
    /// never be deallocated.
    ///
    /// **However, this is not an appropriate design and just a temporary
    /// workaround. We should refine device-tree crate later to support our
    /// needs.**
    node_ptr: *const device_tree::DeviceNode,
}

impl OpenFirmwareNode {
    fn node(&self) -> &device_tree::DeviceNode {
        unsafe { &*self.node_ptr }
    }
}

// See `node_ptr` field of `OpenFirmwareNode` for the safety of this.
unsafe impl Send for OpenFirmwareNode {}
unsafe impl Sync for OpenFirmwareNode {}

impl FwNode for OpenFirmwareNode {
    fn prop_read_u32(&self, prop_name: &str) -> Option<u32> {
        self.node()
            .properties()
            .find(|p| p.name() == prop_name)?
            .value_as_u32()
    }

    fn prop_read_u64(&self, prop_name: &str) -> Option<u64> {
        self.node()
            .properties()
            .find(|p| p.name() == prop_name)?
            .value_as_u64()
    }

    fn prop_read_str(&self, prop_name: &str) -> Option<String> {
        self.node()
            .properties()
            .find(|p| p.name() == prop_name)?
            .value_as_string()
            .map(|s| s.to_string())
    }

    fn prop_read_present(&self, prop_name: &str) -> bool {
        self.node().properties().any(|p| p.name() == prop_name)
    }
}
