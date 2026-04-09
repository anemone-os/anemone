//! Memory management helpers for user address spaces.
//!
//! This module provides [UserSpace], which encapsulates a process' page
//! table, stack and heap area management, segment loading helpers, and
//! small helpers such as [MemArea], [USpacePTableReadGuard], and
//! [USpacePTableWriteGuard] used by the rest of the kernel when accessing
//! user page tables.
use core::{
    fmt::Debug,
    ops::{ControlFlow, Deref, DerefMut},
    ptr::copy,
};

use range_allocator::Rangable;

use crate::{
    mm::kptable::KERNEL_PTABLE,
    prelude::*,
    utils::data::{DataSource, SliceDataSource, ZeroDataSource},
};

mod api;
pub use api::*;
pub mod fault;
pub mod image;

// TODO: these constants should be in KB, not in pages.

/// Maximum number of pages that a user stack may occupy.
///
/// This value is expressed in pages and derived from the configuration
/// constant [USER_STACK_SHIFT_KB]. Refer to [MAX_USER_STACK_PAGES].
pub const MAX_USER_STACK_PAGES: u64 = const {
    const MAX_USER_STACK_BYTES: u64 = 1 << USER_STACK_SHIFT_KB << 10;
    const_assert!(
        MAX_USER_STACK_BYTES % PagingArch::PAGE_SIZE_BYTES as u64 == 0,
        "user stack size must be a multiple of page size"
    );
    MAX_USER_STACK_BYTES / PagingArch::PAGE_SIZE_BYTES as u64
};

/// Initial number of pages allocated for a newly created user stack.
///
/// Derived from [USER_INIT_STACK_SHIFT_KB]. See [INIT_USER_STACK_PAGES].
pub const INIT_USER_STACK_PAGES: u64 = const {
    const INIT_USER_STACK_BYTES: u64 = 1 << USER_INIT_STACK_SHIFT_KB << 10;
    const_assert!(
        INIT_USER_STACK_BYTES % PagingArch::PAGE_SIZE_BYTES as u64 == 0,
        "initial user stack size must be a multiple of page size"
    );
    INIT_USER_STACK_BYTES / PagingArch::PAGE_SIZE_BYTES as u64
};

/// Maximum number of pages the user heap may grow to.
///
/// Derived from [USER_HEAP_SHIFT_MB]. See [MAX_HEAP_PAGES].
pub const MAX_HEAP_PAGES: u64 = const {
    const MAX_HEAP_BYTES: u64 = 1 << USER_HEAP_SHIFT_MB << 20;
    const_assert!(
        MAX_HEAP_BYTES % PagingArch::PAGE_SIZE_BYTES as u64 == 0,
        "user heap size must be a multiple of page size"
    );
    MAX_HEAP_BYTES / PagingArch::PAGE_SIZE_BYTES as u64
};

#[derive(Debug)]
pub struct UserSpace {
    /// Physical page number of the root page-table for quick access.
    ///
    /// This is a cached copy of the page-table root PPN. The actual page
    /// table lives in `inner.table`.
    table_ppn: PhysPageNum,
    data: RwLock<UserSpaceData>,
}

#[derive(Debug)]
pub struct UserSpaceData {
    /// Underlying page table for this address space.
    table: PageTable,
    /// User stack area managed by this address space.
    ustack: DynamicMemArea,
    /// Only used during task initialization, the initial stack pointer for the
    /// user stack.
    uinit_sp: VirtAddr,
    /// User heap area tracked by `brk`.
    uheap: DynamicMemArea,
    /// Current program break for this address space.
    ubrk: VirtAddr,
    /// Segments
    segs: Vec<Segment>,
}

pub trait CloneableMemArea: MemArea {
    unsafe fn clone(&mut self) -> Self;
    /// Create a copy of this segment for use by a new address space.
    ///
    /// The returned [`Segment`] shares frames with the original. The
    /// write permissions of both two [PageTable]s are cleared (copy-on-write
    /// semantics).
    unsafe fn create_copy(
        &mut self,
        cur_mapper: &mut Mapper,
        new_mapper: &mut Mapper,
    ) -> Result<Self, MmError>
    where
        Self: Sized,
    {
        let new_perm;
        let perm = self.perm();
        let range = self.range();
        if perm.contains(PteFlags::WRITE) {
            new_perm = perm - PteFlags::WRITE;
            unsafe {
                cur_mapper.change_flags(
                    range,
                    |_, _| Some(new_perm | PteFlags::USER | PteFlags::VALID),
                    TraverseOrder::PreOrder,
                )
            };
        } else {
            new_perm = perm;
        }
        let new = unsafe { self.clone() };
        unsafe {
            new.map_to(new_mapper, new_perm | PteFlags::USER | PteFlags::VALID);
        }
        Ok(new)
    }
}

pub trait MemArea: Debug {
    fn range(&self) -> VirtPageRange;
    fn perm(&self) -> PteFlags;
    fn frame_mut(&mut self, index: usize) -> Option<&mut FrameHandle>;
    fn frame(&self, index: usize) -> Option<&FrameHandle>;

    fn pages(&self) -> usize {
        self.range().len() as usize
    }

    /// Perform copy-on-write for the page at virtual page number `vpn`.
    ///
    /// Returns an error if the area is not writable or if allocation
    /// fails. This method clones the underlying frame if it is currently
    /// shared, leaving the original frame intact for other owners.
    unsafe fn copy_on_write(&mut self, vpn: VirtPageNum) -> Result<PhysPageNum, MmError> {
        let perm = self.perm();
        let range = self.range();
        if !perm.contains(PteFlags::WRITE) {
            return Err(MmError::PermissionDenied);
        }
        let idx = (vpn - range.start()) as usize;
        let frame_mut = self.frame_mut(idx).unwrap_or_else(|| {
            panic!(
                "unexpected error: unable to find frame for vpn {} at index {}",
                vpn, idx
            )
        });
        let meta = frame_mut.meta();
        // if the frame is shared, allocate a new frame and copy the data.

        // If incrementing/decrementing the count occurs simultaneously with the
        // instructions below, the maximum overhead is merely copying one more page of
        // memory, and no unsafety will result.

        // Shared pages are **read only**
        if meta.is_shared() {
            let mut new_frame = unsafe { alloc_frame().ok_or(MmError::OutOfMemory)? };
            /*kdebugln!(
                "copied shared page from {} to {} for vpn {}",
                frame_mut.ppn(),
                new_frame.ppn(),
                vpn
            );*/
            let new_ppn = new_frame.ppn();
            let dest: *mut u8 = new_frame.ppn().to_phys_addr().to_hhdm().as_ptr_mut();
            let src: *const u8 = frame_mut.ppn().to_phys_addr().to_hhdm().as_ptr();
            unsafe {
                copy(src, dest, PagingArch::PAGE_SIZE_BYTES);
                *frame_mut = new_frame.into_frame_handle();
            }
            Ok(new_ppn)
        } else {
            /* kdebugln!("({}) claimed shared page at {}", current_task_id(), vpn); */
            Ok(frame_mut.ppn())
        }
    }

    /// Map this segment to the given page table mapper with the permissions
    /// specified in `perm`.
    ///
    /// # Safety
    ///  * Rollback is not performed if an error occurs during the mapping
    ///    process.
    unsafe fn map_to(&self, mapper: &mut Mapper, perm: PteFlags) -> Result<(), MmError> {
        let range = self.range();
        for i in 0..range.len() {
            let vpn = range.start() + i as u64;
            if let Some(translated) = mapper.translate(vpn) {
                // segments not aligned to pages
                return Err(MmError::NotAligned);
            } else {
                unsafe {
                    mapper.map_one(
                        vpn,
                        self.frame(i)
                            .unwrap_or_else(|| {
                                panic!(
                                    "unexpected error: unable to find frame for vpn {} at index {}",
                                    vpn, i
                                )
                            })
                            .ppn(),
                        perm | PteFlags::USER | PteFlags::VALID,
                        0,
                        false,
                    )?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Segment {
    range: VirtPageRange,
    perm: PteFlags,
    frames: Box<[FrameHandle]>,
}

impl Segment {
    /// Create a new [`Segment`] covering `range` with permission `perm` and
    /// backing `frames`.
    ///
    /// **The `frames` field is expected to be in the same order as the virtual
    /// pages in `range`.**
    ///
    /// The caller must ensure that `frames.len() == range.len()`.
    pub fn new(range: VirtPageRange, perm: PteFlags, frames: Box<[FrameHandle]>) -> Self {
        debug_assert!(range.len() == frames.len());
        Self {
            range,
            perm,
            frames: frames,
        }
    }
}

impl MemArea for Segment {
    fn range(&self) -> VirtPageRange {
        self.range
    }

    fn perm(&self) -> PteFlags {
        self.perm
    }

    fn frame_mut(&mut self, index: usize) -> Option<&mut FrameHandle> {
        self.frames.get_mut(index)
    }

    fn frame(&self, index: usize) -> Option<&FrameHandle> {
        self.frames.get(index)
    }
}

impl CloneableMemArea for Segment {
    unsafe fn clone(&mut self) -> Self {
        Self {
            range: self.range,
            perm: self.perm,
            frames: self.frames.clone(),
        }
    }
}

/// Growth direction for a memory area.
#[derive(Debug, Clone, Copy)]
pub enum AreaGrowthDirection {
    Higher,
    Lower,
}

#[derive(Debug)]
pub struct DynamicMemArea {
    vpn_range: VirtPageRange,
    frames: VecDeque<FrameHandle>,
    max_pages: usize,
    growth: AreaGrowthDirection,
    perm: PteFlags,
}

impl DynamicMemArea {
    /// The empty memory area constant [EMPTY].
    ///
    /// This is a convenient zero-value used when swapping areas during
    /// adjustments (e.g., in `set_brk`).
    pub const EMPTY: Self = Self {
        vpn_range: VirtPageRange::new(VirtPageNum::new(0), 0),
        frames: VecDeque::new(),
        max_pages: 0,
        growth: AreaGrowthDirection::Higher,
        perm: PteFlags::empty(),
    };

    /// Create a new memory area.
    pub const fn new(
        init_vpn: VirtPageNum,
        max_pages: usize,
        growth: AreaGrowthDirection,
        perm: PteFlags,
    ) -> Result<Self, MmError> {
        let mut memarea = DynamicMemArea {
            vpn_range: VirtPageRange::new(init_vpn, 0),
            frames: VecDeque::new(),
            max_pages,
            growth,
            perm,
        };
        Ok(memarea)
    }

    /// Create a new memory area and preallocate the area with `prealloc` pages.
    pub fn prealloc(
        init_vpn: VirtPageNum,
        max_pages: usize,
        growth: AreaGrowthDirection,
        perm: PteFlags,
        prealloc: usize,
        page_table: &mut PageTable,
    ) -> Result<Self, MmError> {
        if prealloc > max_pages {
            return Err(MmError::InvalidArgument);
        }
        let mut memarea = DynamicMemArea::new(init_vpn, max_pages, growth, perm)?;
        for _ in 0..prealloc {
            memarea.grow(page_table)?;
        }
        Ok(memarea)
    }

    /// Return the maximum number of pages this area can hold.
    pub fn max_pages(&self) -> usize {
        self.max_pages
    }

    /// Grow this memory area by `pages` pages.
    ///
    /// If allocation fails, any pages mapped during this call are rolled
    /// back before returning the error.
    pub fn try_grow(&mut self, page_table: &mut PageTable, pages: usize) -> Result<(), MmError> {
        let mut grown = self.pages();
        for _ in 0..pages {
            match self.grow(page_table) {
                Ok(()) => {
                    grown += 1;
                },
                Err(e) => {
                    kinfoln!(
                        "out of memory when growing mem area, unmapping {} pages",
                        grown
                    );
                    let mut mapper = page_table.mapper();
                    unsafe {
                        mapper.try_unmap(Unmapping {
                            range: VirtPageRange::new(
                                self.vpn_range.end() - grown as u64,
                                grown as u64,
                            ),
                        }); // unmap the newly mapped pages
                        while grown > 0 {
                            self.shrink();
                            grown -= 1;
                        }
                    }
                    return Err(e);
                },
            }
        }
        Ok(())
    }

    /// Grow the area by one page.
    pub fn grow(&mut self, page_table: &mut PageTable) -> Result<(), MmError> {
        if self.pages() + 1 > self.max_pages {
            return Err(MmError::OutOfMemory);
        }
        let frame = unsafe {
            alloc_frame()
                .ok_or(MmError::OutOfMemory)?
                .into_frame_handle()
        };
        let mut mapper = page_table.mapper();
        let flags = self.perm | PteFlags::USER | PteFlags::VALID;
        match self.growth {
            AreaGrowthDirection::Lower => unsafe {
                mapper.map_one(self.vpn_range.start() - 1, frame.ppn(), flags, 0, false)?;
                self.vpn_range = VirtPageRange::new(
                    self.vpn_range.start() - 1,
                    (self.vpn_range.len() + 1) as u64,
                )
            },
            AreaGrowthDirection::Higher => unsafe {
                mapper.map_one(self.vpn_range.end(), frame.ppn(), flags, 0, false)?;
                self.vpn_range =
                    VirtPageRange::new(self.vpn_range.start(), (self.vpn_range.len() + 1) as u64)
            },
        }
        match self.growth {
            AreaGrowthDirection::Higher => self.frames.push_back(frame),
            AreaGrowthDirection::Lower => self.frames.push_front(frame),
        }
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
        let _ = match self.growth {
            AreaGrowthDirection::Higher => self.frames.pop_back(),
            AreaGrowthDirection::Lower => self.frames.pop_front(),
        };
        true
    }
}

impl MemArea for DynamicMemArea {
    fn range(&self) -> VirtPageRange {
        self.vpn_range
    }

    fn perm(&self) -> PteFlags {
        self.perm
    }

    fn frame_mut(&mut self, index: usize) -> Option<&mut FrameHandle> {
        self.frames.get_mut(index)
    }

    fn frame(&self, index: usize) -> Option<&FrameHandle> {
        self.frames.get(index)
    }
}

impl CloneableMemArea for DynamicMemArea {
    unsafe fn clone(&mut self) -> Self {
        Self {
            vpn_range: self.vpn_range,
            frames: self.frames.clone(),
            max_pages: self.max_pages,
            growth: self.growth,
            perm: self.perm,
        }
    }
}

impl UserSpace {
    /// Create a new, empty [UserSpace] with a fresh page table and
    /// uninitialized user areas (stack/heap are empty).
    ///
    /// See [Self::new_user] for creating a [UserSpace] prepopulated with kernel
    /// mappings and an allocated initial stack.
    pub fn new_empty() -> Result<Self, MmError> {
        let table = PageTable::new()?;
        Ok(Self {
            table_ppn: table.root_ppn(),
            data: RwLock::new(UserSpaceData {
                table,
                ustack: DynamicMemArea::new(
                    KernelLayout::USPACE_TOP_VPN,
                    MAX_USER_STACK_PAGES as usize,
                    AreaGrowthDirection::Lower,
                    PteFlags::READ | PteFlags::WRITE,
                )
                .unwrap(),
                uinit_sp: KernelLayout::USPACE_TOP_VPN.to_virt_addr(),
                uheap: DynamicMemArea::new(
                    VirtPageNum::new(0),
                    MAX_HEAP_PAGES as usize,
                    AreaGrowthDirection::Higher,
                    PteFlags::READ | PteFlags::WRITE,
                )
                .unwrap(),
                ubrk: VirtPageNum::new(0).to_virt_addr(),
                segs: vec![],
            }),
        })
    }

    /// Create a new [UserSpace] prepared for running a user process.
    ///
    /// This will copy kernel mappings into the new page table and preallocate
    /// the user stack to [INIT_USER_STACK_PAGES]. The heap will be left for
    /// initialization after the user image is loaded.
    pub fn new_user() -> Result<Self, MmError> {
        let mut table = PageTable::new()?;
        KERNEL_PTABLE.copy_to_ptable(&mut table);
        let stack = DynamicMemArea::prealloc(
            KernelLayout::USPACE_TOP_VPN,
            MAX_USER_STACK_PAGES as usize,
            AreaGrowthDirection::Lower,
            PteFlags::READ | PteFlags::WRITE,
            INIT_USER_STACK_PAGES as usize,
            &mut table,
        )?;

        // heap will be initialized after loading the user image
        let heap = DynamicMemArea::new(
            VirtPageNum::new(0),
            MAX_HEAP_PAGES as usize,
            AreaGrowthDirection::Higher,
            PteFlags::READ | PteFlags::WRITE,
        )?;
        Ok(UserSpace {
            table_ppn: table.root_ppn(),
            data: RwLock::new(UserSpaceData {
                table,
                uinit_sp: KernelLayout::USPACE_TOP_VPN.to_virt_addr(),
                ustack: stack,
                uheap: heap,
                ubrk: VirtPageNum::new(0).to_virt_addr(),
                segs: vec![],
            }),
        })
    }

    pub fn read(&self) -> ReadNoPreemptGuard<'_, UserSpaceData> {
        self.data.read()
    }

    pub fn write(&self) -> WriteNoPreemptGuard<'_, UserSpaceData> {
        self.data.write()
    }

    pub fn create_copy(&self) -> Result<Self, MmError> {
        let data = self.write().create_copy()?;
        Ok(UserSpace {
            table_ppn: data.table.root_ppn(),
            data: RwLock::new(data),
        })
    }

    pub fn activate(&self) {
        self.read().activate();
    }
}

impl Drop for UserSpace {
    fn drop(&mut self) {
        kdebugln!(
            "dropping user space with root page at ppn {}",
            self.table_ppn
        );
    }
}

impl UserSpaceData {
    pub fn iter_areas<F, R>(&self, f: F) -> Option<R>
    where
        F: Fn(&dyn MemArea) -> ControlFlow<R>,
    {
        for area in core::iter::empty()
            .chain(self.segs.iter().map(|s| s as &dyn MemArea))
            .chain(core::iter::once(&self.ustack as &dyn MemArea))
            .chain(core::iter::once(&self.uheap as &dyn MemArea))
        {
            match f(area) {
                ControlFlow::Continue(()) => {},
                ControlFlow::Break(r) => return Some(r),
            }
        }
        None
    }

    pub fn iter_areas_mut<F, R>(&mut self, mut f: F) -> Option<R>
    where
        F: FnMut(&mut dyn MemArea) -> ControlFlow<R>,
    {
        for area in core::iter::empty()
            .chain(self.segs.iter_mut().map(|s| s as &mut dyn MemArea))
            .chain(core::iter::once(&mut self.ustack as &mut dyn MemArea))
            .chain(core::iter::once(&mut self.uheap as &mut dyn MemArea))
        {
            match f(area) {
                ControlFlow::Continue(()) => {},
                ControlFlow::Break(r) => return Some(r),
            }
        }
        None
    }

    /// Add a memory segment to the user space and fill the segment with the
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
    ///  * **Calling this after the heap area is initialized will lead to
    ///    panic.**
    pub unsafe fn add_segment<TErr: Debug + From<MmError>>(
        &mut self,
        vaddr: VirtAddr,
        vsize: usize,
        psize: usize,
        source: &impl DataSource<TError = impl Into<TErr>>,
        rwx_flags: PteFlags,
    ) -> Result<(), TErr> {
        assert!(self.uheap.vpn_range.len() == 0);
        let vaddr_ed = vaddr + vsize as u64;
        let vpn_st = vaddr.page_down();
        let vpn_ed = vaddr_ed.page_up();
        let len = vpn_ed - vpn_st;
        if vpn_ed > self.uheap.vpn_range.start() {
            self.uheap.vpn_range = VirtPageRange::new(vpn_ed, 0u64);
            self.ubrk = vpn_ed.to_virt_addr();
        }
        let mut mapper = self.table.mapper();
        let mut frames = (0..len)
            .map(|_| {
                alloc_frame()
                    .and_then(|owned| unsafe { Some(owned.into_frame_handle()) })
                    .ok_or(MmError::OutOfMemory)
            })
            .collect::<Result<Vec<_>, MmError>>()?
            .into_boxed_slice();
        let seg = Segment::new(VirtPageRange::new(vpn_st, len), rwx_flags, frames);
        unsafe {
            seg.map_to(&mut mapper, rwx_flags)?;
            mapper.fill_data(vaddr, source, psize as u64)?;
        }
        if vsize > psize {
            unsafe {
                mapper.fill_data(
                    vaddr + psize as u64,
                    &ZeroDataSource::<MmError>::new(),
                    (vsize - psize) as u64,
                )?;
            }
        }
        self.segs.push(seg);
        Ok(())
    }

    /// Get the program break
    pub fn brk(&self) -> VirtAddr {
        self.ubrk
    }

    /// Adjust the program break for this address space.
    ///
    /// This function grows or shrinks the heap tracked in `uheap` to make
    /// `brk` the new program break. It returns an error if the requested
    /// break is out of range or if allocation fails while growing.
    pub fn set_brk(&mut self, brk: VirtAddr) -> Result<(), MmError> {
        if brk < self.uheap.vpn_range.start().to_virt_addr() {
            return Err(MmError::OutOfMemory); // see reference https://www.man7.org/linux/man-pages/man2/brk.2.html
        }
        if brk
            > self.uheap.vpn_range.start().to_virt_addr()
                + (MAX_HEAP_PAGES << PagingArch::PAGE_SIZE_BITS) as u64
        {
            return Err(MmError::OutOfMemory);
        }
        let new_brk_vpn = brk.page_up();
        if self.uheap.vpn_range.end() > new_brk_vpn {
            // shrink heap
            let mut count = self.uheap.vpn_range.end() - new_brk_vpn;
            let mut mapper = self.table.mapper();
            unsafe {
                mapper.try_unmap(Unmapping {
                    range: VirtPageRange::new(
                        self.uheap.vpn_range.end() - count as u64,
                        count as u64,
                    ),
                }); // unmap the newly mapped pages
                while count > 0 {
                    self.uheap.shrink();
                    count -= 1;
                }
            }
        } else if new_brk_vpn > self.uheap.vpn_range.end() {
            let mut grown: usize = 0;
            // grow heap
            let cur_heapend_vpn = self.uheap.vpn_range.end().to_virt_addr().page_up();
            let count = (new_brk_vpn - cur_heapend_vpn) as usize;
            let res = self.uheap.try_grow(&mut self.table, count);
            res?;
        }
        self.ubrk = brk;
        kinfoln!("brk of {} set to {}", current_task_id(), brk);
        Ok(())
    }

    /// Push data onto the user init stack and return a pointer to the copied
    /// data on the stack.
    ///
    /// Returns [MmError::ArgumentTooLarge] if the task init stack is not large
    /// enough to hold the data.
    ///
    /// ## Safety
    /// **Invoke this function when the stack is in use may lead to undefined
    /// behavior**
    pub unsafe fn push_to_init_stack<A: Sized>(
        &mut self,
        data: &[u8],
    ) -> Result<VirtAddr, MmError> {
        let align = align_of::<A>() as u64;
        let mut stack_area_top = self.ustack.vpn_range.end().to_virt_addr().get();
        let mut sp = self.uinit_sp.get() - data.len() as u64;
        sp = align_down!(sp, align) as u64;
        if sp < self.ustack.vpn_range.start().to_virt_addr().get() {
            return Err(MmError::ArgumentTooLarge);
        }
        let mut mapper = self.table.mapper();
        unsafe {
            mapper
                .fill_data::<MmError>(
                    VirtAddr::new(sp),
                    &SliceDataSource::new(data),
                    data.len() as u64,
                )
                .expect("stack push should not fail after ensuring the stack is large enough");
        }
        drop(mapper);
        let sp = VirtAddr::new(sp);
        self.uinit_sp = sp;
        Ok(sp)
    }

    /// Move the init stack pointer to the top of the user stack.
    ///
    /// This will not deallocate the old stack
    ///
    /// ## Safety
    /// **Invoke this function when the stack is in use may lead to undefined
    /// behavior**
    pub unsafe fn clear_stack(&mut self) {
        self.uinit_sp = self.ustack.vpn_range.end().to_virt_addr();
    }

    /// Get the maximum user stack space for this address space.
    ///
    /// It's a fixed range.
    pub fn max_stack_space(&self) -> VirtPageRange {
        self.ustack.vpn_range
    }

    /// Return a read guard for inspecting this address space's page table.
    /// The guard dereferences to a [PageTable].
    pub fn page_table(&self) -> &PageTable {
        &self.table
    }

    /// Return a write guard for mutating this address space's page table.
    /// The guard allows safe mutation of the underlying [PageTable].
    pub fn page_table_mut(&mut self) -> &mut PageTable {
        &mut self.table
    }

    /// Make this [UserSpace] active on the CPU so address translation uses
    /// its page table. This calls architecture-specific helpers to load the
    /// root page-table.
    pub fn activate(&self) {
        unsafe {
            PagingArch::activate_addr_space(&self.table);
        }
    }
    /// Create a copy of this [UserSpace] with copy-on-write semantics.
    ///
    /// # Notes
    /// If the operation fails, pages that have already been converted to
    /// read-only will not be rolled back, but will be restored during a later
    /// page fault.
    pub fn create_copy(&mut self) -> Result<Self, MmError> {
        // old fields
        let uinit_sp = self.uinit_sp;
        let ubrk = self.ubrk;
        let ustack = &mut self.ustack;
        let table = &mut self.table;
        let mut mapper = table.mapper();
        let uheap = &mut self.uheap;
        let segs = &mut self.segs;
        // new fields
        let mut new_segs = vec![];
        let mut new_table = PageTable::new()?;
        KERNEL_PTABLE.copy_to_ptable(&mut new_table);
        let mut new_mapper = new_table.mapper();
        for seg in segs {
            new_segs.push(unsafe { seg.create_copy(&mut mapper, &mut new_mapper)? });
        }
        let mut new_ustack = unsafe { ustack.create_copy(&mut mapper, &mut new_mapper)? };
        let mut new_uheap = unsafe { uheap.create_copy(&mut mapper, &mut new_mapper)? };
        let new_ppn = new_table.root_ppn();
        // return the created value
        let new_inner = UserSpaceData {
            table: new_table,
            ustack: new_ustack,
            uinit_sp: uinit_sp,
            uheap: new_uheap,
            ubrk: ubrk,
            segs: new_segs,
        };
        Ok(new_inner)
    }

    pub fn copy_on_write(&mut self, vaddr: VirtAddr) -> Result<(), MmError> {
        let vpn = vaddr.page_down();
        let mut new_ppn = None;
        self.iter_areas_mut(|area| {
            if area.range().contains(vpn) {
                unsafe {
                    new_ppn = Some(area.copy_on_write(vpn));
                }
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });
        if let Some(ppn) = new_ppn {
            let ppn = ppn?;
            let mut mapper = self.table.mapper();
            match mapper.translate(vpn) {
                Some(translated) => unsafe {
                    let new_flags = translated.flags | PteFlags::WRITE;
                    mapper.map_one(vpn, ppn, new_flags, 0, true)?;
                },
                None => {
                    kcritln!("unexpected inconsistency: vpn within area range but not mapped");
                    return Err(MmError::NotMapped);
                },
            };
            Ok(())
        } else {
            Err(MmError::NotMapped)
        }
    }

    pub fn check_permission(&self, vpn: VirtPageNum, rwx_flags: PteFlags) -> Result<(), MmError> {
        let perm = self.iter_areas(|area| {
            if area.range().contains(vpn) {
                ControlFlow::Break(area.perm())
            } else {
                ControlFlow::Continue(())
            }
        });
        if let Some(perm) = perm {
            if perm.contains(rwx_flags) {
                Ok(())
            } else {
                Err(MmError::PermissionDenied)
            }
        } else {
            Err(MmError::NotMapped)
        }
    }
}

impl PartialEq for UserSpace {
    fn eq(&self, other: &Self) -> bool {
        self.table_ppn == other.table_ppn
    }
}

impl Eq for UserSpace {}

/// Read guard for accessing the user-space page table.
///
/// The guard dereferences to a [`PageTable`] allowing safe read-only access
/// to the user page table while preventing preemption.
pub struct USpacePTableReadGuard<'a> {
    guard: ReadNoPreemptGuard<'a, UserSpaceData>,
}

impl Deref for USpacePTableReadGuard<'_> {
    type Target = PageTable;

    fn deref(&self) -> &Self::Target {
        &self.guard.table
    }
}

impl<'a> USpacePTableReadGuard<'a> {
    /// Create a new read guard for `uspace`.
    pub fn new(uspace: &'a UserSpace) -> Self {
        Self {
            guard: uspace.data.read(),
        }
    }
}

/// Write guard for mutating the user-space page table.
pub struct USpacePTableWriteGuard<'a> {
    guard: WriteNoPreemptGuard<'a, UserSpaceData>,
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
    /// Create a new write guard for `uspace`.
    pub fn new(uspace: &'a UserSpace) -> Self {
        Self {
            guard: uspace.data.write(),
        }
    }
}
