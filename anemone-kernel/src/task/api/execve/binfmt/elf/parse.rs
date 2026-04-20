//! Parse ELF binaries and load them into [UserSpace]

use core::{ffi::CStr, mem::MaybeUninit};

use goblin::{
    elf::header::{EI_CLASS, EI_DATA, ELFCLASS64, ELFDATA2LSB, ET_DYN, ET_EXEC},
    elf64::{
        header::{Header, SIZEOF_EHDR},
        program_header::*,
    },
};

use crate::{
    mm::{
        layout::KernelLayoutTrait,
        uspace::vmo::{VmObject, anon::AnonObject},
    },
    prelude::{
        vma::{ForkPolicy, Protection, VmArea, VmFlags},
        *,
    },
    utils::data::FileDataSource,
};

/// randomly chosen. should refine this to really randomize it per process.
///
/// If multiple PIE binaries are loaded into the same address space(e.g. pie
/// program and its interpreter/ld.so), they should be layered on top of each
/// other, thus avoiding conflicts.
const DYN_LOAD_BIAS: u64 = 0x390000;

#[derive(Debug)]
pub struct ElfMeta {
    /// entry point.
    ///
    /// **Note that this is always the entry point of the main program, even
    /// though there does exist an interpreter.**
    pub entry: VirtAddr,

    // most of following fields are for auxv's sake.
    /// the address of the Elf program headers, used for auxv AT_PHDR
    pub phdrs_addr: VirtAddr,
    /// the size of each Elf program header entry, used for auxv AT_PHENT
    pub phdr_entry_sz: usize,
    /// the number of Elf program header entries, used for auxv AT_PHNUM
    pub phdr_entry_num: usize,
    /// for dynamicly linked Elves.
    pub interp: Option<InterpreterMeta>,
}

#[derive(Debug)]
pub struct InterpreterMeta {
    /// the entry point of the Elf interpreter (almost always a dynamic linker).
    ///
    /// for dynamicly linked Elf binaries, execve should jump here instead of
    /// the main program's entry.
    pub entry: VirtAddr,
    /// the load bias of the Elf interpreter. this is the value for auxv
    /// AT_BASE.
    pub base: VirtAddr,
}

/// just some basic validation. passing this does not guarantee the ELF is
/// well-formed or supported.
fn validate_elf(hdr: &Header) -> Result<&Header, SysError> {
    if hdr.e_ident[0..4] != [0x7F, b'E', b'L', b'F'] {
        return Err(SysError::InvalidArgument);
    }

    if hdr.e_ident[EI_CLASS] != ELFCLASS64 {
        // only support 64-bit ELF
        return Err(SysError::InvalidArgument);
    }
    if hdr.e_ident[EI_DATA] != ELFDATA2LSB {
        // only support little-endian ELF
        return Err(SysError::InvalidArgument);
    }

    #[cfg(target_arch = "riscv64")]
    {
        use goblin::elf::header::EM_RISCV;

        if hdr.e_machine != EM_RISCV {
            return Err(SysError::InvalidArgument);
        }
    }
    #[cfg(target_arch = "loongarch64")]
    {
        use goblin::elf::header::EM_LOONGARCH;

        if hdr.e_machine != EM_LOONGARCH {
            return Err(SysError::InvalidArgument);
        }
    }

    Ok(hdr)
}

/// Some elfs don't contain a PHDR segment, we should calculate by hand.
///
/// Note this will not add load_bias.
fn find_phdrs_vaddr(elf_hdr: &Header, phdrs: &[ProgramHeader]) -> Option<VirtAddr> {
    let mut phdrs_vaddr = None;

    let phdrs_sz = elf_hdr.e_phentsize as usize * elf_hdr.e_phnum as usize;

    for phdr in phdrs {
        if phdr.p_type == PT_PHDR {
            phdrs_vaddr = Some(VirtAddr::new(phdr.p_vaddr));
            break;
        }

        if phdr.p_type == PT_LOAD {
            let seg_off = phdr.p_offset as usize;
            // tbh idk why we don't use memsz here.
            let seg_filesz = phdr.p_filesz as usize;

            if seg_off <= elf_hdr.e_phoff as usize
                && elf_hdr.e_phoff as usize + phdrs_sz <= seg_off + seg_filesz
            {
                phdrs_vaddr = Some(VirtAddr::new(
                    phdr.p_vaddr + elf_hdr.e_phoff - seg_off as u64,
                ));
                break;
            }
        }
    }

    phdrs_vaddr
}

struct SegData {
    offset: usize,
    filesz: usize,
    vaddr: VirtAddr,
    memsz: usize,
    prot: Protection,
}

impl SegData {
    // also basic validation.
    fn validate(&self) -> Result<(), SysError> {
        if self.filesz > self.memsz {
            return Err(SysError::InvalidArgument);
        }

        let vaddr_end = VirtAddr::new(self.vaddr.get().wrapping_add(self.memsz as u64));
        if vaddr_end < self.vaddr || vaddr_end.get() > KernelLayout::KSPACE_ADDR {
            return Err(SysError::InvalidArgument);
        }
        Ok(())
    }

    fn file_vaddr_end(&self) -> VirtAddr {
        self.vaddr + self.filesz as u64
    }

    fn vaddr_end(&self) -> VirtAddr {
        self.vaddr + self.memsz as u64
    }

    fn rounded_vpn_range(&self) -> VirtPageRange {
        let start = self.vaddr.page_down();
        let end = self.vaddr_end().page_up();
        VirtPageRange::new(start, end - start)
    }
}

/// This module mainly focuses on merging loadable segments into load chunks.
///
/// Linkers do not guarantee that every segment in an ELF file is page-aligned;
/// rather, they only ensure that the starting virtual address and file offset
/// are congruent modulo the page size. Therefore, it is entirely possible for
/// multiple segments to reside within a single page. And in that case, the
/// permissions of that page should be the union of permissions of all segments
/// within it.
mod seg_chunk {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct LoadPageRun {
        range: VirtPageRange,
        prot: Protection,
    }

    pub struct LoadChunk {
        range: VirtPageRange,
        prot: Protection,
        backing: AnonObject,
    }

    impl LoadChunk {
        fn new(run: LoadPageRun) -> Self {
            let backing = AnonObject::new(run.range.npages() as usize);
            Self {
                range: run.range,
                prot: run.prot,
                backing,
            }
        }

        pub fn into_vma(self) -> VmArea {
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

    /// Given the segments, collect the load page runs, which are continuous
    /// page ranges with the same permissions after merging all segments.
    ///
    /// This function will union the permissions of overlapping segments. And
    /// **it will merge adjacent page runs with the same permissions, even
    /// though they originate from different segments.**
    fn collect_load_page_runs(segments: &[SegData]) -> Vec<LoadPageRun> {
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

    /// Simply write those filesz bytes from the segment to the corresponding
    /// load chunks.
    fn write_segment_data(
        source: &File,
        seg: &SegData,
        chunks: &[LoadChunk],
    ) -> Result<(), SysError> {
        if seg.filesz == 0 {
            return Ok(());
        }

        let seg_file_end = seg.file_vaddr_end();

        for chunk in chunks {
            let copy_start = seg
                .vaddr
                .get()
                .max(chunk.range.start().to_virt_addr().get());
            let copy_end = seg_file_end
                .get()
                .min(chunk.range.end().to_virt_addr().get());

            if copy_start >= copy_end {
                continue;
            }

            (&chunk.backing as &dyn VmObject).write_from_data_source(
                (copy_start - chunk.range.start().to_virt_addr().get()) as usize,
                &FileDataSource::new(source, seg.offset + (copy_start - seg.vaddr.get()) as usize),
                (copy_end - copy_start) as usize,
            )?;
        }

        Ok(())
    }

    /// What this module mainly does: given the segments, collect load chunks,
    /// which can then be inserted to user's memory space.
    pub fn collect_load_chunks(
        file: &File,
        segments: &[SegData],
    ) -> Result<Vec<LoadChunk>, SysError> {
        let runs = collect_load_page_runs(segments);
        let chunks = runs.into_iter().map(LoadChunk::new).collect::<Vec<_>>();

        for seg in segments {
            // let source = FileDataSource::new(file, seg.offset);
            write_segment_data(file, seg, &chunks)?;
        }

        Ok(chunks)
    }

    #[cfg(feature = "kunit")]
    mod kunits {
        use super::*;

        fn seg(vaddr: u64, memsz: usize, prot: Protection) -> SegData {
            SegData {
                offset: 0,
                filesz: 0,
                vaddr: VirtAddr::new(vaddr),
                memsz,
                prot,
            }
        }

        #[kunit]
        fn merges_overlapping_pages_per_page_protection() {
            let page_sz = PagingArch::PAGE_SIZE_BYTES as u64;
            let base = VirtAddr::new(0x400000).page_down();

            let runs = collect_load_page_runs(&[
                seg(
                    0x400000,
                    page_sz as usize,
                    Protection::READ | Protection::EXECUTE,
                ),
                seg(
                    0x400000 + page_sz - 0x80,
                    0x200,
                    Protection::READ | Protection::WRITE,
                ),
            ]);

            assert_eq!(
                runs,
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
}
use seg_chunk::*;

/// During this process, rolling back will not be performed if any error is
/// encountered, thus leaving the [UserSpace] in a possibly inconsistent state.
pub unsafe fn load_image(file: &File, usp: &mut UserSpaceData) -> Result<ElfMeta, SysError> {
    let mut elf_hdr_bytes = [0; SIZEOF_EHDR];
    file.read_exact(&mut elf_hdr_bytes)?;
    let elf_hdr = validate_elf(Header::from_bytes(&elf_hdr_bytes))?;

    let load_bias: u64 = if elf_hdr.e_type == ET_EXEC {
        0
    } else if elf_hdr.e_type == ET_DYN {
        // for pie programs, we can load it anywhere.
        DYN_LOAD_BIAS
    } else {
        knoticeln!("unsupported ELF type: {}", elf_hdr.e_type);
        return Err(SysError::InvalidArgument);
    };

    let phdrs_offset = elf_hdr.e_phoff as usize;
    let phdr_entry_sz = elf_hdr.e_phentsize as usize;
    let phdr_entry_num = elf_hdr.e_phnum as usize;

    if phdr_entry_sz != size_of::<ProgramHeader>() {
        knoticeln!(
            "unexpected ELF program header entry size: {}",
            phdr_entry_sz
        );
        return Err(SysError::InvalidArgument);
    }
    if !phdrs_offset.is_multiple_of(align_of::<ProgramHeader>()) {
        knoticeln!(
            "ELF program headers offset is not aligned: {:#x}",
            phdrs_offset
        );
        return Err(SysError::InvalidArgument);
    }

    let phdrs = {
        let mut phdrs =
            vec![MaybeUninit::<ProgramHeader>::uninit(); phdr_entry_num].into_boxed_slice();

        {
            let mut raw_bytes = unsafe {
                core::slice::from_raw_parts_mut(
                    phdrs.as_mut_ptr().cast::<u8>(),
                    phdr_entry_sz * phdr_entry_num,
                )
            };

            file.seek(phdrs_offset)?;
            file.read_exact(raw_bytes)?;
        }

        let ptr = Box::into_raw(phdrs) as *mut [ProgramHeader];
        unsafe { Box::from_raw(ptr) }
    };

    let mut dyn_interp = None;
    let mut interp_bias = load_bias;
    let mut segments = vec![];
    for phdr in &phdrs {
        // biased virtual address.
        let vaddr = VirtAddr::new(phdr.p_vaddr + load_bias);

        if phdr.p_type == PT_INTERP {
            if dyn_interp.is_some() {
                knoticeln!("multiple PT_INTERP segments found");
                return Err(SysError::InvalidArgument);
            }

            let mut buf = vec![0u8; phdr.p_filesz as usize];
            file.seek(phdr.p_offset as usize)?;
            file.read_exact(buf.as_mut())?;
            let cstr = CStr::from_bytes_until_nul(&buf).map_err(|_| SysError::InvalidArgument)?;
            let interp = cstr
                .to_str()
                .map_err(|_| SysError::InvalidArgument)?
                .to_string();
            knoticeln!("dynamic linker found: {}", interp);
            dyn_interp = Some(interp);

            // actually PT_INTERP is not loadable, so this continue is unnecessary, but just
            // for clarity.
            continue;
        }
        // PHDR segment is not PT_LOAD, but according to elf spec, it should be
        // contained in some loadable segment.
        if phdr.p_type != PT_LOAD {
            continue;
        }

        let mut prot = Protection::empty();
        let flags_raw = phdr.p_flags;
        if flags_raw & PF_R != 0 {
            prot |= Protection::READ;
        }
        if flags_raw & PF_W != 0 {
            prot |= Protection::WRITE;
        }
        if flags_raw & PF_X != 0 {
            prot |= Protection::EXECUTE;
        }

        interp_bias = align_up_power_of_2!(
            interp_bias.max(vaddr.get().wrapping_add(phdr.p_memsz)),
            PagingArch::PAGE_SIZE_BYTES
        ) as u64;

        let seg = SegData {
            offset: phdr.p_offset as usize,
            filesz: phdr.p_filesz as usize,
            vaddr,
            memsz: phdr.p_memsz as usize,
            prot,
        };
        seg.validate()?;

        segments.push(seg);
    }

    let Some(phdrs_addr) = find_phdrs_vaddr(elf_hdr, phdrs.as_ref()) else {
        return Err(SysError::InvalidArgument);
    };
    // apply bias.
    let phdrs_addr = phdrs_addr + load_bias;

    let load_chunks = collect_load_chunks(file, &segments)?;

    for chunk in load_chunks {
        unsafe {
            usp.add_segment(chunk.into_vma())?;
        }
    }

    let entry = VirtAddr::new(elf_hdr.e_entry + load_bias);

    if let Some(interp_path) = dyn_interp {
        let interp = load_interpreter(&vfs_open(&interp_path)?, usp, interp_bias)?;

        kdebugln!(
            "ELF loaded: entry = {:#x}, interpreter = {} at {:#x} (base {:#x})",
            entry.get(),
            interp_path,
            interp.entry.get(),
            interp.base.get()
        );

        Ok(ElfMeta {
            entry,
            phdrs_addr,
            phdr_entry_sz,
            phdr_entry_num,
            interp: Some(InterpreterMeta {
                entry: interp.entry,
                base: interp.base,
            }),
        })
    } else {
        kdebugln!("ELF loaded: entry = {:#x}", entry.get());

        Ok(ElfMeta {
            entry,
            phdrs_addr,
            phdr_entry_sz,
            phdr_entry_num,
            interp: None,
        })
    }
}

/// Returns entry address of the dynamic linker.
///
/// The logic is mostly the same as [load_image].
fn load_interpreter(
    file: &File,
    usp: &mut UserSpaceData,
    load_bias: u64,
) -> Result<InterpreterMeta, SysError> {
    let mut elf_hdr_bytes = [0; SIZEOF_EHDR];
    file.read_exact(&mut elf_hdr_bytes)?;
    let elf_hdr = validate_elf(Header::from_bytes(&elf_hdr_bytes))?;

    let load_bias: u64 = if elf_hdr.e_type == ET_EXEC {
        kwarningln!("dynamic linker is a non-PIE executable");
        return Err(SysError::InvalidArgument);
    } else if elf_hdr.e_type == ET_DYN {
        load_bias
    } else {
        knoticeln!("unsupported ELF type: {}", elf_hdr.e_type);
        return Err(SysError::InvalidArgument);
    };

    let phdrs_offset = elf_hdr.e_phoff as usize;
    let phdr_entry_sz = elf_hdr.e_phentsize as usize;
    let phdr_entry_num = elf_hdr.e_phnum as usize;

    if phdr_entry_sz != size_of::<ProgramHeader>() {
        knoticeln!(
            "unexpected ELF program header entry size: {}",
            phdr_entry_sz
        );
        return Err(SysError::InvalidArgument);
    }
    if !phdrs_offset.is_multiple_of(align_of::<ProgramHeader>()) {
        knoticeln!(
            "ELF program headers offset is not aligned: {:#x}",
            phdrs_offset
        );
        return Err(SysError::InvalidArgument);
    }

    let phdrs = {
        let mut phdrs =
            vec![MaybeUninit::<ProgramHeader>::uninit(); phdr_entry_num].into_boxed_slice();

        {
            let mut raw_bytes = unsafe {
                core::slice::from_raw_parts_mut(
                    phdrs.as_mut_ptr().cast::<u8>(),
                    phdr_entry_sz * phdr_entry_num,
                )
            };

            file.seek(phdrs_offset)?;
            file.read_exact(raw_bytes)?;
        }

        let ptr = Box::into_raw(phdrs) as *mut [ProgramHeader];
        unsafe { Box::from_raw(ptr) }
    };

    let mut segments = vec![];
    for phdr in &phdrs {
        let vaddr = VirtAddr::new(phdr.p_vaddr + load_bias);

        if phdr.p_type == PT_INTERP {
            // dynamic linker should be the endgame.
            return Err(SysError::InvalidArgument);
        }
        // PHDR segment is not PT_LOAD, but according to elf spec, it should be
        // contained in some loadable segment.
        if phdr.p_type != PT_LOAD {
            continue;
        }

        let mut prot = Protection::empty();
        let flags_raw = phdr.p_flags;
        if flags_raw & PF_R != 0 {
            prot |= Protection::READ;
        }
        if flags_raw & PF_W != 0 {
            prot |= Protection::WRITE;
        }
        if flags_raw & PF_X != 0 {
            prot |= Protection::EXECUTE;
        }

        let seg = SegData {
            offset: phdr.p_offset as usize,
            filesz: phdr.p_filesz as usize,
            vaddr,
            memsz: phdr.p_memsz as usize,
            prot,
        };
        seg.validate()?;

        segments.push(seg);
    }

    let Some(_phdrs_addr) = find_phdrs_vaddr(elf_hdr, &phdrs) else {
        return Err(SysError::InvalidArgument);
    };

    let load_chunks = collect_load_chunks(file, &segments)?;

    for chunk in load_chunks {
        unsafe {
            usp.add_segment(chunk.into_vma())?;
        }
    }

    let entry = VirtAddr::new(elf_hdr.e_entry + load_bias);
    let base = VirtAddr::new(load_bias);
    kdebugln!(
        "interpreter ELF loaded: entry = {:#x}, base = {:#x}",
        entry.get(),
        base.get()
    );

    Ok(InterpreterMeta { entry, base })
}
