use super::paging::LA64PagingArch;
use crate::{mm::layout::KernelLayoutTrait, prelude::*};

/// LoongArch64 kernel layout constants and derived virtual page numbers.
pub struct LA64KernelLayout;

impl KernelLayoutTrait<LA64PagingArch> for LA64KernelLayout {
    const USPACE_TOP_VPN: VirtPageNum = VirtPageNum::new(
        (1 << (LA64PagingArch::PAGE_LEVELS * LA64PagingArch::PGDIR_IDX_BITS) >> 1),
    );

    const FREE_SPACE_VPN: VirtPageNum =
        VirtPageNum::new(Self::KSPACE_VPN.to_virt_addr().get() >> LA64PagingArch::PAGE_SIZE_BITS);

    const KERNEL_VA_BASE_VPN: VirtPageNum =
        VirtPageNum::new(KERNEL_VA_BASE >> LA64PagingArch::PAGE_SIZE_BITS);

    const KERNEL_LA_BASE_VPN: PhysPageNum =
        PhysPageNum::new(KERNEL_LA_BASE >> LA64PagingArch::PAGE_SIZE_BITS);

    const DIRECT_MAPPING_VPN: VirtPageNum =
        VirtPageNum::new(0x9000_0000_0000_0000 >> LA64PagingArch::PAGE_SIZE_BITS);
}

impl LA64KernelLayout {
    /// Temporary virtual address used for early I/O mapping.
    pub const TEMPORARY_IO_VPN: VirtPageNum =
        VirtPageNum::new(0x8000_0000_0000_0000 >> LA64PagingArch::PAGE_SIZE_BITS);
    /// Temporary virtual address of the early I/O window.
    pub const TEMPORARY_IO_ADDR: u64 = Self::TEMPORARY_IO_VPN.to_virt_addr().get();
}
