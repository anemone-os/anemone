//! ELF image loader utilities for constructing a [UserSpace].
//!
//! The main helper [load_image_from_elf] parses an ELF binary and builds a
//! [UserTaskImage] that contains the created [UserSpace], the ELF entry
//! point and the argv-like command vector.
use elf::{ElfBytes, ParseError, abi, endian::AnyEndian};

use crate::{mm::layout::KernelLayoutTrait, prelude::*};

/// Result of loading an ELF into a new address space.
///
/// - memsp: constructed [UserSpace] with segments mapped
/// - entry: ELF entry point
/// - command: argv-like strings
pub struct UserTaskImage {
    pub memsp: UserSpace,
    pub entry: u64,
}

/// Parse `data` as an ELF file and produce a [UserTaskImage].
///
/// Validates PT_LOAD segments, creates a [UserSpace] via [UserSpace::new_user],
/// maps pages and copies segment data using [UserSpace::add_segment]. Returns
/// [ElfLoadError] on parse, segment validation, or memory errors.
pub fn load_image_from_elf(data: &[u8]) -> Result<UserTaskImage, ElfLoadError> {
    let elf_bytes =
        ElfBytes::<AnyEndian>::minimal_parse(data).map_err(|e| ElfLoadError::Parse(e))?;
    let entry = elf_bytes.ehdr.e_entry;
    let segment_headers = elf_bytes
        .segments()
        .ok_or(ElfLoadError::Segment(SegmentError::HeaderNotFound))
        .map_err(|e| ElfLoadError::Segment(SegmentError::HeaderNotFound))?;
    struct SegData<'a> {
        data: &'a [u8],
        vaddr: VirtAddr,
        filesz: u64,
        memsz: u64,
        rwx_flags: PteFlags,
    }
    let mut segments = vec![];
    let mut heap_start = VirtAddr::new(0);
    for seg_header in segment_headers {
        if seg_header.p_type != abi::PT_LOAD {
            // ignore
            continue;
        }
        let data = elf_bytes
            .segment_data(&seg_header)
            .map_err(|e| ElfLoadError::Parse(e))?;
        let filesz = seg_header.p_filesz;
        let memsz = seg_header.p_memsz;
        let vaddr = VirtAddr::new(seg_header.p_vaddr);
        let vaddr_end = vaddr + memsz;
        if vaddr_end < vaddr || vaddr_end.get() > KernelLayout::KSPACE_ADDR {
            return Err(ElfLoadError::Segment(SegmentError::InvalidSegmentData));
        }
        if vaddr_end > heap_start {
            // update heap start
            heap_start = vaddr_end;
        }
        let mut rwx_flags = PteFlags::empty();
        if seg_header.p_flags & abi::PF_R != 0 {
            rwx_flags |= PteFlags::READ;
        }
        if seg_header.p_flags & abi::PF_W != 0 {
            rwx_flags |= PteFlags::WRITE;
        }
        if seg_header.p_flags & abi::PF_X != 0 {
            rwx_flags |= PteFlags::EXECUTE;
        }
        if rwx_flags.is_empty() || !rwx_flags.is_supported_rwx_combination() {
            return Err(ElfLoadError::Mm(MmError::InvalidArgument));
        }
        let segdata = SegData {
            data,
            filesz,
            memsz,
            vaddr,
            rwx_flags,
        };
        segments.push(segdata);
    }
    let mut usersp = UserSpace::new_user().map_err(|e| ElfLoadError::Mm(e))?;
    for segment in &segments {
        unsafe {
            usersp
                .add_segment(
                    segment.vaddr,
                    segment.memsz as usize,
                    segment.filesz as usize,
                    segment.data,
                    segment.rwx_flags,
                )
                .map_err(|e| ElfLoadError::Mm(e))?;
        }
    }
    kdebugln!("ELF loaded: entry = {:#x}, heap_start = {:#x}", entry, heap_start.get());
    Ok(UserTaskImage {
        memsp: usersp,
        entry,
    })
}

#[derive(Debug)]
pub enum ElfLoadError {
    Parse(ParseError),
    Mm(MmError),
    Segment(SegmentError),
}

#[derive(Debug)]
pub enum SegmentError {
    HeaderNotFound,
    InvalidSegmentData,
}
