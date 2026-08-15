//! Shadow virtual memory object.

use crate::prelude::{vmo::*, *};

#[derive(Debug, Default)]
struct ShadowPages {
    overlay: BTreeMap<usize, FrameHandle>,
    /// Ranges explicitly decommitted by this shadow. These ranges suppress
    /// parent fallback until a local write installs a new zero-based overlay.
    decommitted: rangemap::RangeSet<usize>,
}

enum ShadowLookup {
    Resident(FrameHandle),
    Decommitted,
    Parent,
}

/// **LOCK ORDERING:**
///
/// **`parent` -> `pages`**
#[derive(Debug)]
pub struct ShadowObject {
    parent: Arc<dyn VmObject>,
    pages: RwLock<ShadowPages>,
}

impl ShadowObject {
    pub fn new(parent: Arc<dyn VmObject>) -> Self {
        Self {
            parent,
            pages: RwLock::new(ShadowPages::default()),
        }
    }

    fn parent_is_exclusive(&self) -> bool {
        Arc::strong_count(&self.parent) == 1
    }

    fn lookup(&self, pidx: usize) -> ShadowLookup {
        let pages = self.pages.read();
        if let Some(frame) = pages.overlay.get(&pidx) {
            assert!(!pages.decommitted.contains(&pidx));
            ShadowLookup::Resident(frame.clone())
        } else if pages.decommitted.contains(&pidx) {
            ShadowLookup::Decommitted
        } else {
            ShadowLookup::Parent
        }
    }

    fn take_overlay(
        &self,
        range: core::ops::Range<usize>,
        mark_decommitted: bool,
        retired: &mut RetiredFrames,
    ) -> Result<(), SysError> {
        if range.start > range.end {
            return Err(SysError::InvalidArgument);
        }
        if range.is_empty() {
            return Ok(());
        }

        let mut pages = self.pages.write();
        if mark_decommitted {
            pages.decommitted.insert(range.clone());
        }
        retire_frame_range(&mut pages.overlay, range, retired);
        Ok(())
    }
}

impl VmObject for ShadowObject {
    fn resolve_frame(&self, pidx: usize, access: PageFaultType) -> Result<ResolvedFrame, SysError> {
        match access {
            PageFaultType::Write => loop {
                let from_decommitted = match self.lookup(pidx) {
                    ShadowLookup::Resident(frame) => {
                        return Ok(ResolvedFrame {
                            frame,
                            writable: true,
                        });
                    },
                    ShadowLookup::Decommitted => true,
                    ShadowLookup::Parent => false,
                };

                let new_frame = if from_decommitted {
                    alloc_frame_zeroed().ok_or(SysError::OutOfMemory)?
                } else {
                    let parent = self.parent.resolve_frame(pidx, PageFaultType::Read)?;
                    let mut new_frame = alloc_frame().ok_or(SysError::OutOfMemory)?;
                    new_frame
                        .as_bytes_mut()
                        .copy_from_slice(parent.frame.as_bytes());
                    new_frame
                };
                let new_frame = unsafe { new_frame.into_frame_handle() };

                let mut pages = self.pages.write();
                if let Some(frame) = pages.overlay.get(&pidx) {
                    return Ok(ResolvedFrame {
                        frame: frame.clone(),
                        writable: true,
                    });
                }
                if pages.decommitted.contains(&pidx) != from_decommitted {
                    drop(pages);
                    continue;
                }

                if from_decommitted {
                    pages.decommitted.remove(pidx..pidx + 1);
                }
                let resolved = ResolvedFrame {
                    frame: new_frame.clone(),
                    writable: true,
                };
                assert!(pages.overlay.insert(pidx, new_frame).is_none());
                return Ok(resolved);
            },
            PageFaultType::Read | PageFaultType::Execute => match self.lookup(pidx) {
                ShadowLookup::Resident(frame) => Ok(ResolvedFrame {
                    frame,
                    writable: true,
                }),
                ShadowLookup::Decommitted => Ok(shared_zero_frame()),
                ShadowLookup::Parent => {
                    let parent = self.parent.resolve_frame(pidx, access)?;
                    match self.lookup(pidx) {
                        ShadowLookup::Resident(frame) => Ok(ResolvedFrame {
                            frame,
                            writable: true,
                        }),
                        ShadowLookup::Decommitted => Ok(shared_zero_frame()),
                        ShadowLookup::Parent => Ok(ResolvedFrame {
                            frame: parent.frame,
                            writable: false,
                        }),
                    }
                },
            },
        }
    }

    fn resolve_frame_ahead(
        &self,
        pidx: usize,
        request_end: usize,
        access: PageFaultType,
    ) -> Result<Option<ResolvedFrame>, SysError> {
        assert!(
            pidx < request_end,
            "fault-ahead request must contain its page"
        );

        match access {
            PageFaultType::Write => Ok(match self.lookup(pidx) {
                // An existing private overlay is already the authoritative
                // write frame. Parent and decommitted cases deliberately
                // decline so speculation never performs COW or consumes a
                // decommit marker before a real write.
                ShadowLookup::Resident(frame) => Some(ResolvedFrame {
                    frame,
                    writable: true,
                }),
                ShadowLookup::Decommitted | ShadowLookup::Parent => None,
            }),
            PageFaultType::Read | PageFaultType::Execute => match self.lookup(pidx) {
                ShadowLookup::Resident(frame) => Ok(Some(ResolvedFrame {
                    frame,
                    writable: true,
                })),
                ShadowLookup::Decommitted => Ok(Some(shared_zero_frame())),
                ShadowLookup::Parent => {
                    // Preserve parent-before-overlay lock ordering. The second
                    // lookup lets a concurrent local write or decommit win over
                    // the parent result exactly as demand resolution does.
                    let parent = self.parent.resolve_frame_ahead(pidx, request_end, access)?;
                    Ok(match self.lookup(pidx) {
                        ShadowLookup::Resident(frame) => Some(ResolvedFrame {
                            frame,
                            writable: true,
                        }),
                        ShadowLookup::Decommitted => Some(shared_zero_frame()),
                        ShadowLookup::Parent => parent.map(|resolved| ResolvedFrame {
                            frame: resolved.frame,
                            writable: false,
                        }),
                    })
                },
            },
        }
    }

    fn discard_range(&self, range: core::ops::Range<usize>, retired: &mut RetiredFrames) {
        assert!(
            range.start <= range.end,
            "VMA-backed discard range must be ordered"
        );
        retire_frame_range(&mut self.pages.write().overlay, range, retired)
    }

    unsafe fn decommit_private_range(
        &self,
        range: core::ops::Range<usize>,
        retired: &mut RetiredFrames,
    ) -> Result<(), SysError> {
        self.take_overlay(range, true, retired)
    }

    fn exclusive_physical_pages(&self, range: core::ops::Range<usize>) -> usize {
        if range.start > range.end {
            return 0;
        }

        let overlay_pages = self
            .pages
            .read()
            .overlay
            .range(range.clone())
            .filter(|(_, frame)| frame.meta().rc() == 1)
            .count();

        if !self.parent_is_exclusive() {
            return overlay_pages;
        }

        // If the parent VMO is only referenced by this shadow object, dropping
        // this address space will drop the parent chain too. Count it after the
        // overlay lock is released so we do not invert the documented
        // parent-before-overlay lock order.
        overlay_pages + self.parent.exclusive_physical_pages(range)
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::mm::uspace::vmo::anon::AnonObject;

    fn fill_frame(frame: &FrameHandle, value: u8) {
        let bytes = unsafe {
            core::slice::from_raw_parts_mut(
                frame.ppn().to_phys_addr().to_hhdm().as_ptr_mut(),
                PagingArch::PAGE_SIZE_BYTES,
            )
        };
        bytes.fill(value);
    }

    #[kunit]
    fn decommit_masks_overlay_and_parent_contents() {
        let parent: Arc<dyn VmObject> = Arc::new(AnonObject::new(1));
        let parent_page = parent
            .resolve_frame(0, PageFaultType::Write)
            .expect("parent write fault should resolve");
        fill_frame(&parent_page.frame, 0xa5);
        drop(parent_page);

        let shadow = ShadowObject::new(parent.clone());
        let overlay = shadow
            .resolve_frame(0, PageFaultType::Write)
            .expect("shadow write fault should copy the parent");
        fill_frame(&overlay.frame, 0x5a);
        drop(overlay);

        // SAFETY: this test owns the only ShadowObject mapping domain and keeps
        // the returned frame handles alive in `retired` for the whole check.
        let mut retired = RetiredFrames::default();
        unsafe { shadow.decommit_private_range(0..1, &mut retired) }
            .expect("shadow decommit should succeed");
        assert_eq!(retired.len(), 1);

        let zero = shadow
            .resolve_frame(0, PageFaultType::Read)
            .expect("decommitted read should resolve the zero frame");
        assert!(!zero.writable);
        assert!(zero.frame.as_bytes().iter().all(|byte| *byte == 0));

        let new_overlay = shadow
            .resolve_frame(0, PageFaultType::Write)
            .expect("decommitted write should allocate a zeroed overlay");
        assert!(new_overlay.writable);
        assert!(new_overlay.frame.as_bytes().iter().all(|byte| *byte == 0));

        let unchanged_parent = parent
            .resolve_frame(0, PageFaultType::Read)
            .expect("parent contents should remain accessible");
        assert!(
            unchanged_parent
                .frame
                .as_bytes()
                .iter()
                .all(|byte| *byte == 0xa5)
        );
    }

    #[kunit]
    fn write_fault_ahead_neither_copies_parent_nor_consumes_decommit() {
        let parent: Arc<dyn VmObject> = Arc::new(AnonObject::new(1));
        parent
            .resolve_frame(0, PageFaultType::Write)
            .expect("parent write fault should resolve");
        let shadow = ShadowObject::new(parent);

        assert!(
            shadow
                .resolve_frame_ahead(0, 1, PageFaultType::Write)
                .unwrap()
                .is_none()
        );
        assert!(matches!(shadow.lookup(0), ShadowLookup::Parent));

        let mut retired = RetiredFrames::default();
        unsafe { shadow.decommit_private_range(0..1, &mut retired) }
            .expect("shadow decommit should succeed");
        assert!(
            shadow
                .resolve_frame_ahead(0, 1, PageFaultType::Write)
                .unwrap()
                .is_none()
        );
        assert!(matches!(shadow.lookup(0), ShadowLookup::Decommitted));
    }
}
