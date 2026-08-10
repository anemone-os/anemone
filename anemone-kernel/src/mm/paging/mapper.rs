use core::{cmp::min, fmt::Debug, marker::PhantomData, mem::ManuallyDrop, slice};

use crate::{prelude::*, utils::data::DataSource};

/// POD struct representing a mapping operation.
#[derive(Debug, Clone, Copy)]
pub struct Mapping {
    pub vpn: VirtPageNum,
    pub ppn: PhysPageNum,
    pub flags: PteFlags,
    pub npages: usize,
    /// Only for architectures that support huge page mapping,
    /// and indicates whether huge page mapping should be used for this mapping
    /// operation.
    pub huge_pages: bool,
}

/// An unmapping operation.
#[derive(Debug, Clone, Copy)]
pub struct Unmapping {
    pub range: VirtPageRange,
}

#[derive(Debug, Clone, Copy)]
pub struct Translated {
    pub ppn: PhysPageNum,
    pub flags: PteFlags,
}

/// Operation-local relation between the old and new leaf PTE at one commit.
///
/// The PTE remains the only source of truth. This result describes only the
/// completed mutation and must not be cached as address-space state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeafPteCommit {
    /// No valid mapping was replaced anywhere along the committed walk.
    Added,
    /// The same frame and generic flags were written again.
    Unchanged,
    /// The same frame retained every old generic permission and gained more.
    Relaxed,
    /// A frame, permission, leaf, or valid ancestor was replaced or restricted.
    ReplacedOrRestricted,
}

/// Flow signal for page table traversal.
///
/// The [core::ops::ControlFlow] type isn't suitable for our use case, as we
/// need to distinguish between "break with a value" and "skip the current node
/// and continue with the next one".
///
/// TODO: implement Skip
#[derive(Debug, Clone, Copy)]
enum ControlFlow<R> {
    Continue,
    Break(R),
    // Skip,
}

enum PageTableRetirement<'a> {
    Immediate,
    Retire(&'a mut Vec<OwnedFrameHandle>),
    Keep,
}

/// Mapper. Computation engine for page table traversal and modification.
///
/// **Note that Mapper won't flush the TLB when needed, as should be done by the
/// caller.**
#[derive(Debug)]
pub struct Mapper<'a> {
    root: PhysPageNum,
    _lifetime: PhantomData<&'a mut PageTable>,
}

impl Mapper<'_> {
    fn canonicalize_vpn(vpn: u64) -> VirtPageNum {
        let vpn_bits = PagingArch::PAGE_LEVELS * PagingArch::PGDIR_IDX_BITS;
        let effective_vpn_bits = u64::BITS as usize - PagingArch::PAGE_SIZE_BITS;

        let low_mask = if vpn_bits == u64::BITS as usize {
            !0u64
        } else {
            (1u64 << vpn_bits) - 1
        };
        let mut value = vpn & low_mask;

        let sign_bit = 1u64 << (vpn_bits - 1);
        if value & sign_bit != 0 {
            let effective_mask = if effective_vpn_bits == u64::BITS as usize {
                !0u64
            } else {
                (1u64 << effective_vpn_bits) - 1
            };
            let sign_extend_mask = effective_mask & !low_mask;
            value |= sign_extend_mask;
        }

        VirtPageNum::new(value)
    }

    pub(super) fn new(pgtbl: &mut PageTable) -> Self {
        Self {
            root: pgtbl.root_ppn(),
            _lifetime: PhantomData,
        }
    }

    /// Fill bytes in the given vaddr range from the source via hhdm.
    ///
    /// # Safety
    ///
    /// * Access via vaddr bypasses permission checks and may write to
    ///   unauthorized code regions.
    /// * This function overwrites existing data.
    /// * No rollback is possible if an error occurs.
    /// * This function does not validate that the index is within the valid
    ///   index range of `source`.
    pub unsafe fn fill_data<TErr>(
        &mut self,
        vaddr: VirtAddr,
        source: &impl DataSource<TError = impl Into<TErr>>,
        length: u64,
    ) -> Result<(), TErr>
    where
        TErr: Debug + From<SysError>,
    {
        if length == 0 {
            return Ok(());
        }
        let vaddr = vaddr;
        let vaddr_end = VirtAddr::new(vaddr.get().wrapping_add(length));
        if vaddr_end < vaddr {
            return Err(TErr::from(SysError::InvalidArgument));
        }
        let vpn_st = vaddr.page_down();
        let vpn_end = vaddr_end.page_up();
        let fp_offset = (vaddr - vpn_st.to_virt_addr()) as usize;
        let fp_datasz = PagingArch::PAGE_SIZE_BYTES - fp_offset;
        let fp_ppn = self.translate(vpn_st).ok_or(SysError::NotMapped)?.ppn;
        source
            .copy_to(0, unsafe {
                slice::from_raw_parts_mut(
                    (fp_ppn.to_hhdm().to_virt_addr().get() as usize + fp_offset) as *mut u8,
                    min(fp_datasz, length as usize),
                )
            })
            .map_err(|e| e.into())?;
        let mut count = 0;
        for vpn in (vpn_st + 1).get()..(vpn_end - 1).get() {
            let ppn = self
                .translate(VirtPageNum::new(vpn))
                .ok_or(SysError::NotMapped)?
                .ppn;
            let addr_st = ppn.to_hhdm().to_virt_addr().get();
            source
                .copy_to(fp_datasz + count * PagingArch::PAGE_SIZE_BYTES, unsafe {
                    slice::from_raw_parts_mut(
                        addr_st as *const u8 as *mut u8,
                        PagingArch::PAGE_SIZE_BYTES,
                    )
                })
                .map_err(|e| e.into())?;
            count += 1;
        }
        if vpn_end != vpn_st + 1 {
            let ed_ppn = self.translate(vpn_end - 1).ok_or(SysError::NotMapped)?.ppn;
            let addr_st = ed_ppn.to_hhdm().to_virt_addr().get();
            let data_st = fp_datasz + count * PagingArch::PAGE_SIZE_BYTES;
            unsafe {
                source
                    .copy_to(data_st, unsafe {
                        slice::from_raw_parts_mut(
                            addr_st as *const u8 as *mut u8,
                            length as usize - data_st,
                        )
                    })
                    .map_err(|e| e.into())?;
            }
        }
        Ok(())
    }

    /// Map a virtual memory region to a physical memory region with the given
    /// flags. Had encountered an already mapped page will cause an error to be
    /// returned, and all the successfully mapped pages will be rolled back.
    ///
    /// Only global pages can be huge pages, otherwise a panic will be
    /// triggered.
    pub fn map(&mut self, mapping: Mapping) -> Result<(), SysError> {
        if mapping.huge_pages && !mapping.flags.contains(PteFlags::GLOBAL) {
            panic!("internal error: huge page mapping must be global");
        }

        #[derive(Debug)]
        struct MapTransaction<'a, 'm> {
            mapper: &'a mut Mapper<'m>,
            mapping: Mapping,
            mapped_pages: usize,
        }

        impl<'a, 'm> MapTransaction<'a, 'm> {
            fn new(mapper: &'a mut Mapper<'m>, mapping: Mapping) -> Self {
                Self {
                    mapper,
                    mapping,
                    mapped_pages: 0,
                }
            }

            fn do_map(&mut self) -> Result<(), SysError> {
                let Mapping {
                    vpn,
                    ppn,
                    flags,
                    npages,
                    huge_pages: enable_huge_pages,
                    ..
                } = self.mapping;
                let mut index = 0;
                let max_hpage_level = {
                    if !enable_huge_pages {
                        0
                    } else {
                        let levels = (vpn.get() ^ ppn.get()).trailing_zeros() as usize
                            / PagingArch::PGDIR_IDX_BITS;
                        min(levels, PagingArch::MAX_HUGE_PAGE_LEVEL)
                    }
                };
                while index < npages {
                    for level in (0..=max_hpage_level).rev() {
                        let level_page_size = 1 << (level * PagingArch::PGDIR_IDX_BITS);
                        if (vpn.get() as usize + index) & (level_page_size - 1) != 0 {
                            continue;
                        }
                        if npages - index < level_page_size {
                            continue;
                        }
                        let next_vpn = VirtPageNum::new(vpn.get() + index as u64);
                        let next_ppn = PhysPageNum::new(ppn.get() + index as u64);
                        unsafe {
                            self.mapper
                                .map_one(next_vpn, next_ppn, flags, level, false)?;
                        }
                        self.mapped_pages += level_page_size;
                        index += level_page_size;
                        break;
                    }
                }
                Ok(())
            }

            fn commit(self) {
                let _ = ManuallyDrop::new(self);
            }
        }

        impl Drop for MapTransaction<'_, '_> {
            fn drop(&mut self) {
                knoticeln!(
                    "MapTransaction::Drop: transaction failed, rolling back the mapped pages"
                );
                // roll back the mapping of already mapped pages
                unsafe {
                    self.mapper.try_unmap(Unmapping {
                        range: VirtPageRange::new(self.mapping.vpn, self.mapped_pages as u64),
                    });
                }
            }
        }

        let mut transaction = MapTransaction::new(self, mapping);
        transaction.do_map()?;
        transaction.commit();

        Ok(())
    }

    /// Map a virtual memory region to a physical memory region with the given
    /// flags, even if some of the pages in the region are already mapped.
    ///
    /// # Safety
    ///
    /// * **No atomicity guarantees**
    ///
    /// * Unless user can ensure success of the operation, it is
    /// always recommended to first unmap the virtual address and then map
    /// it again, instead of using overwrite mapping directly.
    ///
    /// * Some **dangerous** operations, like overwrite mapping a global page,
    /// should be manually avoided.
    ///
    /// * Only global pages can be huge pages, otherwise a panic will be
    ///   triggered.
    pub unsafe fn map_overwrite(&mut self, mapping: Mapping) -> Result<(), SysError> {
        if mapping.huge_pages && !mapping.flags.contains(PteFlags::GLOBAL) {
            panic!("internal error: huge page mapping must be global");
        }

        let Mapping {
            vpn,
            ppn,
            flags,
            npages,
            huge_pages: enable_huge_pages,
            ..
        } = mapping;
        let mut index = 0;
        let max_hpage_level = {
            if !enable_huge_pages {
                0
            } else {
                let levels =
                    (vpn.get() ^ ppn.get()).trailing_zeros() as usize / PagingArch::PGDIR_IDX_BITS;
                min(levels, PagingArch::MAX_HUGE_PAGE_LEVEL)
            }
        };
        while index < npages {
            for level in (0..=max_hpage_level).rev() {
                let level_page_size = 1 << (level * PagingArch::PGDIR_IDX_BITS);
                if (vpn.get() as usize + index) & (level_page_size - 1) != 0 {
                    continue;
                }
                if npages - index < level_page_size {
                    continue;
                }
                let next_vpn = VirtPageNum::new(vpn.get() + index as u64);
                let next_ppn = PhysPageNum::new(ppn.get() + index as u64);
                unsafe {
                    self.map_one(next_vpn, next_ppn, flags, level,true).map_err(|err| {
                        kwarningln!(
                            "Mapper::map_overwrite: failed to map vpn {:?} to ppn {:?} with flags {:?}: {:?}",
                            next_vpn,
                            next_ppn,
                            flags,
                            err
                        );
                        err
                    })?;
                }
                index += level_page_size;
                break;
            }
        }

        Ok(())
    }

    /// Unmap a virtual memory region and deallocate page tables if they become
    /// empty after unmapping.
    ///
    /// When encountered with large pages that is not fully covered by the
    /// unmapping range,     **that large page will be skipped and left
    /// intact**, as we don't want allocate new page tables for
    ///     splitting the large page, which may cause errors that we don't want
    /// to handle.
    /// # Rules
    ///  * Global Pages won't be unmapped, and will be left intact.
    ///  * Huge pages that are not fully covered by the unmapping range won't be
    ///    unmapped.
    ///  * Empty page tables, except those marked as global, will be
    ///    deallocated.
    ///  * Unmapping an already unmapped page is considered a no-op, but will
    ///    cause a warning to be printed.
    ///
    /// **This method is deliberately designed not to return a [Result] but
    /// always succeed.**
    ///
    /// # Safety
    ///
    /// This method is unsafe because it cannot always fully unmap
    ///     mappings within the given range.
    pub unsafe fn try_unmap(&mut self, unmapping: Unmapping) {
        unsafe { self.try_unmap_with(unmapping, PageTableRetirement::Immediate) }
    }

    /// Unmap leaves while transferring detached page-table frames to the
    /// caller. The caller must retain them until remote page walks using the
    /// old branch PTEs have completed.
    pub(crate) unsafe fn try_unmap_retiring_page_tables(
        &mut self,
        unmapping: Unmapping,
        retired: &mut Vec<OwnedFrameHandle>,
    ) {
        unsafe { self.try_unmap_with(unmapping, PageTableRetirement::Retire(retired)) }
    }

    /// Unmap leaves without detaching empty page tables. This is used by exit,
    /// where the complete page table remains owned until its later safe drop.
    pub(crate) unsafe fn try_unmap_keep_page_tables(&mut self, unmapping: Unmapping) {
        unsafe { self.try_unmap_with(unmapping, PageTableRetirement::Keep) }
    }

    unsafe fn try_unmap_with(
        &mut self,
        unmapping: Unmapping,
        mut retirement: PageTableRetirement<'_>,
    ) {
        let Unmapping { range } = unmapping;

        unsafe {
            match self.traverse(
                range,
                |pte, _ctx| {
                    let ppn = pte.ppn();
                    let pgdir = ppn
                        .to_phys_addr()
                        .to_hhdm()
                        .as_ptr::<PgDir>()
                        .as_ref()
                        .expect("pgdir ppn should not be null");

                    if pgdir.is_empty() && !pte.is_global() {
                        match &mut retirement {
                            PageTableRetirement::Immediate => {
                                *pte = Pte::ZEROED;
                                let _frame = OwnedFrameHandle::from_ppn(ppn);
                            },
                            PageTableRetirement::Retire(retired) => {
                                *pte = Pte::ZEROED;
                                retired.push(OwnedFrameHandle::from_ppn(ppn));
                            },
                            PageTableRetirement::Keep => {},
                        }
                    }
                    ControlFlow::<()>::Continue
                },
                |pte, ctx| {
                    if range.covers(&VirtPageRange::new(
                        ctx.vpn,
                        1u64 << (ctx.level * PagingArch::PGDIR_IDX_BITS),
                    )) {
                        if !pte.is_valid() {
                            kwarningln!(
                                "Mapper::try_unmap: trying to unmap an unmapped page: vpn={:?}",
                                ctx.vpn
                            );
                        }
                        if !pte.is_global() {
                            *pte = Pte::ZEROED;
                        }
                    }

                    ControlFlow::Continue
                },
                TraverseOrder::PostOrder,
            ) {
                ControlFlow::Continue => {},
                ControlFlow::Break(err) => {
                    unreachable!("unexpected break during unmapping: {:?}", err)
                },
            }
        }
    }

    /// Translate a virtual page number to a physical page number and its flags.
    pub fn translate(&self, vpn: VirtPageNum) -> Option<Translated> {
        let levels = PagingArch::PAGE_LEVELS;
        let vpn_bits = PagingArch::PTE_PER_PGDIR.trailing_zeros() as usize;

        let mut pgdir = unsafe {
            self.root
                .to_phys_addr()
                .to_hhdm()
                .as_ptr::<PgDir>()
                .as_ref()
                .expect("root ppn should not be null")
        };

        for level in (0..levels).rev() {
            let idx = (vpn.get() as usize >> (level * vpn_bits)) & (PagingArch::PTE_PER_PGDIR - 1);
            let pte = &pgdir[idx];

            if pte.is_branch() {
                if level == 0 {
                    return None;
                }
                pgdir = unsafe {
                    pte.ppn()
                        .to_phys_addr()
                        .to_hhdm()
                        .as_ptr::<PgDir>()
                        .as_ref()
                        .expect("pgdir ppn should not be null")
                };
            } else if pte.is_leaf() {
                debug_assert!(pte.is_valid());
                if level == 0 {
                    return Some(Translated {
                        ppn: pte.ppn(),
                        flags: pte.flags() & !PteFlags::VALID,
                    });
                } else {
                    // huge page
                    let level_offset = level * vpn_bits;
                    let level_mask = (1 << level_offset) - 1;
                    let page_offset = vpn.get() & level_mask;
                    let ppn_value = pte.ppn().get();
                    debug_assert!(ppn_value & level_mask == 0);
                    return Some(Translated {
                        ppn: PhysPageNum::new(ppn_value + page_offset),
                        flags: pte.flags() & !PteFlags::VALID,
                    });
                }
            } else {
                return None;
            }
        }
        None
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TraverseOrder {
    PreOrder,
    PostOrder,
}

#[derive(Debug)]
struct BranchCtx {
    /// The level of the pgdir that contains the branch pte.
    ///
    /// this field will never be zero.
    level: usize,
}

#[derive(Debug)]
struct LeafCtx {
    vpn: VirtPageNum,
    /// The level of the pgdir that contains the branch pte.
    level: usize,
}

impl Mapper<'_> {
    pub unsafe fn change_flags<F: FnMut(VirtPageNum, PteFlags) -> Option<PteFlags>>(
        &mut self,
        range: VirtPageRange,
        mut op: F,
        order: TraverseOrder,
    ) {
        unsafe {
            self.traverse::<_, _, ()>(
                range,
                |_, _| ControlFlow::Continue,
                |pte, ctx| {
                    let res = &op(ctx.vpn, pte.flags());
                    if let Some(new_flags) = res {
                        pte.set_flags(*new_flags);
                    }
                    ControlFlow::Continue
                },
                order,
            )
        };
    }
    /// Traverse the page table with the given branch and leaf operations.
    ///
    /// **Huge pages will be treated as leaf nodes, and will not be traversed
    /// into.**
    ///
    /// # Safety
    ///
    /// Use-after-free and double-free may happen if the branch and leaf
    /// operations deallocate page tables or page mappings. Caller must
    /// ensure that the operations are safe to perform.
    unsafe fn traverse<B, L, R>(
        &mut self,
        range: VirtPageRange,
        branch_op: B,
        leaf_op: L,
        order: TraverseOrder,
    ) -> ControlFlow<R>
    where
        B: FnMut(&mut Pte, BranchCtx) -> ControlFlow<R>,
        L: FnMut(&mut Pte, LeafCtx) -> ControlFlow<R>,
    {
        let (mut branch_op, mut leaf_op) = (branch_op, leaf_op);
        unsafe {
            Self::do_traverse(
                self.root,
                range,
                &mut branch_op,
                &mut leaf_op,
                order,
                PagingArch::PAGE_LEVELS - 1,
                0,
            )
        }
    }

    /// Internal method for page table traversal. See [Mapper::traverse] for
    /// details.
    ///
    /// Traverse the page table rooted at `pgdir_ppn`, with the given branch and
    /// leaf operations, and the current level and vpn prefix.
    /// This will recursively traverse the branch Ptes.
    ///
    /// # Notes
    ///
    /// **Huge pages are treated as leaf nodes, and will not be traversed
    /// into.**
    unsafe fn do_traverse<B, L, R>(
        pgdir_ppn: PhysPageNum,
        range: VirtPageRange,
        branch_op: &mut B,
        leaf_op: &mut L,
        order: TraverseOrder,
        level: usize,
        vpn_prefix: u64,
    ) -> ControlFlow<R>
    where
        B: FnMut(&mut Pte, BranchCtx) -> ControlFlow<R>,
        L: FnMut(&mut Pte, LeafCtx) -> ControlFlow<R>,
    {
        let vpn_bits = PagingArch::PTE_PER_PGDIR.trailing_zeros() as usize;

        let pgdir = unsafe {
            pgdir_ppn
                .to_phys_addr()
                .to_hhdm()
                .as_ptr_mut::<PgDir>()
                .as_mut()
                .expect("root ppn should not be null")
        };

        for idx in 0..PagingArch::PTE_PER_PGDIR {
            let pte = &mut pgdir[idx];

            if !pte.is_branch() && !pte.is_leaf() {
                continue;
            }

            let vpn_prefix = (vpn_prefix << vpn_bits) | (idx as u64);
            let node_start_vpn = vpn_prefix << (level * vpn_bits);
            let node_range = VirtPageRange::new(
                Self::canonicalize_vpn(node_start_vpn),
                1u64 << (level * vpn_bits),
            );

            // Prune the traversal tree early when this node's covered range
            // doesn't overlap with the caller requested range.
            if !range.intersects(&node_range) {
                continue;
            }

            if pte.is_branch() {
                assert_ne!(level, 0);
                let ctx = BranchCtx { level };
                match order {
                    TraverseOrder::PreOrder => {
                        match branch_op(pte, ctx) {
                            ControlFlow::Continue => {},
                            r @ ControlFlow::Break(..) => return r,
                        }
                        match unsafe {
                            Self::do_traverse(
                                pte.ppn(),
                                range,
                                branch_op,
                                leaf_op,
                                order,
                                level - 1,
                                vpn_prefix,
                            )
                        } {
                            ControlFlow::Continue => {},
                            r @ ControlFlow::Break(..) => return r,
                        }
                    },
                    TraverseOrder::PostOrder => {
                        match unsafe {
                            Self::do_traverse(
                                pte.ppn(),
                                range,
                                branch_op,
                                leaf_op,
                                order,
                                level - 1,
                                vpn_prefix,
                            )
                        } {
                            ControlFlow::Continue => {},
                            r @ ControlFlow::Break(..) => return r,
                        }
                        match branch_op(pte, ctx) {
                            ControlFlow::Continue => {},
                            r @ ControlFlow::Break(..) => return r,
                        }
                    },
                }
            } else {
                assert!(pte.is_leaf());

                // `vpn_prefix` only contains visited indices. For huge-page leaves,
                // lower-level indices are implicit zeros, so we shift them back.
                let ctx = LeafCtx {
                    vpn: Self::canonicalize_vpn(node_start_vpn),
                    level,
                };
                match leaf_op(pte, ctx) {
                    ControlFlow::Continue => {},
                    r @ ControlFlow::Break(..) => return r,
                }
            }
        }

        ControlFlow::Continue
    }

    /// Map a single page at level `level_at`.
    ///
    /// * If `level_at` is zero, then the page will be mapped as a normal page.
    ///
    /// * If `level_at` is greater than zero, then the page will be mapped as a
    ///   huge page. **Note that this won't check whether the architecture
    ///   supports huge pages at this level**
    ///
    /// This is the most primitive mapping operation, and it won't do any check
    /// on the existing mapping if `overwrite` is true.
    ///
    /// # Safety
    /// The caller must guarantee that:
    ///
    ///     * The `vpn` is aligned to the page size corresponding to `level_at`.
    ///     * The `level_at` is valid and less than the max level of the paging
    ///       architecture.
    ///     * The `flags` are valid for the paging architecture.
    ///
    /// # Returns
    ///     * `Ok(())` if the mapping is successful.
    ///     * `Err(SysError::AlreadyMapped)` if the target page is already
    ///       mapped and `overwrite` is false.
    ///     * `Err(SysError::OutOfMemory)` if the mapping requires allocation of
    ///       new page tables and the allocation fails.
    pub unsafe fn map_one(
        &mut self,
        vpn: VirtPageNum,
        ppn: PhysPageNum,
        flags: PteFlags,
        level_at: usize,
        overwrite: bool,
    ) -> Result<(), SysError> {
        unsafe {
            self.map_one_commit(vpn, ppn, flags, level_at, overwrite)
                .map(|_| ())
        }
    }

    /// Commit one ordinary leaf mapping and report its old/new relation.
    ///
    /// This is the narrow fault-resolution surface. Classification is derived
    /// during the owning page-table walk, including any valid ancestor that the
    /// overwrite replaces, so callers do not need a second translation walk.
    ///
    /// # Safety
    ///
    /// The caller must provide architecture-supported leaf flags and complete
    /// any local and remote TLB invalidation required by the returned relation.
    pub(crate) unsafe fn commit_leaf(
        &mut self,
        vpn: VirtPageNum,
        ppn: PhysPageNum,
        flags: PteFlags,
    ) -> Result<LeafPteCommit, SysError> {
        unsafe { self.map_one_commit(vpn, ppn, flags, 0, true) }
    }

    unsafe fn map_one_commit(
        &mut self,
        vpn: VirtPageNum,
        ppn: PhysPageNum,
        flags: PteFlags,
        level_at: usize,
        overwrite: bool,
    ) -> Result<LeafPteCommit, SysError> {
        if !flags.is_supported_rwx_combination() {
            return Err(SysError::InvalidArgument);
        }
        let levels = PagingArch::PAGE_LEVELS;
        let mut replaced_valid_ancestor = false;

        // Check level_at value
        debug_assert!(
            level_at < levels,
            "Invalid `level_at` argument: greater than max level id {}: {}",
            levels - 1,
            level_at
        );

        let vpn_bits = PagingArch::PTE_PER_PGDIR.trailing_zeros() as usize;

        // Check alignment
        #[cfg(debug_assertions)]
        {
            let level_offset = level_at * vpn_bits;
            debug_assert!(
                vpn.get() & ((1 << level_offset) - 1) == 0,
                "Invalid `vpn` argument: not aligned to the specified level: level_at={}, vpn={:#x}",
                level_at,
                vpn.get()
            );
        }
        let mut pgdir = unsafe {
            self.root
                .to_phys_addr()
                .to_hhdm()
                .as_ptr_mut::<PgDir>()
                .as_mut()
                .expect("root ppn should not be null")
        };

        for level in (0..levels).rev() {
            let idx = (vpn.get() as usize >> (level * vpn_bits)) & (PagingArch::PTE_PER_PGDIR - 1);
            let pte = &mut pgdir[idx];

            if level == level_at {
                // leaf reached
                if pte.is_valid() && !overwrite {
                    return Err(SysError::AlreadyMapped);
                }
                let old_flags = pte.flags() & !PteFlags::VALID;
                let commit = if replaced_valid_ancestor {
                    LeafPteCommit::ReplacedOrRestricted
                } else if !pte.is_valid() {
                    LeafPteCommit::Added
                } else if pte.is_leaf() && pte.ppn() == ppn && old_flags == flags {
                    LeafPteCommit::Unchanged
                } else if pte.is_leaf() && pte.ppn() == ppn && flags.contains(old_flags) {
                    LeafPteCommit::Relaxed
                } else {
                    LeafPteCommit::ReplacedOrRestricted
                };
                *pte = Pte::new(ppn, flags | PteFlags::VALID, level);
                return Ok(commit);
            } else {
                // branch
                if !pte.is_branch() {
                    if pte.is_leaf() && !overwrite {
                        // huge page exists.
                        return Err(SysError::AlreadyMapped);
                    }
                    replaced_valid_ancestor |= pte.is_valid();

                    // allocate a new pgdir

                    // no need to undo all previous frame allocations if this fails,
                    // as once OutOfMemory is returned, the callerwill kill the process and thus
                    // all allocated frames will be deallocated automatically.
                    let new_pgdir_ppn = alloc_frame_zeroed().ok_or(SysError::OutOfMemory)?.leak();

                    *pte = Pte::new(
                        new_pgdir_ppn,
                        if flags.contains(PteFlags::GLOBAL) {
                            PteFlags::VALID | PteFlags::GLOBAL
                        } else {
                            PteFlags::VALID
                        },
                        level,
                    );
                }
                pgdir = unsafe {
                    pte.ppn()
                        .to_phys_addr()
                        .to_hhdm()
                        .as_ptr_mut::<PgDir>()
                        .as_mut()
                        .expect("pgdir ppn should not be null")
                };
            }
        }
        unreachable!("page-table level count must be nonzero")
    }

    /// Change the flags of a single page at level `level_at`.
    pub fn change_flags_one<F: FnMut(PteFlags) -> PteFlags>(
        &mut self,
        vpn: VirtPageNum,
        mut op: F,
        level_at: usize,
    ) -> Result<(), SysError> {
        let levels = PagingArch::PAGE_LEVELS;

        // Check level_at value
        debug_assert!(
            level_at < levels,
            "Invalid `level_at` argument: greater than max level id {}: {}",
            levels - 1,
            level_at
        );

        let vpn_bits = PagingArch::PTE_PER_PGDIR.trailing_zeros() as usize;

        // Check alignment
        #[cfg(debug_assertions)]
        {
            let level_offset = level_at * vpn_bits;
            debug_assert!(
                vpn.get() & ((1 << level_offset) - 1) == 0,
                "Invalid `vpn` argument: not aligned to the specified level: level_at={}, vpn={:#x}",
                level_at,
                vpn.get()
            );
        }
        let mut pgdir = unsafe {
            self.root
                .to_phys_addr()
                .to_hhdm()
                .as_ptr_mut::<PgDir>()
                .as_mut()
                .expect("root ppn should not be null")
        };

        for level in (0..levels).rev() {
            let idx = (vpn.get() as usize >> (level * vpn_bits)) & (PagingArch::PTE_PER_PGDIR - 1);
            let pte = &mut pgdir[idx];

            if level == level_at {
                // leaf reached
                if pte.is_valid() {
                    unsafe {
                        pte.set_flags(op(pte.flags()));
                    }
                    break;
                } else {
                    return Err(SysError::NotMapped);
                }
            } else {
                // branch
                if !pte.is_branch() {
                    return Err(SysError::AlreadyMapped);
                }
                pgdir = unsafe {
                    pte.ppn()
                        .to_phys_addr()
                        .to_hhdm()
                        .as_ptr_mut::<PgDir>()
                        .as_mut()
                        .expect("pgdir ppn should not be null")
                };
            }
        }
        Ok(())
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn test_flags() -> PteFlags {
        PteFlags::READ | PteFlags::WRITE | PteFlags::USER
    }

    fn frame() -> OwnedFrameHandle {
        alloc_frame_zeroed().expect("mapper KUnit frame allocation should succeed")
    }

    unsafe fn map_page(mapper: &mut Mapper<'_>, vpn: VirtPageNum, frame: &OwnedFrameHandle) {
        unsafe {
            mapper
                .map_one(vpn, frame.ppn(), test_flags(), 0, false)
                .expect("mapper KUnit page mapping should succeed");
        }
    }

    unsafe fn withdraw_global_mapping(mapper: &mut Mapper<'_>, range: VirtPageRange) {
        unsafe {
            match mapper.traverse::<_, _, ()>(
                range,
                |pte, _| {
                    pte.set_flags(pte.flags() - PteFlags::GLOBAL);
                    ControlFlow::Continue
                },
                |pte, _| {
                    pte.set_flags(pte.flags() - PteFlags::GLOBAL);
                    ControlFlow::Continue
                },
                TraverseOrder::PreOrder,
            ) {
                ControlFlow::Continue => {},
                ControlFlow::Break(()) => unreachable!(),
            }
            mapper.try_unmap(Unmapping { range });
        }
    }

    #[kunit]
    fn unmap_single_page_reclaims_empty_tables() {
        let mut table = PageTable::new().expect("mapper KUnit page table should allocate");
        let frame = frame();
        let vpn = VirtPageNum::new(1);
        let mut mapper = table.mapper();

        unsafe { map_page(&mut mapper, vpn, &frame) };
        assert_eq!(
            mapper.translate(vpn).expect("page should be mapped").ppn,
            frame.ppn()
        );

        unsafe {
            mapper.try_unmap(Unmapping {
                range: VirtPageRange::new(vpn, 1),
            });
        }
        assert!(mapper.translate(vpn).is_none());
    }

    #[kunit]
    fn unmap_can_transfer_empty_tables_to_remote_retirement() {
        let mut table = PageTable::new().expect("mapper KUnit page table should allocate");
        let frame = frame();
        let vpn = VirtPageNum::new(1);
        let range = VirtPageRange::new(vpn, 1);
        let mut mapper = table.mapper();

        unsafe { map_page(&mut mapper, vpn, &frame) };
        let mut retired = Vec::new();
        unsafe {
            mapper.try_unmap_retiring_page_tables(Unmapping { range }, &mut retired);
        }

        assert!(mapper.translate(vpn).is_none());
        assert!(!retired.is_empty());
        drop(retired);
    }

    #[kunit]
    fn traversal_crosses_leaf_table_boundary() {
        let mut table = PageTable::new().expect("mapper KUnit page table should allocate");
        let first = frame();
        let second = frame();
        let first_vpn = VirtPageNum::new(PagingArch::PTE_PER_PGDIR as u64 - 1);
        let second_vpn = first_vpn + 1;
        let mut mapper = table.mapper();

        unsafe {
            map_page(&mut mapper, first_vpn, &first);
            map_page(&mut mapper, second_vpn, &second);
            mapper.try_unmap(Unmapping {
                range: VirtPageRange::new(first_vpn, 2),
            });
        }

        assert!(mapper.translate(first_vpn).is_none());
        assert!(mapper.translate(second_vpn).is_none());
    }

    #[kunit]
    fn change_flags_visits_each_cross_boundary_leaf_once() {
        let mut table = PageTable::new().expect("mapper KUnit page table should allocate");
        let first = frame();
        let second = frame();
        let first_vpn = VirtPageNum::new(PagingArch::PTE_PER_PGDIR as u64 - 1);
        let second_vpn = first_vpn + 1;
        let mut mapper = table.mapper();
        let mut changed = 0;

        unsafe {
            map_page(&mut mapper, first_vpn, &first);
            map_page(&mut mapper, second_vpn, &second);
            mapper.change_flags(
                VirtPageRange::new(first_vpn, 2),
                |_, flags| {
                    changed += 1;
                    Some(flags - PteFlags::WRITE)
                },
                TraverseOrder::PreOrder,
            );
        }

        assert_eq!(changed, 2);
        for vpn in [first_vpn, second_vpn] {
            let flags = mapper
                .translate(vpn)
                .expect("page should remain mapped")
                .flags;
            assert!(flags.contains(PteFlags::READ));
            assert!(!flags.contains(PteFlags::WRITE));
        }
        drop(mapper);
        drop(table);
    }

    #[kunit]
    fn huge_leaf_survives_partial_range_and_unmaps_when_covered() {
        let mut table = PageTable::new().expect("mapper KUnit page table should allocate");
        let huge_pages = PagingArch::PTE_PER_PGDIR as u64;
        let vpn = VirtPageNum::new(huge_pages * 4);
        let mut mapper = table.mapper();

        unsafe {
            mapper
                .map_one(vpn, PhysPageNum::new(0), PteFlags::READ, 1, false)
                .expect("mapper KUnit huge mapping should succeed");
            mapper.try_unmap(Unmapping {
                range: VirtPageRange::new(vpn + 1, 1),
            });
        }
        assert!(mapper.translate(vpn + 1).is_some());

        unsafe {
            mapper.try_unmap(Unmapping {
                range: VirtPageRange::new(vpn, huge_pages),
            });
        }
        assert!(mapper.translate(vpn).is_none());
    }

    #[kunit]
    fn global_leaf_is_not_unmapped() {
        let mut table = PageTable::new().expect("mapper KUnit page table should allocate");
        let frame = frame();
        let vpn = VirtPageNum::new(7);
        let range = VirtPageRange::new(vpn, 1);
        let mut mapper = table.mapper();

        unsafe {
            mapper
                .map_one(vpn, frame.ppn(), test_flags() | PteFlags::GLOBAL, 0, false)
                .expect("mapper KUnit global mapping should succeed");
            mapper.try_unmap(Unmapping { range });
        }
        assert!(mapper.translate(vpn).is_some());

        // Restore non-global ownership before cleanup. Global branch entries
        // intentionally survive ordinary unmap, so the test must withdraw the
        // test-only global publication before PageTable::drop.
        unsafe { withdraw_global_mapping(&mut mapper, range) };
        assert!(mapper.translate(vpn).is_none());
    }

    #[cfg(target_arch = "loongarch64")]
    #[kunit]
    fn global_huge_leaf_translation_ignores_huge_global_bit() {
        let mut table = PageTable::new().expect("mapper KUnit page table should allocate");
        let huge_pages = PagingArch::PTE_PER_PGDIR as u64;
        let vpn = VirtPageNum::new(huge_pages * 4);
        let ppn = PhysPageNum::new(huge_pages * 6);
        let range = VirtPageRange::new(vpn, huge_pages);
        let mut mapper = table.mapper();

        unsafe {
            mapper
                .map_one(vpn, ppn, PteFlags::READ | PteFlags::GLOBAL, 1, false)
                .expect("mapper KUnit global huge mapping should succeed");
        }
        assert_eq!(
            mapper
                .translate(vpn)
                .expect("huge base should be mapped")
                .ppn,
            ppn
        );
        assert_eq!(
            mapper
                .translate(vpn + 37)
                .expect("huge offset should be mapped")
                .ppn,
            ppn + 37
        );

        unsafe { withdraw_global_mapping(&mut mapper, range) };
        assert!(mapper.translate(vpn).is_none());
    }

    #[kunit]
    fn failed_range_map_rolls_back_only_new_pages() {
        let mut table = PageTable::new().expect("mapper KUnit page table should allocate");
        let first = frame();
        let collision = frame();
        let vpn = VirtPageNum::new(32);
        let mut mapper = table.mapper();

        unsafe { map_page(&mut mapper, vpn + 1, &collision) };
        assert_eq!(
            mapper.map(Mapping {
                vpn,
                ppn: first.ppn(),
                flags: test_flags(),
                npages: 2,
                huge_pages: false,
            }),
            Err(SysError::AlreadyMapped)
        );

        assert!(mapper.translate(vpn).is_none());
        assert_eq!(
            mapper
                .translate(vpn + 1)
                .expect("collision mapping must remain")
                .ppn,
            collision.ppn()
        );
        drop(mapper);
        drop(table);
    }

    #[kunit]
    fn canonical_high_half_traversal_sign_extends_vpn() {
        let vpn_bits = PagingArch::PAGE_LEVELS * PagingArch::PGDIR_IDX_BITS;
        let effective_vpn_bits = u64::BITS as usize - PagingArch::PAGE_SIZE_BITS;
        let high_half_start = 1u64 << (vpn_bits - 1);
        let effective_mask = (1u64 << effective_vpn_bits) - 1;
        let low_mask = (1u64 << vpn_bits) - 1;
        let high_vpn = Mapper::canonicalize_vpn(high_half_start);

        assert_eq!(high_vpn.get(), effective_mask & !low_mask | high_half_start);
        assert_eq!(Mapper::canonicalize_vpn(low_mask).get(), effective_mask);

        let mut table = PageTable::new().expect("mapper KUnit page table should allocate");
        let frame = frame();
        let mut mapper = table.mapper();

        unsafe {
            map_page(&mut mapper, high_vpn, &frame);
        }
        assert_eq!(
            mapper
                .translate(high_vpn)
                .expect("canonical high-half page should be mapped")
                .ppn,
            frame.ppn()
        );
        unsafe {
            mapper.try_unmap(Unmapping {
                range: VirtPageRange::new(high_vpn, 1),
            });
        }
        assert!(mapper.translate(high_vpn).is_none());
    }

    #[kunit]
    fn leaf_commit_classifies_additive_and_present_changes() {
        let mut table = PageTable::new().expect("mapper KUnit page table should allocate");
        let first = frame();
        let second = frame();
        let vpn = VirtPageNum::new(64);
        let read = PteFlags::READ | PteFlags::USER;
        let write = read | PteFlags::WRITE;
        let mut mapper = table.mapper();

        assert_eq!(
            unsafe { mapper.commit_leaf(vpn, first.ppn(), read) }
                .expect("additive leaf commit should succeed"),
            LeafPteCommit::Added
        );
        assert_eq!(
            unsafe { mapper.commit_leaf(vpn, first.ppn(), read) }
                .expect("unchanged leaf commit should succeed"),
            LeafPteCommit::Unchanged
        );
        assert_eq!(
            unsafe { mapper.commit_leaf(vpn, first.ppn(), write) }
                .expect("permission relaxation should succeed"),
            LeafPteCommit::Relaxed
        );
        assert_eq!(
            unsafe { mapper.commit_leaf(vpn, first.ppn(), read) }
                .expect("permission restriction should succeed"),
            LeafPteCommit::ReplacedOrRestricted
        );
        assert_eq!(
            unsafe { mapper.commit_leaf(vpn, second.ppn(), read) }
                .expect("frame replacement should succeed"),
            LeafPteCommit::ReplacedOrRestricted
        );
    }

    #[kunit]
    fn leaf_commit_does_not_hide_replaced_valid_ancestor() {
        let mut table = PageTable::new().expect("mapper KUnit page table should allocate");
        let frame = frame();
        let huge_pages = PagingArch::PTE_PER_PGDIR as u64;
        let huge_vpn = VirtPageNum::new(huge_pages * 8);
        let mut mapper = table.mapper();

        unsafe {
            mapper
                .map_one(huge_vpn, PhysPageNum::new(0), PteFlags::READ, 1, false)
                .expect("mapper KUnit huge mapping should succeed");
        }
        assert_eq!(
            unsafe { mapper.commit_leaf(huge_vpn + 1, frame.ppn(), test_flags()) }
                .expect("leaf replacement below the huge mapping should succeed"),
            LeafPteCommit::ReplacedOrRestricted
        );
    }
}
