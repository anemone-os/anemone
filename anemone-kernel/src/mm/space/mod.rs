//! Memory management helpers for user address spaces.
//!
//! This module provides `UserSpace` which encapsulates a process' page
//! table, stack and heap area management, segment loading helpers, and
//! small helpers such as `MemArea` and page-table guard types used by the
//! rest of the kernel when accessing user page tables.
use core::{
    mem::swap,
    ops::{Deref, DerefMut, Index},
};

use range_allocator::Rangable;

use crate::{mm::kptable::KERNEL_PTABLE, prelude::*};

mod api;
pub use api::*;
pub mod image;

pub const MAX_USER_STACK_PAGES: u64 = const {
    const_assert!(
        MAX_USER_STACK_SIZE % PagingArch::PAGE_SIZE_BYTES as u64 == 0,
        "user stack size must be a multiple of page size"
    );
    MAX_USER_STACK_SIZE / PagingArch::PAGE_SIZE_BYTES as u64
};

pub const INIT_USER_STACK_PAGES: u64 = const {
    const_assert!(
        INIT_USER_STACK_SIZE % PagingArch::PAGE_SIZE_BYTES as u64 == 0,
        "initial user stack size must be a multiple of page size"
    );
    INIT_USER_STACK_SIZE / PagingArch::PAGE_SIZE_BYTES as u64
};

pub const MAX_HEAP_PAGES: u64 = const {
    const_assert!(
        MAX_HEAP_SIZE % PagingArch::PAGE_SIZE_BYTES as u64 == 0,
        "user heap size must be a multiple of page size"
    );
    MAX_HEAP_SIZE / PagingArch::PAGE_SIZE_BYTES as u64
};

#[derive(Debug)]
pub struct UserSpace {
    /// Physical page number of the root page-table for quick access.
    ///
    /// This is a cached copy of the page-table root PPN. The actual page
    /// table lives in [UserSpaceInner].table.
    table_ppn: PhysPageNum,
    inner: RwLock<UserSpaceInner>,
}
#[derive(Debug)]
pub struct UserSpaceInner {
    table: PageTable,
    ustack: MemArea,
    uheap: MemArea,
    ubrk: VirtAddr,
    seg_frames: Vec<FrameHandle>,
}
impl UserSpace {
    pub fn new_empty() -> Self {
        let table = PageTable::new();
        Self {
            table_ppn: table.root_ppn(),
            inner: RwLock::new(UserSpaceInner {
                table,
                ustack: MemArea::new(
                    KernelLayout::USPACE_TOP_VPN,
                    MAX_USER_STACK_PAGES as usize,
                    AreaGrowthDirection::Lower,
                    PteFlags::READ | PteFlags::WRITE | PteFlags::USER | PteFlags::VALID,
                )
                .unwrap(),
                uheap: MemArea::new(
                    VirtPageNum::new(0),
                    MAX_HEAP_PAGES as usize,
                    AreaGrowthDirection::Higher,
                    PteFlags::READ | PteFlags::WRITE | PteFlags::USER | PteFlags::VALID,
                )
                .unwrap(),
                ubrk: VirtPageNum::new(0).to_virt_addr(),
                seg_frames: vec![],
            }),
        }
    }

    pub fn new_user() -> Result<Self, MmError> {
        let mut table = PageTable::new();
        KERNEL_PTABLE.copy_to_ptable(&mut table);
        let stack = MemArea::prealloc(
            KernelLayout::USPACE_TOP_VPN,
            MAX_USER_STACK_PAGES as usize,
            AreaGrowthDirection::Lower,
            PteFlags::READ | PteFlags::WRITE | PteFlags::USER | PteFlags::VALID,
            INIT_USER_STACK_PAGES as usize,
            &mut table,
        )?;

        // heap will be initialized after loading the user image
        let heap = MemArea::new(
            VirtPageNum::new(0),
            MAX_HEAP_PAGES as usize,
            AreaGrowthDirection::Higher,
            PteFlags::READ | PteFlags::WRITE | PteFlags::USER | PteFlags::VALID,
        )?;
        Ok(UserSpace {
            table_ppn: table.root_ppn(),
            inner: RwLock::new(UserSpaceInner {
                table,

                ustack: stack,
                uheap: heap,
                ubrk: VirtPageNum::new(0).to_virt_addr(),
                seg_frames: vec![],
            }),
        })
    }

    /// Add a memory segment to the user space, and fill the segment with the
    /// given data.
    ///
    /// This function will automatically adjust the `ubrk` value and the
    /// position of the heap area.
    ///
    /// # Safety
    /// This function is unsafe because:
    ///  * **any already mapped page tables will not be rolled back if an
    ///    exception is encountered during the page table mapping process.**
    ///  * This function does not validate address range conflicts with existing
    ///    mappings, potentially causing code/data overwrites.
    ///  * **Call after the heap area is initialized will lead to panic.**.
    pub unsafe fn add_segment(
        &self,
        vaddr: VirtAddr,
        vsize: usize,
        psize: usize,
        source: &[u8],
        rwx_flags: PteFlags,
    ) -> Result<(), MmError> {
        let mut inner = self.inner.write();
        debug_assert!(inner.uheap.vpn_range.len() == 0);
        let vaddr_ed = vaddr + vsize as u64;
        let vpn_st = vaddr.page_down();
        let vpn_ed = vaddr_ed.page_up();
        if vpn_ed > inner.uheap.vpn_range.start() {
            inner.uheap.vpn_range = VirtPageRange::new(vpn_ed, 0u64);
            inner.ubrk = vpn_ed.to_virt_addr();
        }
        let mut mapper = inner.table.mapper();
        let mut frames = vec![];
        for vpn in vpn_st.get()..vpn_ed.get() {
            let vpn = VirtPageNum::new(vpn);
            if let Some(translated) = mapper.translate(vpn) {
                if translated.flags.extract_rwx() != rwx_flags {
                    // segment with different flags is already mapped in this page
                    return Err(MmError::AlreadyMapped);
                }
            } else {
                unsafe {
                    let frame = alloc_frame()
                        .ok_or(MmError::OutOfMemory)?
                        .into_frame_handle();
                    mapper.map_one(
                        vpn,
                        frame.ppn(),
                        rwx_flags | PteFlags::USER | PteFlags::VALID,
                        0,
                        false,
                    )?;
                    frames.push(frame);
                }
            }
        }
        unsafe {
            mapper.fill_data(vaddr, source, psize as u64)?;
        }
        if vsize > psize {
            struct ZeroData;
            impl Index<usize> for ZeroData {
                type Output = u8;
                fn index(&self, index: usize) -> &Self::Output {
                    &0
                }
            }
            unsafe {
                mapper.fill_data(vaddr + psize as u64, &ZeroData, (vsize - psize) as u64)?;
            }
        }
        inner.seg_frames.extend(frames);
        Ok(())
    }

    pub fn set_brk(&self, brk: VirtAddr) -> Result<(), MmError> {
        /// Adjust the program break for this address space.
        ///
        /// This function grows or shrinks the heap tracked in
        /// [UserSpaceInner].uheap to make `brk` the new program break. It
        /// returns an error if the requested break is out-of-range or if
        /// allocation fails while growing.
        let mut inner = self.inner.write();
        if brk < inner.uheap.vpn_range.start().to_virt_addr() {
            return Err(MmError::OutOfMemory); // see reference https://www.man7.org/linux/man-pages/man2/brk.2.html
        }
        if brk
            > inner.uheap.vpn_range.start().to_virt_addr()
                + (MAX_HEAP_PAGES << PagingArch::PAGE_SIZE_BITS) as u64
        {
            return Err(MmError::OutOfMemory);
        }
        let brk_ppn = brk.page_up();
        if inner.uheap.vpn_range.end() > brk_ppn {
            // shrink heap
            let mut count = inner.uheap.vpn_range.end() - brk_ppn;
            let mut uheap = MemArea::EMPTY;
            swap(&mut inner.uheap, &mut uheap);
            let mut mapper = inner.table.mapper();
            unsafe {
                mapper.try_unmap(Unmapping {
                    range: VirtPageRange::new(uheap.vpn_range.end() - count as u64, count as u64),
                }); // unmap the newly mapped pages
                while count > 0 {
                    uheap.shrink();
                    count -= 1;
                }
            }
            swap(&mut inner.uheap, &mut uheap);
        } else if brk_ppn > inner.uheap.vpn_range.end() {
            let mut uheap = MemArea::EMPTY;
            swap(&mut inner.uheap, &mut uheap);
            let mut grown: usize = 0;
            // grow heap
            while uheap.vpn_range.end().to_virt_addr() < brk {
                uheap.grow(&mut inner.table).map_err(|e| {
                    // roll back the changes to the page table and the heap area
                    kerr!(
                        "out of memory when growing heap area, unmapping {} pages",
                        grown
                    );
                    let mut mapper = inner.table.mapper();
                    unsafe {
                        mapper.try_unmap(Unmapping {
                            range: VirtPageRange::new(
                                uheap.vpn_range.end() - grown as u64,
                                grown as u64,
                            ),
                        }); // unmap the newly mapped pages
                        while grown > 0 {
                            uheap.shrink();
                            grown -= 1;
                        }
                    }

                    swap(&mut inner.uheap, &mut uheap);
                    e
                })?;
                grown += 1;
                if grown % 3000 == 0 {
                    kdebugln!(
                        "grown heap to {} pages ( {} MiB)",
                        uheap.pages(),
                        uheap.pages() * 4 / 1024
                    );
                }
            }
            swap(&mut inner.uheap, &mut uheap);
        }
        inner.ubrk = brk;
        Ok(())
    }

    pub fn page_table(&self) -> USpacePTableReadGuard<'_> {
        /// Return a read guard for inspecting this address space's page
        /// table. The guard dereferences to a [PageTable].
        USpacePTableReadGuard::new(self)
    }

    pub fn page_table_mut(&self) -> USpacePTableWriteGuard<'_> {
        /// Return a write guard for mutating this address space's page
        /// table. The guard allows safe mutation of the underlying
        /// [PageTable].
        USpacePTableWriteGuard::new(self)
    }

    pub fn activate(&self) {
        /// Make this [UserSpace] active on the CPU so address translation
        /// uses its page table. This calls architecture-specific helpers
        /// to load the root page-table.
        unsafe {
            PagingArch::activate_addr_space(&self.inner.read().table);
        }
    }
}
impl Drop for UserSpace {
    fn drop(&mut self) {
        kdebugln!("memspace with root ppn {:?} dropped", self.table_ppn,);
    }
}

impl PartialEq for UserSpace {
    fn eq(&self, other: &Self) -> bool {
        self.table_ppn == other.table_ppn
    }
}

impl Eq for UserSpace {}

#[derive(Debug)]
pub enum AreaGrowthDirection {
    Higher,
    Lower,
}

#[derive(Debug)]
pub struct MemArea {
    vpn_range: VirtPageRange,
    frames: Vec<FrameHandle>,
    max_pages: usize,
    growth: AreaGrowthDirection,
    flags: PteFlags,
}

impl MemArea {
    pub const EMPTY: Self = Self {
        vpn_range: VirtPageRange::new(VirtPageNum::new(0), 0),
        frames: Vec::new(),
        max_pages: 0,
        growth: AreaGrowthDirection::Higher,
        flags: PteFlags::empty(),
    };

    /// Create a new memory area
    pub const fn new(
        init_vpn: VirtPageNum,
        max_pages: usize,
        growth: AreaGrowthDirection,
        flags: PteFlags,
    ) -> Result<Self, MmError> {
        let mut memarea = MemArea {
            vpn_range: VirtPageRange::new(init_vpn, 0),
            frames: vec![],
            max_pages: max_pages,
            growth: growth,
            flags,
        };
        Ok(memarea)
    }

    /// Create a new memory area and preallocate the area with `prealloc` pages.
    pub fn prealloc(
        init_vpn: VirtPageNum,
        max_pages: usize,
        growth: AreaGrowthDirection,
        flags: PteFlags,
        prealloc: usize,
        page_table: &mut PageTable,
    ) -> Result<Self, MmError> {
        if prealloc > max_pages {
            return Err(MmError::InvalidArgument);
        }
        let mut memarea = MemArea::new(init_vpn, max_pages, growth, flags)?;
        for _ in 0..prealloc {
            memarea.grow(page_table)?;
        }
        Ok(memarea)
    }

    pub fn pages(&self) -> usize {
        self.vpn_range.len()
    }

    pub fn max_pages(&self) -> usize {
        self.max_pages
    }

    /// Grow the area by one page.
    pub fn grow(&mut self, page_table: &mut PageTable) -> Result<(), MmError> {
        if self.pages() + 1 > self.max_pages {
            return Err(MmError::InvalidArgument);
        }
        let frame = unsafe {
            alloc_frame()
                .ok_or(MmError::OutOfMemory)?
                .into_frame_handle()
        };
        let mut mapper = page_table.mapper();
        match self.growth {
            AreaGrowthDirection::Lower => unsafe {
                mapper.map_one(
                    self.vpn_range.start() - 1,
                    frame.ppn(),
                    self.flags,
                    0,
                    false,
                )?;
                self.vpn_range = VirtPageRange::new(
                    self.vpn_range.start() - 1,
                    (self.vpn_range.len() + 1) as u64,
                )
            },
            AreaGrowthDirection::Higher => unsafe {
                mapper.map_one(self.vpn_range.end(), frame.ppn(), self.flags, 0, false)?;
                self.vpn_range =
                    VirtPageRange::new(self.vpn_range.start(), (self.vpn_range.len() + 1) as u64)
            },
        }
        self.frames.push(frame);
        Ok(())
    }

    /// Shrink the area by one page. Returns `true` if the area is shrunk, or
    /// `false` if the area is already empty.
    ///
    /// **This function will not unmap pages**
    pub unsafe fn shrink(&mut self) -> bool {
        if self.pages() == 0 {
            return false;
        }
        match self.growth {
            AreaGrowthDirection::Lower => {
                self.vpn_range = VirtPageRange::new(
                    self.vpn_range.start() + 1 as u64,
                    (self.vpn_range.len() - 1) as u64,
                );
            },
            AreaGrowthDirection::Higher => {
                self.vpn_range =
                    VirtPageRange::new(self.vpn_range.start(), (self.vpn_range.len() - 1) as u64)
            },
        }
        self.frames.pop();
        true
    }
}

pub struct USpacePTableReadGuard<'a> {
    guard: ReadNoPreemptGuard<'a, UserSpaceInner>,
}

impl Deref for USpacePTableReadGuard<'_> {
    type Target = PageTable;

    fn deref(&self) -> &Self::Target {
        &self.guard.table
    }
}

impl<'a> USpacePTableReadGuard<'a> {
    pub fn new(uspace: &'a UserSpace) -> Self {
        Self {
            guard: uspace.inner.read(),
        }
    }
}

pub struct USpacePTableWriteGuard<'a> {
    guard: WriteNoPreemptGuard<'a, UserSpaceInner>,
}

impl Deref for USpacePTableWriteGuard<'_> {
    type Target = PageTable;

    fn deref(&self) -> &Self::Target {
        &self.guard.table
    }
}

impl DerefMut for USpacePTableWriteGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard.table
    }
}

impl<'a> USpacePTableWriteGuard<'a> {
    pub fn new(uspace: &'a UserSpace) -> Self {
        Self {
            guard: uspace.inner.write(),
        }
    }
}
