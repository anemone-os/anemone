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

/// The bounded-range persistence capability consumed by an inode address space.
///
/// Implementations own only filesystem identity and transaction access. Page
/// publication, dirty state, accounting, and logical size remain owned by the
/// address space and inode.
pub(in crate::fs) trait AddressSpaceBackend: Send + Sync {
    fn batch_page_cap(&self) -> usize;

    fn fill_range(&self, offset: usize, data: &mut [u8]) -> Result<(), SysError>;

    fn writeback_range(&self, offset: usize, data: &[u8]) -> Result<(), SysError>;
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
        let batch_page_cap = backend.batch_page_cap();
        assert!(
            batch_page_cap > 0,
            "address-space batch cap must be non-zero"
        );
        assert!(
            batch_page_cap <= isize::MAX as usize / PagingArch::PAGE_SIZE_BYTES,
            "address-space batch buffer must fit the allocation size domain"
        );
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

    fn page_end(offset: usize, len: usize) -> Result<usize, SysError> {
        let end = offset.checked_add(len).ok_or(SysError::InvalidArgument)?;
        if len == 0 {
            Ok(Self::page_index(offset))
        } else {
            Self::page_index(end - 1)
                .checked_add(1)
                .ok_or(SysError::InvalidArgument)
        }
    }

    fn zeroed_staging(len: usize) -> Result<Vec<u8>, SysError> {
        let mut data = Vec::new();
        data.try_reserve_exact(len)
            .map_err(|_| SysError::OutOfMemory)?;
        data.resize(len, 0);
        Ok(data)
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

    fn load_page_for_request(
        &self,
        pidx: usize,
        requested_page_end: usize,
    ) -> Result<ResidentPage, SysError> {
        let offset = Self::page_start(pidx)?;
        let size = self.size()?;
        if offset >= size {
            return Err(SysError::NotMapped);
        }

        if let Some(page) = self.pages.read().get(&pidx) {
            return Ok(page.clone());
        }

        let Some(backend) = &self.backend else {
            return self.allocate_page(pidx);
        };

        let file_page_end = Self::page_index(size - 1) + 1;
        let run_limit = requested_page_end
            .min(file_page_end)
            .min(pidx.saturating_add(backend.batch_page_cap()));
        assert!(
            run_limit > pidx,
            "requested miss run must contain its first page"
        );

        let run_end = {
            let pages = self.pages.read();
            let mut end = pidx + 1;
            while end < run_limit && !pages.contains_key(&end) {
                end += 1;
            }
            end
        };
        let run_pages = run_end - pidx;
        let staging_len = run_pages
            .checked_mul(PagingArch::PAGE_SIZE_BYTES)
            .ok_or(SysError::InvalidArgument)?;
        let valid_len = (size - offset).min(staging_len);
        let mut staging = Self::zeroed_staging(staging_len)?;
        let mut new_pages = Vec::new();
        new_pages
            .try_reserve_exact(run_pages)
            .map_err(|_| SysError::OutOfMemory)?;
        for _ in 0..run_pages {
            let frame = alloc_frame_zeroed().ok_or(SysError::OutOfMemory)?;
            new_pages.push(ResidentPage {
                frame: unsafe { frame.into_frame_handle() },
                dirty: false,
            });
        }

        // Backend I/O runs without the page-map lock. No page from this run is
        // published until the complete bounded fill succeeds; concurrent
        // publication still wins independently for each page below.
        backend.fill_range(offset, &mut staging[..valid_len])?;
        let mut first = None;
        for (index, page) in new_pages.into_iter().enumerate() {
            let src_start = index * PagingArch::PAGE_SIZE_BYTES;
            let dst = unsafe {
                core::slice::from_raw_parts_mut(
                    page.frame.ppn().to_phys_addr().to_hhdm().as_ptr_mut(),
                    PagingArch::PAGE_SIZE_BYTES,
                )
            };
            dst.copy_from_slice(&staging[src_start..src_start + PagingArch::PAGE_SIZE_BYTES]);
            let published = self.publish(pidx + index, page);
            if first.is_none() {
                first = Some(published);
            }
        }
        Ok(first.expect("non-empty miss run must publish its first page"))
    }

    fn load_page(&self, pidx: usize) -> Result<ResidentPage, SysError> {
        let requested_page_end = pidx.checked_add(1).ok_or(SysError::InvalidArgument)?;
        self.load_page_for_request(pidx, requested_page_end)
    }

    fn resolve_frame_for_request(
        &self,
        pidx: usize,
        requested_page_end: usize,
        access: PageFaultType,
    ) -> Result<ResolvedFrame, SysError> {
        if requested_page_end <= pidx {
            return Err(SysError::InvalidArgument);
        }

        let page = self.load_page_for_request(pidx, requested_page_end)?;
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
        let requested_page_end = Self::page_end(offset, buffer.len())?;
        let mut remaining = buffer;
        let mut cur_offset = offset;

        while !remaining.is_empty() {
            let pidx = Self::page_index(cur_offset);
            let page_offset = cur_offset & (PagingArch::PAGE_SIZE_BYTES - 1);
            let copy_len = remaining
                .len()
                .min(PagingArch::PAGE_SIZE_BYTES - page_offset);

            let page = if self.backend.is_some() {
                Some(self.load_page_for_request(pidx, requested_page_end)?)
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
        let requested_page_end = Self::page_end(offset, len)?;
        let mut remaining = len;
        let mut cur_offset = offset;

        while remaining > 0 {
            let pidx = Self::page_index(cur_offset);
            let page_offset = cur_offset & (PagingArch::PAGE_SIZE_BYTES - 1);
            let copy_len = remaining.min(PagingArch::PAGE_SIZE_BYTES - page_offset);
            let page = if self.backend.is_some() {
                Some(self.load_page_for_request(pidx, requested_page_end)?)
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

    fn writeback_run(
        &self,
        pages: &[(usize, ResidentPage)],
        size_snapshot: usize,
    ) -> Result<(), SysError> {
        assert!(!pages.is_empty(), "writeback run must not be empty");
        let Some(backend) = &self.backend else {
            return Ok(());
        };
        let mut valid_pages = 0;
        for (pidx, _) in pages {
            if Self::page_start(*pidx)? >= size_snapshot {
                break;
            }
            valid_pages += 1;
        }
        if valid_pages == 0 {
            return Err(SysError::NotMapped);
        }

        let valid = &pages[..valid_pages];
        let offset = Self::page_start(valid[0].0)?;
        let staging_len = valid_pages
            .checked_mul(PagingArch::PAGE_SIZE_BYTES)
            .ok_or(SysError::InvalidArgument)?;
        let write_len = (size_snapshot - offset).min(staging_len);
        let mut staging = Self::zeroed_staging(staging_len)?;
        for (index, (_, page)) in valid.iter().enumerate() {
            let dst_start = index * PagingArch::PAGE_SIZE_BYTES;
            staging[dst_start..dst_start + PagingArch::PAGE_SIZE_BYTES]
                .copy_from_slice(page.frame.as_bytes());
        }
        backend.writeback_range(offset, &staging[..write_len])?;
        // Dirty is intentionally sticky: writable PTE stores are not yet
        // tracked well enough to prove that a successful writeback stayed
        // clean. Eviction is the only current dirty retirement point.
        if valid_pages < pages.len() {
            Err(SysError::NotMapped)
        } else {
            Ok(())
        }
    }

    fn next_dirty_run(
        &self,
        start: usize,
        end: usize,
    ) -> Result<Option<(Vec<(usize, ResidentPage)>, usize)>, SysError> {
        let Some(backend) = &self.backend else {
            return Ok(None);
        };
        let cap = backend.batch_page_cap();
        let pages = self.pages.read();
        let Some((&first, _)) = pages.range(start..end).find(|(_, page)| page.dirty) else {
            return Ok(None);
        };

        let mut run_end = first;
        let mut run_pages = 0;
        while run_end < end && run_pages < cap {
            match pages.get(&run_end) {
                Some(page) if page.dirty => {
                    run_pages += 1;
                    run_end = run_end.checked_add(1).ok_or(SysError::InvalidArgument)?;
                },
                _ => break,
            }
        }

        let mut run = Vec::new();
        run.try_reserve_exact(run_pages)
            .map_err(|_| SysError::OutOfMemory)?;
        for pidx in first..run_end {
            run.push((
                pidx,
                pages
                    .get(&pidx)
                    .expect("discovered dirty run must remain under page-map read lock")
                    .clone(),
            ));
        }
        Ok(Some((run, run_end)))
    }

    fn sync_dirty_range(&self, start: usize, end: usize) -> Result<(), SysError> {
        let mut cursor = start;
        while cursor < end {
            // Size is sampled before the frame snapshot. A concurrent grow may
            // invalidate this run, but it must not make bytes beyond the old
            // EOF in an already-cloned frame eligible for writeback.
            let size_snapshot = self.size()?;
            // A frame snapshot remains authoritative only for this bounded
            // backend call. The next run is rediscovered from the live page
            // map so invalidation cannot leave operation-wide stale frames.
            let Some((run, next)) = self.next_dirty_run(cursor, end)? else {
                break;
            };
            self.writeback_run(&run, size_snapshot)?;
            cursor = next;
        }
        Ok(())
    }

    pub(in crate::fs) fn sync_page(&self, pidx: usize) -> Result<(), SysError> {
        if self.backend.is_none() {
            return Ok(());
        }
        let size = self.size()?;
        if Self::page_start(pidx)? >= size {
            return Err(SysError::NotMapped);
        }
        let page = self.pages.read().get(&pidx).cloned();
        if let Some(page) = page.filter(|page| page.dirty) {
            self.writeback_run(&[(pidx, page)], size)?;
        }
        Ok(())
    }

    pub(in crate::fs) fn sync_all(&self) -> Result<(), SysError> {
        self.sync_dirty_range(0, usize::MAX)
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
        let requested_page_end = pidx.checked_add(1).ok_or(SysError::InvalidArgument)?;
        self.resolve_frame_for_request(pidx, requested_page_end, access)
    }

    fn resolve_frame_ahead(
        &self,
        pidx: usize,
        request_end: usize,
        access: PageFaultType,
    ) -> Result<Option<ResolvedFrame>, SysError> {
        // A speculative backed write may populate clean cache pages, but it
        // must not publish sticky dirty state before userspace actually writes.
        // Volatile address spaces have no dirty/writeback fact, so they can
        // resolve the initiating access directly.
        let ahead_access = if self.backend.is_some() && matches!(access, PageFaultType::Write) {
            PageFaultType::Read
        } else {
            access
        };
        self.resolve_frame_for_request(pidx, request_end, ahead_access)
            .map(Some)
    }

    fn sync_range(&self, range: core::ops::Range<usize>) -> Result<(), SysError> {
        if self.backend.is_none() || range.start >= range.end {
            return Ok(());
        }
        self.sync_dirty_range(range.start, range.end)?;
        let size = self.size()?;
        let file_page_end = Self::page_end(0, size)?;
        if range.end > file_page_end {
            Err(SysError::NotMapped)
        } else {
            Ok(())
        }
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
        batch_page_cap: usize,
        fills: SpinLock<Vec<(usize, usize)>>,
        writebacks: SpinLock<Vec<(usize, usize)>>,
        fail_fill_offset: SpinLock<Option<usize>>,
        fail_writeback_offset: SpinLock<Option<usize>>,
    }

    impl FakeBackend {
        fn new(batch_page_cap: usize) -> Self {
            Self {
                batch_page_cap,
                fills: SpinLock::new(Vec::new()),
                writebacks: SpinLock::new(Vec::new()),
                fail_fill_offset: SpinLock::new(None),
                fail_writeback_offset: SpinLock::new(None),
            }
        }

        fn take_fills(&self) -> Vec<(usize, usize)> {
            core::mem::take(&mut *self.fills.lock())
        }

        fn take_writebacks(&self) -> Vec<(usize, usize)> {
            core::mem::take(&mut *self.writebacks.lock())
        }

        fn fail_fill_at(&self, offset: Option<usize>) {
            *self.fail_fill_offset.lock() = offset;
        }

        fn fail_writeback_at(&self, offset: Option<usize>) {
            *self.fail_writeback_offset.lock() = offset;
        }
    }

    impl AddressSpaceBackend for FakeBackend {
        fn batch_page_cap(&self) -> usize {
            self.batch_page_cap
        }

        fn fill_range(&self, offset: usize, data: &mut [u8]) -> Result<(), SysError> {
            self.fills.lock().push((offset, data.len()));
            if *self.fail_fill_offset.lock() == Some(offset) {
                return Err(SysError::InvalidArgument);
            }
            for (index, byte) in data.iter_mut().enumerate() {
                *byte = offset.wrapping_add(index) as u8;
            }
            Ok(())
        }

        fn writeback_range(&self, offset: usize, data: &[u8]) -> Result<(), SysError> {
            self.writebacks.lock().push((offset, data.len()));
            if *self.fail_writeback_offset.lock() == Some(offset) {
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
        let backend = Arc::new(FakeBackend::new(4));
        let address_space = AddressSpace::new_backed(size, backend.clone());

        let first = address_space.resolve_frame(0, PageFaultType::Read).unwrap();
        assert_eq!(first.frame.as_bytes()[17], 17);
        address_space.resolve_frame(0, PageFaultType::Read).unwrap();
        assert_eq!(backend.take_fills(), [(0, PagingArch::PAGE_SIZE_BYTES)]);
        assert_eq!(crate::fs::resident_file_inode_cache_pages(), before + 1);

        address_space
            .resolve_frame(0, PageFaultType::Write)
            .unwrap();
        backend.fail_writeback_at(Some(0));
        assert_eq!(
            address_space.sync_page(0).unwrap_err(),
            SysError::InvalidArgument
        );
        assert!(address_space.pages.read().get(&0).unwrap().dirty);

        backend.fail_writeback_at(None);
        address_space.sync_page(0).unwrap();
        assert!(address_space.pages.read().get(&0).unwrap().dirty);
        assert_eq!(
            backend.take_writebacks(),
            [
                (0, PagingArch::PAGE_SIZE_BYTES),
                (0, PagingArch::PAGE_SIZE_BYTES)
            ]
        );

        drop(address_space);
        assert_eq!(crate::fs::resident_file_inode_cache_pages(), before);
    }

    #[kunit]
    fn backed_requested_reads_batch_misses_and_respect_resident_and_cap_boundaries() {
        let page_size = PagingArch::PAGE_SIZE_BYTES;

        let backend = Arc::new(FakeBackend::new(4));
        let address_space = AddressSpace::new_backed(
            Arc::new(AtomicU64::new((page_size * 3) as u64)),
            backend.clone(),
        );
        address_space.read(0, &mut vec![0; page_size * 3]).unwrap();
        assert_eq!(backend.take_fills(), [(0, page_size * 3)]);

        let backend = Arc::new(FakeBackend::new(4));
        let address_space = AddressSpace::new_backed(
            Arc::new(AtomicU64::new((page_size * 5) as u64)),
            backend.clone(),
        );
        let resident = address_space
            .resolve_frame(2, PageFaultType::Read)
            .unwrap()
            .frame;
        backend.take_fills();
        address_space.read(0, &mut vec![0; page_size * 5]).unwrap();
        assert_eq!(
            backend.take_fills(),
            [(0, page_size * 2), (page_size * 3, page_size * 2)]
        );
        assert_eq!(
            address_space.pages.read().get(&2).unwrap().frame.ppn(),
            resident.ppn()
        );

        let backend = Arc::new(FakeBackend::new(2));
        let address_space = AddressSpace::new_backed(
            Arc::new(AtomicU64::new((page_size * 5) as u64)),
            backend.clone(),
        );
        address_space.read(0, &mut vec![0; page_size * 5]).unwrap();
        assert_eq!(
            backend.take_fills(),
            [
                (0, page_size * 2),
                (page_size * 2, page_size * 2),
                (page_size * 4, page_size)
            ]
        );
    }

    #[kunit]
    fn backed_write_fault_ahead_batches_clean_followers_without_premature_dirty() {
        let page_size = PagingArch::PAGE_SIZE_BYTES;
        let backend = Arc::new(FakeBackend::new(8));
        let address_space = AddressSpace::new_backed(
            Arc::new(AtomicU64::new((page_size * 4) as u64)),
            backend.clone(),
        );

        let demand = address_space
            .resolve_frame(0, PageFaultType::Write)
            .unwrap();
        assert!(demand.writable);
        let follower = address_space
            .resolve_frame_ahead(1, 4, PageFaultType::Write)
            .unwrap()
            .expect("backed address space should opt in to fault-ahead");
        assert!(!follower.writable);
        address_space
            .resolve_frame_ahead(2, 4, PageFaultType::Write)
            .unwrap()
            .expect("batched follower should remain resolvable");

        assert_eq!(
            backend.take_fills(),
            [(0, page_size), (page_size, page_size * 3)]
        );
        let pages = address_space.pages.read();
        assert!(pages.get(&0).unwrap().dirty);
        for pidx in 1..4 {
            assert!(!pages.get(&pidx).unwrap().dirty);
        }
    }

    #[kunit]
    fn backed_read_batch_handles_unaligned_eof_and_failure_before_publication() {
        let page_size = PagingArch::PAGE_SIZE_BYTES;
        let before = crate::fs::resident_file_inode_cache_pages();
        let backend = Arc::new(FakeBackend::new(4));
        let address_space = AddressSpace::new_backed(
            Arc::new(AtomicU64::new((page_size + 17) as u64)),
            backend.clone(),
        );
        let offset = page_size - 3;
        let mut data = [0u8; 20];
        address_space.read(offset, &mut data).unwrap();
        for (index, byte) in data.iter().enumerate() {
            assert_eq!(*byte, offset.wrapping_add(index) as u8);
        }
        assert_eq!(backend.take_fills(), [(0, page_size + 17)]);
        assert!(
            address_space.pages.read().get(&1).unwrap().frame.as_bytes()[17..]
                .iter()
                .all(|byte| *byte == 0)
        );
        drop(address_space);
        assert_eq!(crate::fs::resident_file_inode_cache_pages(), before);

        let backend = Arc::new(FakeBackend::new(4));
        backend.fail_fill_at(Some(0));
        let address_space = AddressSpace::new_backed(
            Arc::new(AtomicU64::new((page_size * 2) as u64)),
            backend.clone(),
        );
        assert_eq!(
            address_space
                .read(0, &mut vec![0; page_size * 2])
                .unwrap_err(),
            SysError::InvalidArgument
        );
        assert!(address_space.pages.read().is_empty());
        assert_eq!(backend.take_fills(), [(0, page_size * 2)]);
        assert_eq!(crate::fs::resident_file_inode_cache_pages(), before);
    }

    #[kunit]
    fn backed_read_failure_after_completed_batch_keeps_published_prefix() {
        let page_size = PagingArch::PAGE_SIZE_BYTES;
        let before = crate::fs::resident_file_inode_cache_pages();
        let backend = Arc::new(FakeBackend::new(1));
        backend.fail_fill_at(Some(page_size));
        let address_space = AddressSpace::new_backed(
            Arc::new(AtomicU64::new((page_size * 2) as u64)),
            backend.clone(),
        );
        let mut data = vec![0; page_size * 2];

        assert_eq!(
            address_space.read(0, &mut data).unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(data[17], 17);
        assert_eq!(
            address_space
                .pages
                .read()
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            [0]
        );
        assert_eq!(
            backend.take_fills(),
            [(0, page_size), (page_size, page_size)]
        );
        assert_eq!(crate::fs::resident_file_inode_cache_pages(), before + 1);

        drop(address_space);
        assert_eq!(crate::fs::resident_file_inode_cache_pages(), before);
    }

    #[kunit]
    fn backed_competing_publication_selects_one_frame_and_one_accounting_winner() {
        let before = crate::fs::resident_file_inode_cache_pages();
        let address_space = AddressSpace::new_backed(
            Arc::new(AtomicU64::new(PagingArch::PAGE_SIZE_BYTES as u64)),
            Arc::new(FakeBackend::new(1)),
        );
        let first = ResidentPage {
            frame: unsafe {
                alloc_frame_zeroed()
                    .expect("test frame allocation must succeed")
                    .into_frame_handle()
            },
            dirty: false,
        };
        let second = ResidentPage {
            frame: unsafe {
                alloc_frame_zeroed()
                    .expect("test frame allocation must succeed")
                    .into_frame_handle()
            },
            dirty: false,
        };
        let winner = address_space.publish(0, first.clone());
        let loser_observation = address_space.publish(0, second);

        assert_eq!(winner.frame.ppn(), first.frame.ppn());
        assert_eq!(loser_observation.frame.ppn(), first.frame.ppn());
        assert_eq!(address_space.pages.read().len(), 1);
        assert_eq!(crate::fs::resident_file_inode_cache_pages(), before + 1);

        drop(address_space);
        assert_eq!(crate::fs::resident_file_inode_cache_pages(), before);
    }

    #[kunit]
    fn backed_writeback_batches_dirty_runs_and_keeps_dirty_sticky() {
        let page_size = PagingArch::PAGE_SIZE_BYTES;
        let backend = Arc::new(FakeBackend::new(2));
        let address_space = AddressSpace::new_backed(
            Arc::new(AtomicU64::new((page_size * 6) as u64)),
            backend.clone(),
        );
        for pidx in [0, 1, 3, 4, 5] {
            address_space
                .resolve_frame(pidx, PageFaultType::Write)
                .unwrap();
        }
        address_space.resolve_frame(2, PageFaultType::Read).unwrap();
        backend.take_fills();

        backend.fail_writeback_at(Some(page_size * 3));
        assert_eq!(
            address_space.sync_all().unwrap_err(),
            SysError::InvalidArgument
        );
        assert_eq!(
            backend.take_writebacks(),
            [(0, page_size * 2), (page_size * 3, page_size * 2)]
        );
        assert!(
            [0, 1, 3, 4, 5]
                .iter()
                .all(|pidx| address_space.pages.read().get(pidx).unwrap().dirty)
        );

        backend.fail_writeback_at(None);
        address_space.sync_all().unwrap();
        assert_eq!(
            backend.take_writebacks(),
            [
                (0, page_size * 2),
                (page_size * 3, page_size * 2),
                (page_size * 5, page_size)
            ]
        );
        assert!(
            [0, 1, 3, 4, 5]
                .iter()
                .all(|pidx| address_space.pages.read().get(pidx).unwrap().dirty)
        );

        address_space.sync_page(3).unwrap();
        assert_eq!(backend.take_writebacks(), [(page_size * 3, page_size)]);
    }

    #[kunit]
    fn backed_writeback_run_uses_size_sampled_before_its_frame_snapshot() {
        let page_size = PagingArch::PAGE_SIZE_BYTES;
        let size = Arc::new(AtomicU64::new((page_size + 17) as u64));
        let backend = Arc::new(FakeBackend::new(2));
        let address_space = AddressSpace::new_backed(size.clone(), backend.clone());
        address_space
            .resolve_frame(1, PageFaultType::Write)
            .unwrap();
        backend.take_fills();

        let size_snapshot = address_space.size().unwrap();
        let (run, _) = address_space
            .next_dirty_run(1, 2)
            .unwrap()
            .expect("dirty tail page must form one run");
        size.store((page_size * 2) as u64, Ordering::Release);
        address_space.writeback_run(&run, size_snapshot).unwrap();

        assert_eq!(backend.take_writebacks(), [(page_size, 17)]);
    }

    #[kunit]
    fn backed_sync_range_respects_range_cap_eof_and_existing_errno_order() {
        let page_size = PagingArch::PAGE_SIZE_BYTES;
        let backend = Arc::new(FakeBackend::new(2));
        let address_space = AddressSpace::new_backed(
            Arc::new(AtomicU64::new((page_size * 4 + 17) as u64)),
            backend.clone(),
        );
        for pidx in 0..5 {
            address_space
                .resolve_frame(pidx, PageFaultType::Write)
                .unwrap();
        }
        backend.take_fills();

        address_space.sync_range(1..4).unwrap();
        assert_eq!(
            backend.take_writebacks(),
            [(page_size, page_size * 2), (page_size * 3, page_size)]
        );

        address_space.sync_range(6..6).unwrap();
        assert!(backend.take_writebacks().is_empty());

        assert_eq!(
            address_space.sync_range(4..6).unwrap_err(),
            SysError::NotMapped
        );
        assert_eq!(backend.take_writebacks(), [(page_size * 4, 17)]);
    }

    #[kunit]
    fn backed_resolution_arms_first_write_before_publishing_writable_frame() {
        let size = Arc::new(AtomicU64::new(PagingArch::PAGE_SIZE_BYTES as u64));
        let backend = Arc::new(FakeBackend::new(4));
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
