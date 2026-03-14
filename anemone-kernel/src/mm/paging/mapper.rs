use core::{cmp::min, marker::PhantomData, mem::ManuallyDrop};

use crate::prelude::*;

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

/// Mapper. Computation engine for page table traversal and modification.
///
/// **Note that Mapper won't flush the TLB when needed, as should be done by the
/// caller.**
#[derive(Debug)]
pub struct Mapper<'a> {
    root: PhysPageNum,
    _lifetime: PhantomData<&'a mut PageTable>,
}

type PgDir = <PagingArch as PagingArchTrait>::PgDir;
type Pte = <PgDir as PgDirArch>::Pte;

impl Mapper<'_> {
    pub(super) fn new(pgtbl: &mut PageTable) -> Self {
        Self {
            root: pgtbl.root_ppn(),
            _lifetime: PhantomData,
        }
    }

    /// Map a virtual memory region to a physical memory region with the given
    /// flags. Had encountered an already mapped page will cause an error to be
    /// returned, and all the successfully mapped pages will be rolled back.
    pub fn map(&mut self, mapping: Mapping) -> Result<(), MmError> {
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

            fn do_map(&mut self) -> Result<(), MmError> {
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
                                .map_one(next_vpn, next_ppn, flags, level, true)?;
                        }
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
                self.mapper.unmap(Unmapping {
                    range: VirtPageRange::new(self.mapping.vpn, self.mapped_pages as u64),
                });
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
    /// **No atomicity guarantees**
    ///
    /// Unless user can ensure success of the operation, it is
    /// always recommended to first unmap the virtual address and then map
    /// it again, instead of using overwrite mapping directly.
    pub unsafe fn map_overwrite(&mut self, mapping: Mapping) -> Result<(), MmError> {
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
    /// **This method is deliberately designed not to return a [Result] but
    /// always succeed.**
    pub fn unmap(&mut self, unmapping: Unmapping) {
        let Unmapping { range } = unmapping;

        unsafe {
            match self.traverse(
                |pte, _ctx| {
                    let ppn = pte.ppn();
                    let pgdir = ppn
                        .to_phys_addr()
                        .to_hhdm()
                        .as_ptr::<PgDir>()
                        .as_ref()
                        .expect("pgdir ppn should not be null");

                    if pgdir.is_empty() {
                        // deallocate the empty page table
                        let ppn = pte.ppn();
                        *pte = Pte::ZEROED;
                        let _frame = OwnedFrameHandle::from_ppn(ppn);
                    }
                    ControlFlow::<()>::Continue
                },
                |pte, ctx| {
                    if range.contains(ctx.vpn) {
                        if !pte.is_valid() {
                            kwarningln!(
                                "Mapper::unmap: trying to unmap an unmapped page: vpn={:?}",
                                ctx.vpn
                            );
                        }
                        *pte = Pte::ZEROED;
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
                    let page_offset = vpn.get() & !level_mask;
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
enum TraverseOrder {
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
}

impl Mapper<'_> {
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
    ///
    /// TODO: currently we traverse all Ptes, which is inefficient. we should
    /// support traverse within a given vpn range.
    unsafe fn traverse<B, L, R>(
        &mut self,
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
            let vpn_prefix = (vpn_prefix << vpn_bits) | (idx as u64);

            if !pte.is_branch() && !pte.is_leaf() {
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
                assert_eq!(level, 0);
                assert!(pte.is_leaf());
                let ctx = LeafCtx {
                    vpn: VirtPageNum::new(vpn_prefix),
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
    ///     * `Err(MmError::AlreadyMapped)` if the target page is already mapped
    ///       and `overwrite` is false.
    ///     * `Err(MmError::OutOfMemory)` if the mapping requires allocation of
    ///       new page tables and the allocation fails.
    unsafe fn map_one(
        &mut self,
        vpn: VirtPageNum,
        ppn: PhysPageNum,
        flags: PteFlags,
        level_at: usize,
        overwrite: bool,
    ) -> Result<(), MmError> {
        let levels = PagingArch::PAGE_LEVELS;

        /// Check level_at value
        debug_assert!(
            level_at < levels,
            "Invalid `level_at` argument: greater than max level id {}: {}",
            levels - 1,
            level_at
        );

        let vpn_bits = PagingArch::PTE_PER_PGDIR.trailing_zeros() as usize;

        /// Check alignment
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
                    return Err(MmError::AlreadyMapped);
                }
                *pte = Pte::new(ppn, flags | PteFlags::VALID);
                break;
            } else {
                // branch
                if !pte.is_branch() {
                    // allocate a new pgdir

                    // no need to undo all previous frame allocations if this fails,
                    // as once OutOfMemory is returned, the callerwill kill the process and thus
                    // all allocated frames will be deallocated automatically.
                    let new_pgdir_ppn = alloc_frame_zeroed().ok_or(MmError::OutOfMemory)?.leak();

                    *pte = Pte::new(new_pgdir_ppn, PteFlags::VALID);
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
