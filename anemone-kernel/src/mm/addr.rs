use crate::prelude::*;

pub trait KernelVirtAddrExt {
    #[inline(always)]
    unsafe fn hhdm_to_phys(self) -> PhysAddr;
    #[inline(always)]
    unsafe fn kvirt_to_phys(self) -> PhysAddr;
    #[inline(always)]
    fn to_vpn(self) -> VirtPageNum;
}

pub trait KernelPhysAddrExt {
    #[inline(always)]
    fn to_hhdm(self) -> VirtAddr;
    #[inline(always)]
    fn to_kvirt(self) -> VirtAddr;
    #[inline(always)]
    fn to_ppn(self) -> PhysPageNum;
}

pub trait KernelVirtPageNumExt {
    #[inline(always)]
    unsafe fn hhdm_to_phys(self) -> PhysPageNum;
    #[inline(always)]
    unsafe fn kvirt_to_phys(self) -> PhysPageNum;
    #[inline(always)]
    fn to_vaddr(self) -> VirtAddr;
}

pub trait KernelPhysPageNumExt {
    #[inline(always)]
    fn to_hhdm(self) -> VirtPageNum;
    #[inline(always)]
    fn to_kvirt(self) -> VirtPageNum;
    #[inline(always)]
    fn to_paddr(self) -> PhysAddr;
}

pub trait KernelPhysPageRangeExt {
    #[inline(always)]
    fn to_hhdm(self) -> VirtPageRange;
    #[inline(always)]
    fn to_kvirt(self) -> VirtPageRange;
}

pub trait KernelVirtPageRangeExt {
    #[inline(always)]
    unsafe fn hhdm_to_phys(self) -> PhysPageRange;
    #[inline(always)]
    unsafe fn kvirt_to_phys(self) -> PhysPageRange;
}

impl KernelVirtAddrExt for VirtAddr {
    unsafe fn hhdm_to_phys(self) -> PhysAddr {
        unsafe { KernelLayout::hhdm_to_phys(self) }
    }

    unsafe fn kvirt_to_phys(self) -> PhysAddr {
        unsafe { KernelLayout::kvirt_to_phys(self) }
    }

    fn to_vpn(self) -> VirtPageNum {
        VirtPageNum::new(self.get() >> PagingArch::PAGE_SIZE_BITS)
    }
}

impl KernelPhysAddrExt for PhysAddr {
    fn to_hhdm(self) -> VirtAddr {
        KernelLayout::phys_to_hhdm(self)
    }

    fn to_kvirt(self) -> VirtAddr {
        KernelLayout::phys_to_kvirt(self)
    }

    fn to_ppn(self) -> PhysPageNum {
        PhysPageNum::new(self.get() >> PagingArch::PAGE_SIZE_BITS)
    }
}

impl KernelVirtPageNumExt for VirtPageNum {
    unsafe fn hhdm_to_phys(self) -> PhysPageNum {
        unsafe { KernelLayout::hhdm_to_phys(self.to_vaddr()).to_ppn() }
    }

    unsafe fn kvirt_to_phys(self) -> PhysPageNum {
        unsafe { KernelLayout::kvirt_to_phys(self.to_vaddr()).to_ppn() }
    }

    fn to_vaddr(self) -> VirtAddr {
        VirtAddr::new(self.get() << PagingArch::PAGE_SIZE_BITS)
    }
}

impl KernelPhysPageNumExt for PhysPageNum {
    fn to_hhdm(self) -> VirtPageNum {
        self.to_paddr().to_hhdm().to_vpn()
    }

    fn to_kvirt(self) -> VirtPageNum {
        self.to_paddr().to_kvirt().to_vpn()
    }

    fn to_paddr(self) -> PhysAddr {
        PhysAddr::new(self.get() << PagingArch::PAGE_SIZE_BITS)
    }
}

impl KernelPhysPageRangeExt for PhysPageRange {
    fn to_hhdm(self) -> VirtPageRange {
        VirtPageRange::new(self.start().to_hhdm(), self.npages())
    }

    fn to_kvirt(self) -> VirtPageRange {
        VirtPageRange::new(self.start().to_kvirt(), self.npages())
    }
}

impl KernelVirtPageRangeExt for VirtPageRange {
    unsafe fn hhdm_to_phys(self) -> PhysPageRange {
        unsafe { PhysPageRange::new(self.start().hhdm_to_phys(), self.npages()) }
    }

    unsafe fn kvirt_to_phys(self) -> PhysPageRange {
        unsafe { PhysPageRange::new(self.start().kvirt_to_phys(), self.npages()) }
    }
}
