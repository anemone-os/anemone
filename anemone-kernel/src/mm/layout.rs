use crate::prelude::*;

/// Intentionally, this trait is not suffixed with "Arch", since the memory
/// layout is not determined by the architecture, but rather by the platform and
/// the kernel design.
pub trait KernelLayoutTrait<P: PagingArchTrait> {
    /// The starting virtual address of the direct mapping region, i.e., the
    /// region where physical memory is directly mapped to virtual memory with a
    /// fixed offset.
    ///
    /// Typically, this address is at the upper half of the virtual address
    /// space.
    ///
    /// e.g. on a riscv sv39 system, the virtual address space is 512 GiB, and
    /// the direct mapping region starts at 256 GiB, so the offset is 256 GiB.
    const DIRECT_MAPPING_ADDR: u64;

    /// The offset between the physical address and the virtual address in the
    /// direct mapping region.
    const DIRECT_MAPPING_OFFSET: usize = const {
        let ret = Self::DIRECT_MAPPING_ADDR as usize - 0;
        assert!(ret.is_multiple_of(P::PAGE_SIZE_BYTES));
        ret
    };

    /// The starting virtual address of the kernel mapping region, i.e., the
    /// region where the kernel is mapped to virtual memory.
    const KERNEL_VA_BASE: u64;

    /// Where the kernel is loaded in physical memory, i.e., the physical
    /// address of the kernel image.
    const KERNEL_LA_BASE: u64;

    /// The offset between the physical address and the virtual address in the
    /// kernel mapping region.
    const KERNEL_MAPPING_OFFSET: usize = (Self::KERNEL_VA_BASE - Self::KERNEL_LA_BASE) as usize;

    // starting from (Self::DIRECT_MAPPING_ADDR + MAX_PHYS_MEM_SIZE), Anemone
    // defines various virtual memory regions for management.

    /// vmalloc and ioremap region.
    const REMAP_REGION: VirtPageRange = VirtPageRange::new(
        VirtPageNum::new(
            (Self::DIRECT_MAPPING_ADDR + PHYS_RAM_START + MAX_PHYS_RAM_SIZE) >> P::PAGE_SIZE_BITS,
        ),
        P::NPAGES_PER_GB as u64 * (1 << REMAP_SHIFT_GB),
    );

    /// Convert a physical address to a virtual address in the direct mapping
    /// region.
    fn phys_to_hhdm(paddr: PhysAddr) -> VirtAddr {
        VirtAddr::new(paddr.get() + Self::DIRECT_MAPPING_OFFSET as u64)
    }

    /// Convert a virtual address in the direct mapping region to a physical
    /// address.
    unsafe fn hhdm_to_phys(vaddr: VirtAddr) -> PhysAddr {
        PhysAddr::new(vaddr.get() - Self::DIRECT_MAPPING_OFFSET as u64)
    }

    /// Convert a physical address to a virtual address in the region where the
    /// kernel is executed.
    fn phys_to_kvirt(paddr: PhysAddr) -> VirtAddr {
        VirtAddr::new(paddr.get() + Self::KERNEL_MAPPING_OFFSET as u64)
    }

    /// Convert a virtual address in the region where the kernel is executed to
    /// a physical address.
    unsafe fn kvirt_to_phys(vaddr: VirtAddr) -> PhysAddr {
        PhysAddr::new(vaddr.get() - Self::KERNEL_MAPPING_OFFSET as u64)
    }
}
