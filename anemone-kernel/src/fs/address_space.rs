use crate::{
    fs::{
        UserBufferSink, UserBufferSource,
        cache_stats::{backing_file_cache_page_inserted, backing_file_cache_pages_removed},
    },
    prelude::{
        vmo::{ResolvedFrame, VmObject},
        *,
    },
};

/// The page-local persistence capability consumed by an inode address space.
///
/// Implementations own only filesystem identity and transaction access. Page
/// publication, dirty state, accounting, and logical size remain owned by the
/// address space and inode.
pub(in crate::fs) trait AddressSpaceBackend: Send + Sync {
    fn fill_page(&self, offset: usize, frame: &mut [u8]) -> Result<(), SysError>;

    fn writeback_page(&self, offset: usize, data: &[u8]) -> Result<(), SysError>;
}

#[derive(Clone)]
struct ResidentPage {
    frame: FrameHandle,
    dirty: bool,
}

/// Resident pages associated with one regular inode.
///
/// `size` is the inode-owned logical-size cell, not a cached projection. The
/// address space may read it for admission and writeback but never mutates it;
/// file operations commit size through the inode owner after content work
/// succeeds.
pub(in crate::fs) struct AddressSpace {
    size: Arc<AtomicU64>,
    backend: Option<Arc<dyn AddressSpaceBackend>>,
    pages: RwLock<BTreeMap<usize, ResidentPage>>,
}

impl AddressSpace {
    pub(in crate::fs) fn new_volatile(size: Arc<AtomicU64>) -> Arc<Self> {
        Arc::new(Self {
            size,
            backend: None,
            pages: RwLock::new(BTreeMap::new()),
        })
    }

    pub(in crate::fs) fn new_backed(
        size: Arc<AtomicU64>,
        backend: Arc<dyn AddressSpaceBackend>,
    ) -> Arc<Self> {
        Arc::new(Self {
            size,
            backend: Some(backend),
            pages: RwLock::new(BTreeMap::new()),
        })
    }

    fn size(&self) -> Result<usize, SysError> {
        usize::try_from(self.size.load(Ordering::Acquire)).map_err(|_| SysError::FileTooLarge)
    }

    fn page_start(pidx: usize) -> Result<usize, SysError> {
        pidx.checked_mul(PagingArch::PAGE_SIZE_BYTES)
            .ok_or(SysError::InvalidArgument)
    }

    fn page_index(offset: usize) -> usize {
        offset >> PagingArch::PAGE_SIZE_BITS
    }

    fn publish(&self, pidx: usize, page: ResidentPage) -> ResidentPage {
        let mut pages = self.pages.write();
        if let Some(existing) = pages.get(&pidx) {
            return existing.clone();
        }

        pages.insert(pidx, page.clone());
        if self.backend.is_some() {
            backing_file_cache_page_inserted();
        }
        page
    }

    fn allocate_page(&self, pidx: usize) -> Result<ResidentPage, SysError> {
        if let Some(page) = self.pages.read().get(&pidx) {
            return Ok(page.clone());
        }

        let page = ResidentPage {
            frame: unsafe {
                alloc_frame_zeroed()
                    .ok_or(SysError::OutOfMemory)?
                    .into_frame_handle()
            },
            dirty: false,
        };
        Ok(self.publish(pidx, page))
    }

    fn load_page(&self, pidx: usize) -> Result<ResidentPage, SysError> {
        let offset = Self::page_start(pidx)?;
        if offset >= self.size()? {
            return Err(SysError::NotMapped);
        }

        if let Some(page) = self.pages.read().get(&pidx) {
            return Ok(page.clone());
        }

        let Some(backend) = &self.backend else {
            return self.allocate_page(pidx);
        };

        // Fill outside the page-map lock. Concurrent fills may duplicate I/O,
        // but publication below selects exactly one resident frame.
        let mut frame = alloc_frame_zeroed().ok_or(SysError::OutOfMemory)?;
        backend.fill_page(offset, frame.as_bytes_mut())?;
        let page = ResidentPage {
            frame: unsafe { frame.into_frame_handle() },
            dirty: false,
        };
        Ok(self.publish(pidx, page))
    }

    fn page_for_write(
        &self,
        pidx: usize,
        preserve_existing: bool,
    ) -> Result<ResidentPage, SysError> {
        if preserve_existing && self.backend.is_some() {
            self.load_page(pidx)
        } else {
            self.allocate_page(pidx)
        }
    }

    fn mark_dirty(&self, pidx: usize) {
        if self.backend.is_none() {
            return;
        }
        self.pages
            .write()
            .get_mut(&pidx)
            .expect("written address-space page must remain resident")
            .dirty = true;
    }

    pub(in crate::fs) fn read(&self, offset: usize, buffer: &mut [u8]) -> Result<(), SysError> {
        let mut remaining = buffer;
        let mut cur_offset = offset;

        while !remaining.is_empty() {
            let pidx = Self::page_index(cur_offset);
            let page_offset = cur_offset & (PagingArch::PAGE_SIZE_BYTES - 1);
            let copy_len = remaining
                .len()
                .min(PagingArch::PAGE_SIZE_BYTES - page_offset);

            let page = if self.backend.is_some() {
                Some(self.load_page(pidx)?)
            } else {
                self.pages.read().get(&pidx).cloned()
            };
            if let Some(page) = page {
                remaining[..copy_len]
                    .copy_from_slice(&page.frame.as_bytes()[page_offset..page_offset + copy_len]);
            } else {
                remaining[..copy_len].fill(0);
            }

            remaining = &mut remaining[copy_len..];
            cur_offset = cur_offset
                .checked_add(copy_len)
                .ok_or(SysError::InvalidArgument)?;
        }
        Ok(())
    }

    pub(in crate::fs) fn read_user(
        &self,
        offset: usize,
        len: usize,
        dst: &mut UserBufferSink<'_>,
    ) -> Result<(), SysError> {
        let mut remaining = len;
        let mut cur_offset = offset;

        while remaining > 0 {
            let pidx = Self::page_index(cur_offset);
            let page_offset = cur_offset & (PagingArch::PAGE_SIZE_BYTES - 1);
            let copy_len = remaining.min(PagingArch::PAGE_SIZE_BYTES - page_offset);
            let page = if self.backend.is_some() {
                Some(self.load_page(pidx)?)
            } else {
                self.pages.read().get(&pidx).cloned()
            };

            // The frame clone is stable after all page-map and backend locks
            // have been released; user copy must never run under either lock.
            let copied = if let Some(page) = page {
                dst.write_from_slice(&page.frame.as_bytes()[page_offset..page_offset + copy_len])?
            } else {
                dst.write_zeros(copy_len)?
            };
            if copied < copy_len {
                return Ok(());
            }

            remaining -= copy_len;
            cur_offset = cur_offset
                .checked_add(copy_len)
                .ok_or(SysError::InvalidArgument)?;
        }
        Ok(())
    }

    pub(in crate::fs) fn write(&self, offset: usize, data: &[u8]) -> Result<usize, SysError> {
        let old_size = self.size()?;
        let mut remaining = data;
        let mut cur_offset = offset;
        while !remaining.is_empty() {
            let pidx = Self::page_index(cur_offset);
            let page_offset = cur_offset & (PagingArch::PAGE_SIZE_BYTES - 1);
            let copy_len = remaining
                .len()
                .min(PagingArch::PAGE_SIZE_BYTES - page_offset);
            let page_start = Self::page_start(pidx)?;
            let preserve_existing = (page_offset != 0 || copy_len != PagingArch::PAGE_SIZE_BYTES)
                && page_start < old_size;
            let page = self.page_for_write(pidx, preserve_existing)?;
            let dst = unsafe {
                core::slice::from_raw_parts_mut(
                    page.frame.ppn().to_phys_addr().to_hhdm().as_ptr_mut(),
                    PagingArch::PAGE_SIZE_BYTES,
                )
            };
            dst[page_offset..page_offset + copy_len].copy_from_slice(&remaining[..copy_len]);
            self.mark_dirty(pidx);

            remaining = &remaining[copy_len..];
            cur_offset = cur_offset
                .checked_add(copy_len)
                .ok_or(SysError::InvalidArgument)?;
        }
        offset
            .checked_add(data.len())
            .ok_or(SysError::InvalidArgument)
    }

    pub(in crate::fs) fn write_user(
        &self,
        offset: usize,
        len: usize,
        src: &mut UserBufferSource<'_>,
    ) -> Result<usize, SysError> {
        let old_size = self.size()?;
        let mut written = 0usize;
        let mut remaining = len;
        let mut cur_offset = offset;

        while remaining > 0 {
            let pidx = Self::page_index(cur_offset);
            let page_offset = cur_offset & (PagingArch::PAGE_SIZE_BYTES - 1);
            let copy_len = remaining.min(PagingArch::PAGE_SIZE_BYTES - page_offset);
            let page_start = Self::page_start(pidx)?;
            // User copy may fault after a prefix of a planned full-page write,
            // so any persistent page overlapping the old file must be filled
            // before exposing its frame to the copy.
            let preserve_existing = page_start < old_size;
            let page = match self.page_for_write(pidx, preserve_existing) {
                Ok(page) => page,
                Err(_) if written > 0 => break,
                Err(err) => return Err(err),
            };
            let dst = unsafe {
                core::slice::from_raw_parts_mut(
                    page.frame.ppn().to_phys_addr().to_hhdm().as_ptr_mut(),
                    PagingArch::PAGE_SIZE_BYTES,
                )
            };
            let copied = match src.copy_into_slice(&mut dst[page_offset..page_offset + copy_len]) {
                Ok(copied) => copied,
                Err(_) if written > 0 => break,
                Err(err) => return Err(err),
            };
            if copied == 0 {
                break;
            }
            self.mark_dirty(pidx);

            written = written
                .checked_add(copied)
                .ok_or(SysError::InvalidArgument)?;
            cur_offset = cur_offset
                .checked_add(copied)
                .ok_or(SysError::InvalidArgument)?;
            if copied < copy_len {
                break;
            }
            remaining -= copied;
        }
        Ok(written)
    }

    pub(in crate::fs) fn sync_page(&self, pidx: usize) -> Result<(), SysError> {
        let Some(backend) = &self.backend else {
            return Ok(());
        };
        let size = self.size()?;
        let offset = Self::page_start(pidx)?;
        if offset >= size {
            return Err(SysError::NotMapped);
        }
        let page = self.pages.read().get(&pidx).cloned();
        if let Some(page) = page.filter(|page| page.dirty) {
            let valid_len = (size - offset).min(PagingArch::PAGE_SIZE_BYTES);
            backend.writeback_page(offset, &page.frame.as_bytes()[..valid_len])?;
            // Dirty is intentionally sticky: writable PTE stores are not yet
            // tracked well enough to prove that a successful writeback stayed
            // clean. Eviction is the only current dirty retirement point.
        }
        Ok(())
    }

    pub(in crate::fs) fn sync_all(&self) -> Result<(), SysError> {
        let dirty_pages = self
            .pages
            .read()
            .iter()
            .filter_map(|(pidx, page)| page.dirty.then_some(*pidx))
            .collect::<Vec<_>>();
        for pidx in dirty_pages {
            self.sync_page(pidx)?;
        }
        Ok(())
    }

    pub(in crate::fs) fn apply_volatile_truncate(&self, old_size: usize, new_size: usize) {
        assert!(
            self.backend.is_none(),
            "persistent address space used volatile truncate"
        );
        let mut pages = self.pages.write();
        if new_size < old_size {
            let keep_pages = if new_size == 0 {
                0
            } else {
                Self::page_index(new_size - 1) + 1
            };
            pages.retain(|pidx, _| *pidx < keep_pages);
            Self::zero_tail(&pages, new_size);
        } else if new_size > old_size {
            Self::zero_tail(&pages, old_size);
        }
    }

    fn zero_tail(pages: &BTreeMap<usize, ResidentPage>, start: usize) {
        let tail_offset = start & (PagingArch::PAGE_SIZE_BYTES - 1);
        if tail_offset == 0 {
            return;
        }
        if let Some(page) = pages.get(&Self::page_index(start)) {
            unsafe {
                core::ptr::write_bytes(
                    page.frame
                        .ppn()
                        .to_phys_addr()
                        .to_hhdm()
                        .as_ptr_mut::<u8>()
                        .add(tail_offset),
                    0,
                    PagingArch::PAGE_SIZE_BYTES - tail_offset,
                );
            }
        }
    }

    pub(in crate::fs) fn invalidate_size_delta(&self, old_size: usize, new_size: usize) {
        assert!(
            self.backend.is_some(),
            "volatile address space used backed invalidation"
        );
        if new_size == old_size {
            return;
        }
        let (first, last) = if new_size < old_size {
            (Self::page_index(new_size), Self::page_index(old_size - 1))
        } else {
            (Self::page_index(old_size), Self::page_index(new_size - 1))
        };
        let mut pages = self.pages.write();
        let old_len = pages.len();
        pages.retain(|pidx, _| *pidx < first || *pidx > last);
        backing_file_cache_pages_removed(old_len - pages.len());
    }
}

impl VmObject for AddressSpace {
    fn resolve_frame(&self, pidx: usize, access: PageFaultType) -> Result<ResolvedFrame, SysError> {
        let page = self.load_page(pidx)?;
        if matches!(access, PageFaultType::Write) {
            self.mark_dirty(pidx);
        }
        // A backed page becomes writable only after the write fault above has
        // published sticky dirty state. Subsequent PTE stores need no further
        // faults because the page remains dirty until eviction.
        Ok(ResolvedFrame {
            frame: page.frame,
            writable: self.backend.is_none() || matches!(access, PageFaultType::Write),
        })
    }

    fn sync_range(&self, range: core::ops::Range<usize>) -> Result<(), SysError> {
        for pidx in range {
            self.sync_page(pidx)?;
        }
        Ok(())
    }
}

impl Drop for AddressSpace {
    fn drop(&mut self) {
        if self.backend.is_some() {
            backing_file_cache_pages_removed(self.pages.read().len());
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    struct FakeBackend {
        fills: AtomicUsize,
        writebacks: AtomicUsize,
        fail_writeback: AtomicBool,
    }

    impl FakeBackend {
        fn new() -> Self {
            Self {
                fills: AtomicUsize::new(0),
                writebacks: AtomicUsize::new(0),
                fail_writeback: AtomicBool::new(false),
            }
        }
    }

    impl AddressSpaceBackend for FakeBackend {
        fn fill_page(&self, offset: usize, frame: &mut [u8]) -> Result<(), SysError> {
            self.fills.fetch_add(1, Ordering::Relaxed);
            for (index, byte) in frame.iter_mut().enumerate() {
                *byte = offset.wrapping_add(index) as u8;
            }
            Ok(())
        }

        fn writeback_page(&self, _offset: usize, _data: &[u8]) -> Result<(), SysError> {
            self.writebacks.fetch_add(1, Ordering::Relaxed);
            if self.fail_writeback.load(Ordering::Relaxed) {
                Err(SysError::InvalidArgument)
            } else {
                Ok(())
            }
        }
    }

    #[kunit]
    fn volatile_holes_cross_page_writes_and_truncate_regrow_are_zero_correct() {
        let page_size = PagingArch::PAGE_SIZE_BYTES;
        let size = Arc::new(AtomicU64::new((page_size * 2) as u64));
        let address_space = AddressSpace::new_volatile(size.clone());

        let mut hole = [1u8; 8];
        address_space.read(32, &mut hole).unwrap();
        assert_eq!(hole, [0; 8]);

        address_space.write(page_size - 2, b"abcd").unwrap();
        let mut cross_page = [0u8; 8];
        address_space.read(page_size - 4, &mut cross_page).unwrap();
        assert_eq!(&cross_page, b"\0\0abcd\0\0");

        address_space.apply_volatile_truncate(page_size * 2, page_size - 1);
        size.store((page_size - 1) as u64, Ordering::Release);
        address_space.apply_volatile_truncate(page_size - 1, page_size + 4);
        size.store((page_size + 4) as u64, Ordering::Release);

        let mut regrown = [1u8; 7];
        address_space.read(page_size - 3, &mut regrown).unwrap();
        assert_eq!(regrown, [0, b'a', 0, 0, 0, 0, 0]);
    }

    #[kunit]
    fn backed_fill_cache_sticky_dirty_failure_and_accounting_share_one_owner() {
        let before = crate::fs::resident_file_inode_cache_pages();
        let size = Arc::new(AtomicU64::new(PagingArch::PAGE_SIZE_BYTES as u64));
        let backend = Arc::new(FakeBackend::new());
        let address_space = AddressSpace::new_backed(size, backend.clone());

        let first = address_space.resolve_frame(0, PageFaultType::Read).unwrap();
        assert_eq!(first.frame.as_bytes()[17], 17);
        address_space.resolve_frame(0, PageFaultType::Read).unwrap();
        assert_eq!(backend.fills.load(Ordering::Relaxed), 1);
        assert_eq!(crate::fs::resident_file_inode_cache_pages(), before + 1);

        address_space
            .resolve_frame(0, PageFaultType::Write)
            .unwrap();
        backend.fail_writeback.store(true, Ordering::Relaxed);
        assert_eq!(
            address_space.sync_page(0).unwrap_err(),
            SysError::InvalidArgument
        );
        assert!(address_space.pages.read().get(&0).unwrap().dirty);

        backend.fail_writeback.store(false, Ordering::Relaxed);
        address_space.sync_page(0).unwrap();
        assert!(address_space.pages.read().get(&0).unwrap().dirty);
        assert_eq!(backend.writebacks.load(Ordering::Relaxed), 2);

        drop(address_space);
        assert_eq!(crate::fs::resident_file_inode_cache_pages(), before);
    }

    #[kunit]
    fn backed_resolution_arms_first_write_before_publishing_writable_frame() {
        let size = Arc::new(AtomicU64::new(PagingArch::PAGE_SIZE_BYTES as u64));
        let backend = Arc::new(FakeBackend::new());
        let address_space = AddressSpace::new_backed(size, backend);

        let read = address_space.resolve_frame(0, PageFaultType::Read).unwrap();
        assert!(!read.writable);
        assert!(!address_space.pages.read().get(&0).unwrap().dirty);

        let write = address_space
            .resolve_frame(0, PageFaultType::Write)
            .unwrap();
        assert!(write.writable);
        assert_eq!(write.frame.ppn(), read.frame.ppn());
        assert!(address_space.pages.read().get(&0).unwrap().dirty);
    }

    #[kunit]
    fn address_space_admission_reads_the_inode_size_cell_directly() {
        let size = Arc::new(AtomicU64::new(0));
        let address_space = AddressSpace::new_volatile(size.clone());
        assert_eq!(
            address_space
                .resolve_frame(0, PageFaultType::Read)
                .unwrap_err(),
            SysError::NotMapped
        );

        size.store(PagingArch::PAGE_SIZE_BYTES as u64, Ordering::Release);
        let page = address_space.resolve_frame(0, PageFaultType::Read).unwrap();
        assert!(page.writable);
        size.store(0, Ordering::Release);
        assert_eq!(
            address_space
                .resolve_frame(0, PageFaultType::Read)
                .unwrap_err(),
            SysError::NotMapped
        );
    }
}
