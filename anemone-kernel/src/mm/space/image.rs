//! ELF image loader utilities for constructing a [UserSpace].
//!
//! The main helper [load_image_from_elf] parses an ELF binary and builds a
//! [UserTaskImage] that contains the created [UserSpace], the ELF entry
//! point and the argv-like command vector.
use elf::abi::{PF_W, PF_X, PT_LOAD};
use goblin::{
    elf::program_header::PF_R,
    elf64::{
        header::{Header, SIZEOF_EHDR},
        program_header::ProgramHeader,
    },
};

use crate::{mm::layout::KernelLayoutTrait, prelude::*, utils::data::FileDataSource};

const MAX_UIMAGE_FILE_SZ: u64 = 16 * 1024 * 1024 * 1024; // 16GiB

/// Result of loading an ELF into a new address space.
///
/// - memsp: constructed [UserSpace] with segments mapped
/// - entry: ELF entry point
/// - command: argv-like strings
pub struct UserTaskImage {
    pub memsp: UserSpace,
    pub entry: u64,
}

pub fn load_image_from_file(path: &impl AsRef<str>) -> Result<UserTaskImage, SysError> {
    let file = vfs_open(Path::new(path.as_ref()))?;
    let size = file.get_attr()?.size;
    if size > MAX_UIMAGE_FILE_SZ {
        return Err(MmError::InvalidArgument.into());
    }
    file.seek(0);
    // ELF header
    let mut elf_header_bytes = [0; SIZEOF_EHDR];
    file.read(&mut elf_header_bytes);
    let elf_header = Header::from_bytes(&elf_header_bytes);
    let entry = elf_header.e_entry;
    // Program Headers
    let program_hds_offset = elf_header.e_phoff;
    let program_hds_esize = elf_header.e_phentsize as u64;
    let program_hd_num = elf_header.e_phnum as u64;
    let mut ph_data_boxed = unsafe {
        Box::<[u8]>::new_uninit_slice((program_hds_esize * program_hd_num) as usize).assume_init()
    };
    file.seek(program_hds_offset as usize)?;
    file.read(ph_data_boxed.as_mut())?;
    let headers = unsafe {
        ProgramHeader::from_raw_parts(
            ph_data_boxed.as_ptr() as *const ProgramHeader,
            program_hd_num as usize,
        )
    };
    struct SegData {
        offset: u64,
        filesz: u64,
        vaddr: VirtAddr,
        memsz: u64,
        rwx_flags: PteFlags,
    }
    let mut segments = vec![];
    let mut heap_start = VirtAddr::new(0);
    for header in headers {
        if header.p_type != PT_LOAD{
            continue;
        }
        let offset = header.p_offset;
        let filesz = header.p_filesz;
        let vaddr = VirtAddr::new(header.p_vaddr);
        let memsz = header.p_memsz;
        let vaddr_end = vaddr + memsz;
        if vaddr_end < vaddr || vaddr_end.get() > KernelLayout::KSPACE_ADDR {
            return Err(MmError::InvalidArgument.into());
        }
        if vaddr_end > heap_start {
            // update heap start
            heap_start = vaddr_end;
        }
        let flags_raw = header.p_flags;
        let mut rwx_flags = PteFlags::empty();
        if flags_raw & PF_R != 0 {
            rwx_flags |= PteFlags::READ;
        }
        if flags_raw & PF_W != 0 {
            rwx_flags |= PteFlags::WRITE;
        }
        if flags_raw & PF_X != 0 {
            rwx_flags |= PteFlags::EXECUTE;
        }
        if rwx_flags.is_empty() || !rwx_flags.is_supported_rwx_combination() {
            return Err(MmError::InvalidArgument.into());
        }
        segments.push(SegData {
            offset,
            filesz,
            vaddr,
            memsz,
            rwx_flags,
        });
    }
    let mut usersp = UserSpace::new_user()?;
    for segment in &segments {
        unsafe {
            usersp.add_segment::<SysError>(
                segment.vaddr,
                segment.memsz as usize,
                segment.filesz as usize,
                &FileDataSource::new(&file, segment.offset as usize),
                segment.rwx_flags,
            )?;
        }
    }

    kdebugln!(
        "ELF loaded: entry = {:#x}, heap_start = {:#x}",
        entry,
        heap_start.get()
    );
    Ok(UserTaskImage {
        memsp: usersp,
        entry,
    })
}
