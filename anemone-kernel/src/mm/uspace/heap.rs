//! Program-break policy and heap retirement orchestration.

use super::*;

impl UserSpace {
    /// Get the program break.
    pub fn brk(&self) -> VirtAddr {
        self.heap.brk
    }

    /// Adjust the program break for this address space.
    ///
    /// This function grows or shrinks the reserved heap to make `brk` the new
    /// program break. It returns an error if the requested break is out of
    /// range or if the backing cannot decommit a shrink range.
    pub(super) fn set_brk_inner(
        &mut self,
        brk: VirtAddr,
    ) -> Result<Option<DestructiveUserTlbChange>, SysError> {
        let heap_range = *self.heap_vma().range();

        if brk < heap_range.start().to_virt_addr() {
            return Err(SysError::OutOfMemory); // see reference https://www.man7.org/linux/man-pages/man2/brk.2.html
        }
        if brk > heap_range.end().to_virt_addr() {
            return Err(SysError::OutOfMemory);
        }

        let old_brk_vpn = self.heap.brk.page_up();
        let new_brk_vpn = brk.page_up();
        let guard = if self.heap.brk > brk {
            let count = old_brk_vpn - new_brk_vpn;
            let range = VirtPageRange::new(new_brk_vpn, count);
            if count == 0 {
                None
            } else {
                let mut retirement = UserTlbRetirement::default();
                {
                    let heap_vma = self.heap_vma();
                    let start = heap_vma.vmo_pidx(new_brk_vpn);
                    let end = start
                        .checked_add(count as usize)
                        .ok_or(SysError::InvalidArgument)?;
                    // SAFETY: the reserved heap VMA is created CopyOnWrite and
                    // its backing is not published to another VMA. Fork replaces
                    // it with distinct parent/child ShadowObjects, so this
                    // UserSpace owns the complete mapping domain. `retired` is
                    // transferred to the address-space completion owner below
                    // and stays alive until remote acknowledgement.
                    unsafe {
                        heap_vma
                            .backing()
                            .decommit_private_range(start..end, retirement.frames())?
                    }
                }

                let mut mapper = self.table.mapper();
                unsafe {
                    mapper.try_unmap_retiring_page_tables(
                        Unmapping { range },
                        retirement.page_tables(),
                    );
                }
                for vpn in range.iter() {
                    PagingArch::tlb_shootdown(vpn);
                }

                Some(DestructiveUserTlbChange::new(retirement))
            }
        } else {
            None
        };
        self.heap.brk = brk;
        kdebugln!("brk of {} set to {}", current_task_id(), brk);

        Ok(guard)
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn write_first_byte(ppn: PhysPageNum, value: u8) {
        unsafe {
            *ppn.to_phys_addr().to_hhdm().as_ptr_mut::<u8>() = value;
        }
    }

    fn first_byte(ppn: PhysPageNum) -> u8 {
        unsafe { *ppn.to_phys_addr().to_hhdm().as_ptr::<u8>() }
    }

    #[kunit]
    fn brk_shrink_decommits_full_pages_and_preserves_partial_page() {
        let mut uspace = UserSpace::new().expect("user space setup should succeed");
        let heap_start = uspace.heap.svpn;
        let target_vpn = heap_start + 1;
        let grown_brk = (heap_start + 2).to_virt_addr();

        assert!(
            uspace
                .set_brk_inner(grown_brk)
                .expect("heap growth should succeed")
                .is_none()
        );
        drop(
            uspace
                .resolve_page_access(
                    target_vpn.to_virt_addr(),
                    PageFaultType::Write,
                    PageAccessContinuation::Immediate,
                )
                .expect("heap write fault should allocate a page"),
        );
        let old_ppn = uspace
            .page_table_mut()
            .mapper()
            .translate(target_vpn)
            .expect("faulted heap page should be mapped")
            .ppn;
        write_first_byte(old_ppn, 0xa5);

        let change = uspace
            .set_brk_inner(target_vpn.to_virt_addr())
            .expect("full-page heap shrink should succeed")
            .expect("full-page heap shrink should require remote fencing");
        assert!(
            uspace
                .page_table_mut()
                .mapper()
                .translate(target_vpn)
                .is_none()
        );
        assert_eq!(unsafe { get_frame_raw(old_ppn) }.rc(), 1);

        drop(change);
        assert_eq!(unsafe { get_frame_raw(old_ppn) }.rc(), 0);

        assert!(
            uspace
                .set_brk_inner(grown_brk)
                .expect("heap regrowth should succeed")
                .is_none()
        );
        drop(
            uspace
                .resolve_page_access(
                    target_vpn.to_virt_addr(),
                    PageFaultType::Write,
                    PageAccessContinuation::Immediate,
                )
                .expect("regrown heap page should fault from zero"),
        );
        let regrown_ppn = uspace
            .page_table_mut()
            .mapper()
            .translate(target_vpn)
            .expect("regrown heap page should be mapped")
            .ppn;
        assert_eq!(first_byte(regrown_ppn), 0);
        write_first_byte(regrown_ppn, 0x5a);

        let partial_old = target_vpn.to_virt_addr() + 0x800;
        let partial_new = target_vpn.to_virt_addr() + 0x100;
        assert!(
            uspace
                .set_brk_inner(partial_old)
                .expect("partial-page shrink should succeed")
                .is_none()
        );
        assert!(
            uspace
                .set_brk_inner(partial_new)
                .expect("same-page shrink should succeed")
                .is_none()
        );
        let preserved = uspace
            .page_table_mut()
            .mapper()
            .translate(target_vpn)
            .expect("same-page shrink must preserve the boundary page");
        assert_eq!(preserved.ppn, regrown_ppn);
        assert_eq!(first_byte(preserved.ppn), 0x5a);
    }
}
