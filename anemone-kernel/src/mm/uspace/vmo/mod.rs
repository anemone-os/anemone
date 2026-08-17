//! Virtual memory object.
//!
//! TODO: currently we use shadow object, which comes from Mach microkernel. But
//! actually Zircon adopts a more flexible approach - hidden object, as an
//! advanced version of shadow object. We may want to switch to that in the
//! future, but for now shadow object is good enough for our use cases.
//!
//! See [fs::address_space] for inode page cache, which is a special kind of
//! VMO.
//!
//! Reference:
//! - https://fuchsia.dev/fuchsia-src/reference/kernel_objects/vm_object

pub mod anon;
pub mod empty;
pub mod fixed;
pub mod shadow;

use core::{fmt::Debug, ops::Range};

use crate::{prelude::*, utils::data::DataSource};

pub fn shared_zero_frame() -> ResolvedFrame {
    static ZERO_FRAME: Lazy<FrameHandle> = Lazy::new(|| unsafe {
        alloc_frame_zeroed()
            .expect("failed to allocate zero frame")
            .into_frame_handle()
    });

    ResolvedFrame {
        frame: ZERO_FRAME.clone(),
        writable: false,
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedFrame {
    pub frame: FrameHandle,
    /// Whether this frame can be mapped writable if the VMA allows write.
    pub writable: bool,
}

/// Frames removed from a VMO but kept alive until the owning address space
/// completes the required TLB invalidation.
#[derive(Debug, Default)]
pub struct RetiredFrames {
    frames: Vec<FrameHandle>,
}

impl RetiredFrames {
    fn push(&mut self, frame: FrameHandle) {
        self.frames.push(frame);
    }

    pub(super) fn len(&self) -> usize {
        self.frames.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

pub(crate) fn retire_frame_range(
    frames: &mut BTreeMap<usize, FrameHandle>,
    range: Range<usize>,
    retired: &mut RetiredFrames,
) {
    while let Some(key) = frames.range(range.clone()).next().map(|(key, _)| *key) {
        retired.push(
            frames
                .remove(&key)
                .expect("selected resident frame must remain present while locked"),
        );
    }
}

/// Interior mutability should be used to implement some methods.
///
/// TODO: explain why such interior mutability is enforced by this trait, and
/// why we don't just make those methods take `&mut self`.
pub trait VmObject: Send + Sync {
    /// Resolve the frame at `pidx` for the given access type.
    ///
    /// `VmObject` are allowed to create a local copy of the frame in this
    /// method, which will be used for the current and future accesses to this
    /// page. This is how copy-on-write is implemented in
    /// [shadow::ShadowObject].
    fn resolve_frame(&self, pidx: usize, access: PageFaultType) -> Result<ResolvedFrame, SysError>;

    /// Optionally resolve one follower in an actual user-fault locality window.
    ///
    /// `request_end` is an exclusive object-page hint. Implementations may use
    /// it to publish additional owner-local clean state, but return authority
    /// only for `pidx`. Returning `None` declines speculation without changing
    /// the demand fault result. In particular, an implementation must not infer
    /// a user write merely because `access` is [`PageFaultType::Write`].
    fn resolve_frame_ahead(
        &self,
        _pidx: usize,
        _request_end: usize,
        _access: PageFaultType,
    ) -> Result<Option<ResolvedFrame>, SysError> {
        Ok(None)
    }

    fn sync_range(&self, _range: core::ops::Range<usize>) -> Result<(), SysError> {
        Ok(())
    }

    /// Retire resident frames for a range already validated against its VMA.
    ///
    /// This handoff is infallible because callers may compose multiple backing
    /// discards into one destructive transaction. Implementations must assert
    /// owner-internal range violations before changing resident state.
    fn discard_range(&self, _range: core::ops::Range<usize>, _retired: &mut RetiredFrames) {
        // `madvise(DONTNEED)` is a hint. Backings that do not support a
        // dedicated discard path can safely ignore it and let the caller drop
        // the current PTEs.
    }

    /// Remove resident frames from a private mapping and return their ownership
    /// to its address-space retirement protocol. Unlike `discard_range`, this
    /// operation must also prevent a COW backing from exposing parent contents
    /// on a later fault.
    ///
    /// # Safety
    ///
    /// The caller must own the complete mapping domain for this object: no
    /// other address space or VMA may retain a mapping of a returned frame. The
    /// returned frames must remain alive until every affected CPU has completed
    /// the corresponding TLB invalidation.
    unsafe fn decommit_private_range(
        &self,
        _range: Range<usize>,
        _retired: &mut RetiredFrames,
    ) -> Result<(), SysError> {
        Err(SysError::NotSupported)
    }

    fn exclusive_physical_pages(&self, _range: core::ops::Range<usize>) -> usize {
        0
    }
}

impl dyn VmObject {
    /// Copy bytes from this object. Each touched page is resolved exactly once.
    pub fn read_bytes(&self, offset: usize, buffer: &mut [u8]) -> Result<(), SysError> {
        let mut remaining = buffer;
        let mut cur_offset = offset;
        while !remaining.is_empty() {
            let pidx = cur_offset >> PagingArch::PAGE_SIZE_BITS;
            let page_offset = cur_offset & (PagingArch::PAGE_SIZE_BYTES - 1);
            let copy_len = remaining
                .len()
                .min(PagingArch::PAGE_SIZE_BYTES - page_offset);

            let resolved = self.resolve_frame(pidx, PageFaultType::Read)?;
            remaining[..copy_len]
                .copy_from_slice(&resolved.frame.as_bytes()[page_offset..page_offset + copy_len]);

            remaining = &mut remaining[copy_len..];
            cur_offset = cur_offset
                .checked_add(copy_len)
                .ok_or(SysError::InvalidArgument)?;
        }

        Ok(())
    }

    /// Copy bytes into this object. Resolving with write access provides the
    /// final frame, so partial-page writes preserve untouched bytes without a
    /// separate read resolution.
    pub fn write_bytes(&self, offset: usize, data: &[u8]) -> Result<(), SysError> {
        let mut remaining = data;
        let mut cur_offset = offset;

        while !remaining.is_empty() {
            let pidx = cur_offset >> PagingArch::PAGE_SIZE_BITS;
            let page_offset = cur_offset & (PagingArch::PAGE_SIZE_BYTES - 1);
            let copy_len = remaining
                .len()
                .min(PagingArch::PAGE_SIZE_BYTES - page_offset);

            let resolved = self.resolve_frame(pidx, PageFaultType::Write)?;
            if !resolved.writable {
                return Err(SysError::PermissionDenied);
            }
            let dst = unsafe {
                core::slice::from_raw_parts_mut(
                    resolved.frame.ppn().to_phys_addr().to_hhdm().as_ptr_mut(),
                    PagingArch::PAGE_SIZE_BYTES,
                )
            };
            dst[page_offset..page_offset + copy_len].copy_from_slice(&remaining[..copy_len]);

            remaining = &remaining[copy_len..];
            cur_offset = cur_offset
                .checked_add(copy_len)
                .ok_or(SysError::InvalidArgument)?;
        }

        Ok(())
    }
    /// Copy data from the given [DataSource] to this [VmObject] at the given
    /// offset.
    pub fn write_from_data_source<S: DataSource<TError = impl Into<SysError>>>(
        &self,
        offset: usize,
        source: &S,
        len: usize,
    ) -> Result<(), SysError> {
        const BUF_SIZE: usize = PagingArch::PAGE_SIZE_BYTES;

        let mut buffer = vec![0u8; BUF_SIZE];
        let mut remaining = len;
        let mut cur_vmo_offset = offset;
        let mut cur_src_offset = 0;
        while remaining > 0 {
            let copy_len = remaining.min(BUF_SIZE);
            source
                .copy_to(cur_src_offset, &mut buffer[..copy_len])
                .map_err(Into::into)?;
            self.write_bytes(cur_vmo_offset, &buffer[..copy_len])?;
            remaining -= copy_len;
            cur_vmo_offset += copy_len;
            cur_src_offset += copy_len;
        }
        Ok(())
    }
}

impl Debug for dyn VmObject {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "VmObject {{ ... }}")
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    struct CountingObject {
        pages: Box<[FrameHandle]>,
        resolutions: RwLock<Vec<(usize, PageFaultType)>>,
        writable: bool,
    }

    impl CountingObject {
        fn new(npages: usize, fill: u8, writable: bool) -> Self {
            let pages = (0..npages)
                .map(|_| {
                    let frame = unsafe {
                        alloc_frame_zeroed()
                            .expect("VMO byte-helper KUnit frame allocation should succeed")
                            .into_frame_handle()
                    };
                    let bytes = unsafe {
                        core::slice::from_raw_parts_mut(
                            frame.ppn().to_phys_addr().to_hhdm().as_ptr_mut(),
                            PagingArch::PAGE_SIZE_BYTES,
                        )
                    };
                    bytes.fill(fill);
                    frame
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            Self {
                pages,
                resolutions: RwLock::new(Vec::new()),
                writable,
            }
        }
    }

    impl VmObject for CountingObject {
        fn resolve_frame(
            &self,
            pidx: usize,
            access: PageFaultType,
        ) -> Result<ResolvedFrame, SysError> {
            let frame = self.pages.get(pidx).ok_or(SysError::NotMapped)?.clone();
            self.resolutions.write().push((pidx, access));
            Ok(ResolvedFrame {
                frame,
                writable: self.writable,
            })
        }
    }

    #[kunit]
    fn byte_helpers_resolve_each_page_once_and_preserve_partial_edges() {
        let page_size = PagingArch::PAGE_SIZE_BYTES;
        let object = CountingObject::new(2, 0x5a, true);
        let vmo: &dyn VmObject = &object;

        vmo.write_bytes(page_size - 2, b"abcd").unwrap();
        assert_eq!(
            *object.resolutions.read(),
            vec![(0, PageFaultType::Write), (1, PageFaultType::Write)]
        );
        assert_eq!(&object.pages[0].as_bytes()[page_size - 4..], b"ZZab");
        assert_eq!(&object.pages[1].as_bytes()[..4], b"cdZZ");

        object.resolutions.write().clear();
        let mut data = [0u8; 4];
        vmo.read_bytes(page_size - 2, &mut data).unwrap();
        assert_eq!(&data, b"abcd");
        assert_eq!(
            *object.resolutions.read(),
            vec![(0, PageFaultType::Read), (1, PageFaultType::Read)]
        );
    }

    #[kunit]
    fn byte_write_rejects_a_nonwritable_resolved_frame() {
        let object = CountingObject::new(1, 0, false);
        let vmo: &dyn VmObject = &object;
        assert_eq!(
            vmo.write_bytes(0, b"x").unwrap_err(),
            SysError::PermissionDenied
        );
        assert_eq!(*object.resolutions.read(), vec![(0, PageFaultType::Write)]);
        assert_eq!(object.pages[0].as_bytes()[0], 0);
    }
}
