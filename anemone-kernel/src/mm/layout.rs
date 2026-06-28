use crate::prelude::*;

/// Intentionally, this trait is not suffixed with "Arch", since the memory
/// layout is not determined by the architecture, but rather by the platform and
/// the kernel design.
///
/// Common kernel layout and key constants:
///
/// ## Page Table Managed Spaces:
/// ```
///  -0x0   +-----------+---------------+ -0x0
///         |           |     KERNEL    |
///         |           +---------------+ KERNEL_VA_BASE
///         |           |   (INVALID)   |
///         |   KSPACE  +---------------+ REMAP_REGION_TOP
///         |           |  REMAP REGION |
///         |           +---------------+ FREE_SPACE_ADDR
///         |           |    OTHERS     |
///         +-----------+---------------+ KSPACE_ADDR
///         |         (INVALID)         |
///         +-----------+---------------+ USPACE_TOP_ADDR
///         |           USPACE          |
///   0x0   +---------------------------+ 0x0
/// ```
///
/// The place of Direct Mapping(dm) Area is arch-specific.
///
///  * In some architectures, like RISC-V, they may be placed at `OTHERS` area
///    in the `KSPACE`, and the `FREE_SPACE` is calculated by `KSPACE_ADDR +
///    MAX_PHYS_ADDR`.
///  * In some other architectures, like loongarch, they may be placed outside
///    of Page Table Managed Spaces, that means, they are placed in the
///    `(INVALID)` area, using other mapping mechanism like DMWs. In this case,
///    `FREE_SPACE_ADDR = KSPACE_ADDR`.
pub trait KernelLayoutTrait<P: PagingArchTrait> {
    /// Top USPACE VPN.
    ///
    /// USPACE VAddr range is [0, [Self::USPACE_TOP_ADDR]).
    const USPACE_TOP_VPN: VirtPageNum;

    /// Top USPACE VAddr.
    const USPACE_TOP_ADDR: u64 = Self::USPACE_TOP_VPN.get() << P::PAGE_SIZE_BITS;

    /// Base KSPACE VPN.
    ///
    /// KSPACE starts at [Self::KSPACE_ADDR].
    const KSPACE_VPN: VirtPageNum = VirtPageNum::new(
        ((0 as u64).overflowing_sub(Self::USPACE_TOP_ADDR).0) >> P::PAGE_SIZE_BITS,
    );

    /// Base KSPACE VAddr.
    const KSPACE_ADDR: u64 = Self::KSPACE_VPN.get() << P::PAGE_SIZE_BITS;

    const KSPACE_START_INDEX: usize = (KernelLayout::USPACE_TOP_VPN.get()
        >> (PagingArch::PGDIR_IDX_BITS * (PagingArch::PAGE_LEVELS - 1)))
        as usize;

    /// First free VPN in KSPACE for page-table-managed regions.
    ///
    /// On some platforms where DM is outside page-table-managed KSPACE, this
    /// may be equal to [Self::KSPACE_VPN].
    ///
    /// `FREE` means that they can be mapped to any physical page.
    const FREE_SPACE_VPN: VirtPageNum;

    /// First free VAddr in KSPACE for page-table-managed regions.
    ///
    /// See [Self::FREE_SPACE_VPN].
    const FREE_SPACE_ADDR: u64 = Self::FREE_SPACE_VPN.get() << P::PAGE_SIZE_BITS;

    /// Base DM VPN.
    const DIRECT_MAPPING_VPN: VirtPageNum;

    /// Base DM VAddr.
    const DIRECT_MAPPING_ADDR: u64 = Self::DIRECT_MAPPING_VPN.get() << P::PAGE_SIZE_BITS;

    /// Base kernel VPN.
    const KERNEL_VA_BASE_VPN: VirtPageNum;

    /// Base kernel VAddr.
    const KERNEL_VA_BASE: u64 = Self::KERNEL_VA_BASE_VPN.get() << P::PAGE_SIZE_BITS;

    /// Base PAddr where kernel is loaded.
    const KERNEL_LA_BASE_VPN: PhysPageNum;

    /// Place where kernel is loaded in physical memory.
    const KERNEL_LA_BASE: u64 = Self::KERNEL_LA_BASE_VPN.get() << P::PAGE_SIZE_BITS;

    /// Kernel mapping offset between VAddr and PAddr.
    ///
    /// `kvirt_vaddr = paddr + `[Self::KERNEL_MAPPING_OFFSET].
    const KERNEL_MAPPING_OFFSET: usize = (Self::KERNEL_VA_BASE - Self::KERNEL_LA_BASE) as usize;

    // starting from (Self::DIRECT_MAPPING_ADDR + MAX_PHYS_MEM_SIZE), Anemone
    // defines various virtual memory regions for management.

    /// Remap region in KSPACE.
    ///
    /// Starts from [Self::FREE_SPACE_VPN].
    const REMAP_REGION: VirtPageRange = VirtPageRange::new(
        Self::FREE_SPACE_VPN,
        P::NPAGES_PER_GB as u64 * (1 << REMAP_SHIFT_GB),
    );

    /// Convert PAddr to DM VAddr.
    fn phys_to_dm(paddr: PhysAddr) -> VirtAddr {
        VirtAddr::new(paddr.get() + Self::DIRECT_MAPPING_ADDR as u64)
    }

    /// Convert DM VAddr to PAddr.
    unsafe fn dm_to_phys(vaddr: VirtAddr) -> PhysAddr {
        PhysAddr::new(vaddr.get() - Self::DIRECT_MAPPING_ADDR as u64)
    }

    /// Convert PAddr to kernel VAddr.
    fn phys_to_kvirt(paddr: PhysAddr) -> VirtAddr {
        VirtAddr::new(paddr.get() + Self::KERNEL_MAPPING_OFFSET as u64)
    }

    /// Convert kernel VAddr to PAddr.
    unsafe fn kvirt_to_phys(vaddr: VirtAddr) -> PhysAddr {
        PhysAddr::new(vaddr.get() - Self::KERNEL_MAPPING_OFFSET as u64)
    }
}
