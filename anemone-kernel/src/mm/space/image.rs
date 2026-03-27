use elf::{ElfBytes, ParseError, abi, endian::AnyEndian};

use crate::{
    mm::layout::KernelLayoutTrait,
    prelude::{user::UserSpace, *},
};

pub fn load_image_from_elf(data: &[u8]) -> Result<(MemSpace, u64), ElfLoadError> {
    let file_raw =
        ElfBytes::<AnyEndian>::minimal_parse(data).map_err(|e| ElfLoadError::Parse(e))?;
    let entry = file_raw.ehdr.e_entry;
    let seg_headers = file_raw
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
    for seg_header in seg_headers {
        if seg_header.p_type != abi::PT_LOAD {
            continue;
        }
        let data = file_raw
            .segment_data(&seg_header)
            .map_err(|e| ElfLoadError::Parse(e))?;
        let filesz = seg_header.p_filesz;
        let memsz = seg_header.p_memsz;
        let vaddr = VirtAddr::new(seg_header.p_vaddr);
        let vaddr_end = vaddr + memsz;
        if vaddr_end < vaddr || vaddr_end.get() > KernelLayout::KSPACE_ADDR {
            return Err(ElfLoadError::Segment(SegmentError::InvalidSegmentData));
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
        if vaddr_end > heap_start {
            heap_start = vaddr_end;
        }
    }
    let memspace = MemSpace::copy_from_kernel();
    let mut table_guard = memspace.table_locked().write_irqsave();
    let mut user_space =
        UserSpace::new(heap_start.page_up(), &mut *table_guard).map_err(|e| ElfLoadError::Mm(e))?;
    for segment in &segments {
        unsafe {
            user_space
                .add_segment(
                    segment.vaddr,
                    segment.memsz as usize,
                    segment.filesz as usize,
                    segment.data,
                    segment.rwx_flags,
                    &mut *table_guard,
                )
                .map_err(|e| ElfLoadError::Mm(e))?;
        }
    }
    drop(table_guard);
    Ok((memspace, entry))
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
