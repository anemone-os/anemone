//! Lazy backing for ELF `PT_LOAD` segments.

use crate::{
    mm::layout::KernelLayoutTrait,
    prelude::{
        vma::{ForkPolicy, Protection, VmArea, VmFlags},
        vmo::{ResolvedFrame, RetiredFrames, VmObject, retire_frame_range},
        *,
    },
};

pub(super) struct LoadSegment {
    offset: usize,
    filesz: usize,
    vaddr: VirtAddr,
    memsz: usize,
    prot: Protection,
}

impl LoadSegment {
    pub(super) fn new(
        offset: usize,
        filesz: usize,
        vaddr: VirtAddr,
        memsz: usize,
        prot: Protection,
        file_size: usize,
    ) -> Result<Self, SysError> {
        let file_end = offset
            .checked_add(filesz)
            .ok_or(SysError::InvalidArgument)?;
        let vaddr_end = vaddr
            .get()
            .checked_add(memsz as u64)
            .ok_or(SysError::InvalidArgument)?;

        if filesz > memsz
            || file_end > file_size
            || vaddr_end > KernelLayout::KSPACE_ADDR
            || (offset & (PagingArch::PAGE_SIZE_BYTES - 1))
                != (vaddr.get() as usize & (PagingArch::PAGE_SIZE_BYTES - 1))
        {
            return Err(SysError::InvalidArgument);
        }

        Ok(Self {
            offset,
            filesz,
            vaddr,
            memsz,
            prot,
        })
    }

    pub(super) fn vaddr_end(&self) -> VirtAddr {
        VirtAddr::new(self.vaddr.get() + self.memsz as u64)
    }

    fn file_vaddr_end(&self) -> VirtAddr {
        VirtAddr::new(self.vaddr.get() + self.filesz as u64)
    }

    fn rounded_vpn_range(&self) -> VirtPageRange {
        let start = self.vaddr.page_down();
        if self.memsz == 0 {
            return VirtPageRange::new(start, 0);
        }
        let end = self.vaddr_end().page_up();
        VirtPageRange::new(start, end - start)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LoadPageRun {
    range: VirtPageRange,
    prot: Protection,
}

#[derive(Debug)]
struct PagePiece {
    file_pidx: usize,
    file_offset: usize,
    page_offset: usize,
    len: usize,
}

#[derive(Debug)]
struct PageRecipe {
    pieces: Vec<PagePiece>,
}

impl PageRecipe {
    fn direct_file_page(&self) -> Option<usize> {
        let [piece] = self.pieces.as_slice() else {
            return None;
        };
        (piece.file_offset == 0
            && piece.page_offset == 0
            && piece.len == PagingArch::PAGE_SIZE_BYTES)
            .then_some(piece.file_pidx)
    }
}

/// Process-image-local ELF page composition.
///
/// `source` remains the inode address space and is the only file-page cache.
/// `materialized` contains only pages that ELF rules require to be private:
/// partial/overlapping/BSS pages or a writable page after its first write
/// fault. It must never be consulted as a second source of file truth.
///
/// The source intentionally remains live: until executable-vs-writer
/// accounting closes `ANE-20260528-EXEC-ETXTBSY-WRITER-ACCOUNTING`, concurrent
/// write/truncate may affect resident or later-faulted bytes. This object must
/// not grow a private file cache or eager snapshot as a local workaround.
#[derive(Debug)]
struct ElfLoadObject {
    source: Arc<dyn VmObject>,
    recipes: Box<[PageRecipe]>,
    materialized: RwLock<BTreeMap<usize, FrameHandle>>,
}

impl ElfLoadObject {
    fn new(source: Arc<dyn VmObject>, recipes: Box<[PageRecipe]>) -> Self {
        Self {
            source,
            recipes,
            materialized: RwLock::new(BTreeMap::new()),
        }
    }

    fn recipe(&self, pidx: usize) -> Result<&PageRecipe, SysError> {
        self.recipes.get(pidx).ok_or(SysError::InvalidArgument)
    }

    fn materialize(&self, pidx: usize, recipe: &PageRecipe) -> Result<ResolvedFrame, SysError> {
        if let Some(frame) = self.materialized.read().get(&pidx) {
            return Ok(ResolvedFrame {
                frame: frame.clone(),
                writable: true,
            });
        }

        let mut frame = alloc_frame_zeroed().ok_or(SysError::OutOfMemory)?;
        let mut resolved_sources = Vec::<(usize, FrameHandle)>::new();
        for piece in &recipe.pieces {
            let source = if let Some((_, frame)) = resolved_sources
                .iter()
                .find(|(source_pidx, _)| *source_pidx == piece.file_pidx)
            {
                frame.clone()
            } else {
                let resolved = self
                    .source
                    .resolve_frame(piece.file_pidx, PageFaultType::Read)?;
                resolved_sources.push((piece.file_pidx, resolved.frame.clone()));
                resolved.frame
            };
            frame.as_bytes_mut()[piece.page_offset..piece.page_offset + piece.len].copy_from_slice(
                &source.as_bytes()[piece.file_offset..piece.file_offset + piece.len],
            );
        }

        let frame = unsafe { frame.into_frame_handle() };
        let mut materialized = self.materialized.write();
        let frame = materialized.entry(pidx).or_insert_with(|| frame).clone();
        Ok(ResolvedFrame {
            frame,
            writable: true,
        })
    }
}

impl VmObject for ElfLoadObject {
    fn resolve_frame(&self, pidx: usize, access: PageFaultType) -> Result<ResolvedFrame, SysError> {
        let recipe = self.recipe(pidx)?;
        if let Some(frame) = self.materialized.read().get(&pidx) {
            return Ok(ResolvedFrame {
                frame: frame.clone(),
                writable: true,
            });
        }

        if !matches!(access, PageFaultType::Write)
            && let Some(file_pidx) = recipe.direct_file_page()
        {
            let resolved = self.source.resolve_frame(file_pidx, PageFaultType::Read)?;
            // Executable file pages are never mapped writable. A write fault
            // must return through `materialize` to preserve MAP_PRIVATE-like
            // ELF semantics instead of dirtying the inode address space.
            return Ok(ResolvedFrame {
                frame: resolved.frame,
                writable: false,
            });
        }

        self.materialize(pidx, recipe)
    }

    fn discard_range(&self, range: core::ops::Range<usize>, retired: &mut RetiredFrames) {
        assert!(
            range.start <= range.end && range.end <= self.recipes.len(),
            "VMA-backed discard range must stay within the ELF image"
        );
        retire_frame_range(&mut self.materialized.write(), range, retired)
    }

    fn exclusive_physical_pages(&self, range: core::ops::Range<usize>) -> usize {
        if range.start > range.end || range.start >= self.recipes.len() {
            return 0;
        }
        self.materialized
            .read()
            .range(range.start..range.end.min(self.recipes.len()))
            .filter(|(_, frame)| frame.meta().rc() == 1)
            .count()
    }
}

struct LoadChunk {
    range: VirtPageRange,
    prot: Protection,
    backing: ElfLoadObject,
}

impl LoadChunk {
    fn into_vma(self) -> VmArea {
        VmArea::new(
            self.range,
            0,
            self.prot,
            ForkPolicy::CopyOnWrite,
            VmFlags::empty(),
            Arc::new(self.backing),
        )
    }
}

/// Collect continuous page ranges with the same permissions after unioning all
/// overlapping segment permissions.
fn collect_load_page_runs(segments: &[LoadSegment]) -> Vec<LoadPageRun> {
    let mut page_prots = BTreeMap::new();

    for seg in segments {
        for vpn in seg.rounded_vpn_range().iter() {
            page_prots
                .entry(vpn)
                .and_modify(|prot| *prot |= seg.prot)
                .or_insert(seg.prot);
        }
    }

    let mut runs = Vec::new();
    let mut run_start: Option<VirtPageNum> = None;
    let mut run_prot = Protection::empty();
    let mut prev_vpn: Option<VirtPageNum> = None;

    for (vpn, prot) in page_prots {
        let extend = match prev_vpn {
            Some(prev) => prev + 1 == vpn && run_prot == prot,
            None => false,
        };

        if !extend {
            if let (Some(start), Some(prev)) = (run_start, prev_vpn) {
                runs.push(LoadPageRun {
                    range: VirtPageRange::new(start, prev.get() + 1 - start.get()),
                    prot: run_prot,
                });
            }
            run_start = Some(vpn);
            run_prot = prot;
        }

        prev_vpn = Some(vpn);
    }

    if let (Some(start), Some(prev)) = (run_start, prev_vpn) {
        runs.push(LoadPageRun {
            range: VirtPageRange::new(start, prev.get() + 1 - start.get()),
            prot: run_prot,
        });
    }

    runs
}

fn page_recipe(vpn: VirtPageNum, segments: &[LoadSegment]) -> PageRecipe {
    let page_start = vpn.to_virt_addr().get();
    let page_end = page_start + PagingArch::PAGE_SIZE_BYTES as u64;
    let mut pieces = Vec::new();

    // Preserve program-header order: the eager loader wrote segments in this
    // order, so a later overlapping segment must continue to win byte-for-byte.
    for seg in segments {
        let copy_start = page_start.max(seg.vaddr.get());
        let copy_end = page_end.min(seg.file_vaddr_end().get());
        if copy_start >= copy_end {
            continue;
        }

        let source_offset = seg.offset + (copy_start - seg.vaddr.get()) as usize;
        let file_offset = source_offset & (PagingArch::PAGE_SIZE_BYTES - 1);
        let page_offset = (copy_start - page_start) as usize;
        let len = (copy_end - copy_start) as usize;
        assert!(file_offset + len <= PagingArch::PAGE_SIZE_BYTES);
        assert!(page_offset + len <= PagingArch::PAGE_SIZE_BYTES);
        pieces.push(PagePiece {
            file_pidx: source_offset >> PagingArch::PAGE_SIZE_BITS,
            file_offset,
            page_offset,
            len,
        });
    }

    PageRecipe { pieces }
}

fn collect_load_chunks(source: Arc<dyn VmObject>, segments: &[LoadSegment]) -> Vec<LoadChunk> {
    collect_load_page_runs(segments)
        .into_iter()
        .map(|run| {
            let recipes = run
                .range
                .iter()
                .map(|vpn| page_recipe(vpn, segments))
                .collect::<Vec<_>>()
                .into_boxed_slice();
            LoadChunk {
                range: run.range,
                prot: run.prot,
                backing: ElfLoadObject::new(source.clone(), recipes),
            }
        })
        .collect()
}

/// Install all `PT_LOAD` ranges without resolving their body pages.
pub(super) fn map_load_segments(
    file: &File,
    segments: &[LoadSegment],
    usp: &mut UserSpace,
) -> Result<(), SysError> {
    let source = file.inode().mapping().ok_or(SysError::NotSupported)?;
    for chunk in collect_load_chunks(source, segments) {
        unsafe {
            usp.add_segment(chunk.into_vma())?;
        }
    }
    Ok(())
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::mm::uspace::vmo::shadow::ShadowObject;

    struct CountingObject {
        pages: Box<[FrameHandle]>,
        resolutions: RwLock<Vec<(usize, PageFaultType)>>,
    }

    impl CountingObject {
        fn new(fills: &[u8]) -> Self {
            let pages = fills
                .iter()
                .map(|fill| {
                    let mut frame = alloc_frame_zeroed()
                        .expect("ELF lazy-load KUnit frame allocation should succeed");
                    frame.as_bytes_mut().fill(*fill);
                    unsafe { frame.into_frame_handle() }
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            Self {
                pages,
                resolutions: RwLock::new(Vec::new()),
            }
        }
    }

    impl VmObject for CountingObject {
        fn resolve_frame(
            &self,
            pidx: usize,
            access: PageFaultType,
        ) -> Result<ResolvedFrame, SysError> {
            self.resolutions.write().push((pidx, access));
            Ok(ResolvedFrame {
                frame: self
                    .pages
                    .get(pidx)
                    .ok_or(SysError::InvalidArgument)?
                    .clone(),
                writable: true,
            })
        }
    }

    fn seg(
        offset: usize,
        filesz: usize,
        vaddr: u64,
        memsz: usize,
        prot: Protection,
        file_size: usize,
    ) -> LoadSegment {
        LoadSegment::new(offset, filesz, VirtAddr::new(vaddr), memsz, prot, file_size).unwrap()
    }

    fn write_first_byte(frame: &FrameHandle, byte: u8) {
        unsafe {
            core::slice::from_raw_parts_mut(
                frame.ppn().to_phys_addr().to_hhdm().as_ptr_mut(),
                PagingArch::PAGE_SIZE_BYTES,
            )[0] = byte;
        }
    }

    #[kunit]
    fn construction_is_lazy_and_fault_is_page_local() {
        let page_size = PagingArch::PAGE_SIZE_BYTES;
        let source = Arc::new(CountingObject::new(&[0x11, 0x22, 0x33]));
        let segments = [seg(
            0,
            page_size * 3,
            0x400000,
            page_size * 3,
            Protection::READ | Protection::EXECUTE,
            page_size * 3,
        )];
        let chunks = collect_load_chunks(source.clone(), &segments);

        assert!(source.resolutions.read().is_empty());
        let resolved = chunks[0]
            .backing
            .resolve_frame(1, PageFaultType::Execute)
            .unwrap();
        assert_eq!(resolved.frame.as_bytes()[0], 0x22);
        assert!(!resolved.writable);
        assert_eq!(
            source.resolutions.read().as_slice(),
            &[(1, PageFaultType::Read)]
        );
    }

    #[kunit]
    fn partial_file_page_and_bss_are_zero_filled() {
        let page_size = PagingArch::PAGE_SIZE_BYTES;
        let source = Arc::new(CountingObject::new(&[0x5a, 0x6b]));
        let segments = [seg(
            0x100,
            0x200,
            0x400100,
            page_size + 0x200,
            Protection::READ | Protection::WRITE,
            page_size * 2,
        )];
        let chunks = collect_load_chunks(source, &segments);

        let partial = chunks[0]
            .backing
            .resolve_frame(0, PageFaultType::Read)
            .unwrap();
        assert!(
            partial.frame.as_bytes()[..0x100]
                .iter()
                .all(|byte| *byte == 0)
        );
        assert!(
            partial.frame.as_bytes()[0x100..0x300]
                .iter()
                .all(|byte| *byte == 0x5a)
        );
        assert!(
            partial.frame.as_bytes()[0x300..]
                .iter()
                .all(|byte| *byte == 0)
        );

        let bss = chunks[0]
            .backing
            .resolve_frame(1, PageFaultType::Read)
            .unwrap();
        assert!(bss.frame.as_bytes().iter().all(|byte| *byte == 0));
    }

    #[kunit]
    fn overlapping_segments_compose_bytes_and_union_permissions() {
        let page_size = PagingArch::PAGE_SIZE_BYTES;
        let base = 0x400000;
        let source = Arc::new(CountingObject::new(&[0x11, 0x22, 0x33]));
        let segments = [
            seg(
                0,
                page_size,
                base,
                page_size,
                Protection::READ | Protection::EXECUTE,
                page_size * 3,
            ),
            seg(
                page_size * 2 - 0x80,
                0x100,
                base + page_size as u64 - 0x80,
                0x100,
                Protection::READ | Protection::WRITE,
                page_size * 3,
            ),
        ];
        let chunks = collect_load_chunks(source, &segments);

        assert_eq!(
            chunks[0].prot,
            Protection::READ | Protection::WRITE | Protection::EXECUTE
        );
        let resolved = chunks[0]
            .backing
            .resolve_frame(0, PageFaultType::Read)
            .unwrap();
        assert!(
            resolved.frame.as_bytes()[..page_size - 0x80]
                .iter()
                .all(|byte| *byte == 0x11)
        );
        assert!(
            resolved.frame.as_bytes()[page_size - 0x80..]
                .iter()
                .all(|byte| *byte == 0x22)
        );
    }

    #[kunit]
    fn writable_pages_are_private_between_images() {
        let page_size = PagingArch::PAGE_SIZE_BYTES;
        let source = Arc::new(CountingObject::new(&[0x44]));
        let segments = [seg(
            0,
            page_size,
            0x400000,
            page_size,
            Protection::READ | Protection::WRITE,
            page_size,
        )];
        let first = collect_load_chunks(source.clone(), &segments);
        let second = collect_load_chunks(source.clone(), &segments);

        let shared = first[0]
            .backing
            .resolve_frame(0, PageFaultType::Read)
            .unwrap();
        assert_eq!(shared.frame.ppn(), source.pages[0].ppn());
        assert!(!shared.writable);

        let private = first[0]
            .backing
            .resolve_frame(0, PageFaultType::Write)
            .unwrap();
        assert_ne!(private.frame.ppn(), source.pages[0].ppn());
        assert!(private.writable);
        write_first_byte(&private.frame, 0xaa);
        assert_eq!(source.pages[0].as_bytes()[0], 0x44);
        assert_eq!(
            second[0]
                .backing
                .resolve_frame(0, PageFaultType::Read)
                .unwrap()
                .frame
                .as_bytes()[0],
            0x44
        );
    }

    #[kunit]
    fn fork_shadows_keep_materialized_writes_private() {
        let page_size = PagingArch::PAGE_SIZE_BYTES;
        let source = Arc::new(CountingObject::new(&[0x44]));
        let segments = [seg(
            0,
            page_size,
            0x400000,
            page_size,
            Protection::READ | Protection::WRITE,
            page_size,
        )];
        let mut chunks = collect_load_chunks(source.clone(), &segments);
        let original: Arc<dyn VmObject> = Arc::new(chunks.remove(0).backing);
        let before_fork = original.resolve_frame(0, PageFaultType::Write).unwrap();
        write_first_byte(&before_fork.frame, 0x55);

        // This is the same two-shadow shape installed by `VmArea::fork` for a
        // `ForkPolicy::CopyOnWrite` ELF VMA.
        let parent = ShadowObject::new(original.clone());
        let child = ShadowObject::new(original);
        let parent_read = parent.resolve_frame(0, PageFaultType::Read).unwrap();
        let child_write = child.resolve_frame(0, PageFaultType::Write).unwrap();
        write_first_byte(&child_write.frame, 0x66);

        assert!(!parent_read.writable);
        assert_ne!(parent_read.frame.ppn(), child_write.frame.ppn());
        assert_eq!(parent_read.frame.as_bytes()[0], 0x55);
        assert_eq!(child_write.frame.as_bytes()[0], 0x66);
        assert_eq!(source.pages[0].as_bytes()[0], 0x44);
    }

    #[kunit]
    fn discard_drops_only_the_private_page_and_refaults_from_source() {
        let page_size = PagingArch::PAGE_SIZE_BYTES;
        let source = Arc::new(CountingObject::new(&[0x44]));
        let segments = [seg(
            0,
            page_size,
            0x400000,
            page_size,
            Protection::READ | Protection::WRITE,
            page_size,
        )];
        let chunks = collect_load_chunks(source.clone(), &segments);
        let backing = &chunks[0].backing;
        let private = backing.resolve_frame(0, PageFaultType::Write).unwrap();
        write_first_byte(&private.frame, 0x55);

        let mut retired = RetiredFrames::default();
        backing.discard_range(0..1, &mut retired);
        let refaulted = backing.resolve_frame(0, PageFaultType::Read).unwrap();
        assert_eq!(refaulted.frame.ppn(), source.pages[0].ppn());
        assert_eq!(refaulted.frame.as_bytes()[0], 0x44);
        assert!(!refaulted.writable);
    }

    #[kunit]
    fn merges_overlapping_pages_per_page_protection() {
        let page_size = PagingArch::PAGE_SIZE_BYTES;
        let base = VirtAddr::new(0x400000).page_down();
        let segments = [
            seg(
                0,
                0,
                0x400000,
                page_size,
                Protection::READ | Protection::EXECUTE,
                page_size,
            ),
            seg(
                page_size - 0x80,
                0,
                0x400000 + page_size as u64 - 0x80,
                0x200,
                Protection::READ | Protection::WRITE,
                page_size,
            ),
        ];

        assert_eq!(
            collect_load_page_runs(&segments),
            vec![
                LoadPageRun {
                    range: VirtPageRange::new(base, 1),
                    prot: Protection::READ | Protection::WRITE | Protection::EXECUTE,
                },
                LoadPageRun {
                    range: VirtPageRange::new(base + 1, 1),
                    prot: Protection::READ | Protection::WRITE,
                },
            ]
        );
    }
}
