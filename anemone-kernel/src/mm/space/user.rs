use range_allocator::Rangable;

use crate::{mm::layout::KernelLayoutTrait, prelude::*};

pub const MAX_USER_STACK_PAGES: usize = 2048; // 8 MiB if page size is 4 KiB
pub const INIT_USER_STACK_PAGES: usize = 16; // 64 KiB if page size is 4 KiB
pub const MAX_USER_HEAP_PAGES: usize = 128 * 1024 * 256; // 128 GiB if page size is 4 KiB
pub const INIT_USER_HEAP_PAGES: usize = 16; // 64 KiB

#[derive(Debug)]
pub struct UserSpace {
    stack: MemArea,
    heap: MemArea,
    seg_frames: Vec<FrameHandle>,
}

impl UserSpace {
    pub fn new(heap_start: VirtPageNum, page_table: &mut PageTable) -> Result<Self, MmError> {
        let stack = MemArea::prealloc(
            KernelLayout::USPACE_TOP_VPN,
            MAX_USER_STACK_PAGES,
            AreaGrowthDirection::Lower,
            PteFlags::READ | PteFlags::WRITE | PteFlags::USER | PteFlags::VALID,
            INIT_USER_STACK_PAGES,
            page_table,
        )?;

        // heap will be initialized after loading the user image
        let heap = MemArea::prealloc(
            heap_start,
            MAX_USER_HEAP_PAGES,
            AreaGrowthDirection::Higher,
            PteFlags::READ | PteFlags::WRITE | PteFlags::USER | PteFlags::VALID,
            INIT_USER_HEAP_PAGES,
            page_table,
        )?;
        Ok(Self {
            stack,
            heap,
            seg_frames: vec![],
        })
    }

    /// Add a memory segment to the user space, and fill the segment with the
    /// given data.
    ///
    /// # Safety
    /// This function is unsafe because:
    ///  * **any already mapped page tables will not be rolled back if an
    ///    exception is encountered during the page table mapping process.**
    ///  * This function does not validate address range conflicts with existing
    ///    mappings, potentially causing code/data overwrites.
    pub unsafe fn add_segment(
        &mut self,
        vaddr: VirtAddr,
        vsize: usize,
        psize: usize,
        source: &[u8],
        rwx_flags: PteFlags,
        table: &mut PageTable,
    ) -> Result<(), MmError> {
        let vaddr_ed = vaddr + vsize as u64;
        let vpn_st = vaddr.page_down();
        let vpn_ed = vaddr_ed.page_up();
        let mut mapper = table.mapper();
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
                    self.seg_frames.push(frame);
                }
            }
        }
        unsafe { mapper.fill_data(vaddr, source, psize as u64)? }
        // TODO: fill 0
        Ok(())
    }
}

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
    /// Create a new memory area
    pub fn new(
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
    pub fn shrink(&mut self) -> bool {
        if self.pages() == 0 {
            return false;
        }
        match self.growth {
            AreaGrowthDirection::Lower => {
                self.vpn_range = VirtPageRange::new(
                    self.vpn_range.start() + 1 as u64,
                    (self.vpn_range.len() - 1) as u64,
                )
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
