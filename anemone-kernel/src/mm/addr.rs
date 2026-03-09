//! Strongly-typed wrappers around addresses.

use crate::{int_like, mm::layout::KernelLayoutTrait, prelude::*};
use core::{
    fmt::{Debug, Display},
    ops::{Add, AddAssign, BitAnd, BitAndAssign, BitOr, BitOrAssign, Not, Sub, SubAssign},
};

int_like!(PhysAddr, u64);
int_like!(VirtAddr, u64);

int_like!(PhysPageNum, u64);
int_like!(VirtPageNum, u64);

macro_rules! impl_addr {
    ($addr_type:ty) => {
        impl $addr_type {
            pub fn lower_32_bits(&self) -> u32 {
                (self.get() & 0xffffffff) as u32
            }

            pub fn upper_32_bits(&self) -> u32 {
                (self.get() >> 32) as u32
            }
        }
    };
}

macro_rules! impl_ops {
    ($addr_type:ty) => {
        impl Display for $addr_type {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "0x{:016x}", self.get())
            }
        }

        impl Add<u64> for $addr_type {
            type Output = Self;

            fn add(self, rhs: u64) -> Self::Output {
                Self::new(self.get() + rhs)
            }
        }

        impl AddAssign<u64> for $addr_type {
            fn add_assign(&mut self, rhs: u64) {
                *self = Self::new(self.get() + rhs);
            }
        }

        impl Sub<u64> for $addr_type {
            type Output = Self;

            fn sub(self, rhs: u64) -> Self::Output {
                Self::new(self.get() - rhs)
            }
        }

        impl Sub<Self> for $addr_type {
            type Output = u64;

            fn sub(self, rhs: Self) -> Self::Output {
                self.get() - rhs.get()
            }
        }

        impl SubAssign<u64> for $addr_type {
            fn sub_assign(&mut self, rhs: u64) {
                *self = Self::new(self.get() - rhs);
            }
        }

        impl BitAnd<u64> for $addr_type {
            type Output = Self;

            fn bitand(self, rhs: u64) -> Self::Output {
                Self::new(self.get() & rhs)
            }
        }

        impl BitAndAssign<u64> for $addr_type {
            fn bitand_assign(&mut self, rhs: u64) {
                *self = Self::new(self.get() & rhs);
            }
        }

        impl BitOr<u64> for $addr_type {
            type Output = Self;

            fn bitor(self, rhs: u64) -> Self::Output {
                Self::new(self.get() | rhs)
            }
        }

        impl BitOrAssign<u64> for $addr_type {
            fn bitor_assign(&mut self, rhs: u64) {
                *self = Self::new(self.get() | rhs);
            }
        }

        impl Not for $addr_type {
            type Output = Self;

            fn not(self) -> Self::Output {
                Self::new(!self.get())
            }
        }
    };
}

impl_addr!(PhysAddr);
impl_ops!(PhysAddr);
impl_addr!(VirtAddr);
impl_ops!(VirtAddr);
impl_ops!(PhysPageNum);
impl_ops!(VirtPageNum);

impl Into<PhysAddr> for PhysPageNum {
    fn into(self) -> PhysAddr {
        self.to_phys_addr()
    }
}

impl Into<VirtAddr> for VirtPageNum {
    fn into(self) -> VirtAddr {
        self.to_virt_addr()
    }
}

impl PhysPageNum {
    pub const fn to_phys_addr(&self) -> PhysAddr {
        PhysAddr::new(self.get() << PagingArch::PAGE_SIZE_BITS)
    }
}

impl VirtPageNum {
    pub const fn to_virt_addr(&self) -> VirtAddr {
        VirtAddr::new(self.get() << PagingArch::PAGE_SIZE_BITS)
    }
}

macro_rules! impl_page_range {
    ($name:ident, $pn_type:ty) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name {
            start: $pn_type,
            npages: u64,
        }
        paste::paste! {
            #[derive(Clone, Copy)]
            pub struct [<$name Iter>] {
                range: $name,
                next: $pn_type,
            }
        }
        impl $name {
            pub const fn new(start: $pn_type, npages: u64) -> Self {
                Self { start, npages }
            }

            pub const fn start(&self) -> $pn_type {
                self.start
            }

            pub const fn end(&self) -> $pn_type {
                <$pn_type>::new(self.start.get() + self.npages)
            }

            pub const fn npages(&self) -> u64 {
                self.npages
            }

            pub const fn contains(&self, pn: $pn_type) -> bool {
                self.start.get() <= pn.get() && pn.get() < self.start.get() + self.npages
            }

            pub const fn intersects(&self, other: &Self) -> bool {
                self.start.get() < other.end().get() && other.start.get() < self.end().get()
            }

            paste::paste! {
                pub const fn iter(&self) -> [<$name Iter>] {
                    [<$name Iter>] {
                        range: *self,
                        next: self.start,
                    }
                }
            }
        }
        paste::paste! {
            impl Iterator for [<$name Iter>] {
                type Item = $pn_type;

                fn next(&mut self) -> Option<Self::Item> {
                    if self.next < self.range.end() {
                        let pn = self.next;
                        self.next += 1;
                        Some(pn)
                    } else {
                        None
                    }
                }
            }
        }
    };
}

impl_page_range!(PhysPageRange, PhysPageNum);
impl_page_range!(VirtPageRange, VirtPageNum);

impl Debug for PhysPageRange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "[{:#x}, {:#x}) ({} pages)",
            self.start.to_phys_addr().get(),
            self.end().to_phys_addr().get(),
            self.npages
        )
    }
}

impl Debug for VirtPageRange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "[{:#x}, {:#x}) ({} pages)",
            self.start.to_virt_addr().get(),
            self.end().to_virt_addr().get(),
            self.npages
        )
    }
}

impl PhysAddr {
    pub fn to_hhdm(self) -> VirtAddr {
        KernelLayout::phys_to_hhdm(self)
    }

    pub fn to_kvirt(self) -> VirtAddr {
        KernelLayout::phys_to_kvirt(self)
    }
}

impl PhysPageNum {
    pub fn to_hhdm(self) -> VirtPageNum {
        VirtPageNum::new(self.to_phys_addr().to_hhdm().get() >> PagingArch::PAGE_SIZE_BITS)
    }

    pub fn to_kvirt(self) -> VirtPageNum {
        VirtPageNum::new(self.to_phys_addr().to_kvirt().get() >> PagingArch::PAGE_SIZE_BITS)
    }
}

impl PhysPageRange {
    pub fn to_hhdm(self) -> VirtPageRange {
        VirtPageRange::new(self.start.to_hhdm(), self.npages)
    }

    pub fn to_kvirt(self) -> VirtPageRange {
        VirtPageRange::new(self.start.to_kvirt(), self.npages)
    }
}

impl VirtAddr {
    pub unsafe fn hhdm_to_phys(self) -> PhysAddr {
        unsafe { KernelLayout::hhdm_to_phys(self) }
    }

    pub unsafe fn kvirt_to_phys(self) -> PhysAddr {
        unsafe { KernelLayout::kvirt_to_phys(self) }
    }

    pub fn as_ptr<T>(&self) -> *const T {
        core::ptr::with_exposed_provenance(self.get() as usize)
    }

    pub fn as_ptr_mut<T>(&self) -> *mut T {
        core::ptr::with_exposed_provenance_mut(self.get() as usize)
    }
}

impl VirtPageNum {
    pub unsafe fn hhdm_to_phys(self) -> PhysPageNum {
        unsafe {
            PhysPageNum::new(self.to_virt_addr().hhdm_to_phys().get() >> PagingArch::PAGE_SIZE_BITS)
        }
    }

    pub unsafe fn kvirt_to_phys(self) -> PhysPageNum {
        unsafe {
            PhysPageNum::new(
                self.to_virt_addr().kvirt_to_phys().get() >> PagingArch::PAGE_SIZE_BITS,
            )
        }
    }
}

impl VirtPageRange {
    pub unsafe fn hhdm_to_phys(self) -> PhysPageRange {
        unsafe { PhysPageRange::new(self.start.hhdm_to_phys(), self.npages) }
    }

    pub unsafe fn kvirt_to_phys(self) -> PhysPageRange {
        unsafe { PhysPageRange::new(self.start.kvirt_to_phys(), self.npages) }
    }
}
