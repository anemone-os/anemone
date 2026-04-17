//! Parse ELF binaries and load them into [UserSpace]

use goblin::{
    elf::header::{EI_CLASS, EI_DATA, ELFCLASS64, ELFDATA2LSB, ET_DYN, ET_EXEC},
    elf64::{
        header::{Header, SIZEOF_EHDR},
        program_header::*,
    },
};

use crate::{
    mm::layout::KernelLayoutTrait,
    prelude::{vma::Protection, *},
    utils::data::FileDataSource,
};

const MAX_UIMAGE_FILE_SZ: u64 = 16 * 1024 * 1024 * 1024; // 16GiB

/// randomly chosen. should refine this to randomize it per process.
///
/// If multiple PIE binaries are loaded into the same address space(e.g. pie
/// program and its interpreter/ld.so), they should be layered on top of each
/// other, thus avoiding conflicts.
const DYN_LOAD_BIAS: u64 = 0x390000;

#[derive(Debug)]
pub struct ElfMeta {
    /// entry point.
    pub entry: VirtAddr,

    // most of following fields are for auxv's sake.
    /// the address of the ELF program headers, used for auxv AT_PHDR
    pub phdrs_addr: VirtAddr,
    /// the size of each ELF program header entry, used for auxv AT_PHENT
    pub phdr_entry_sz: usize,
    /// the number of ELF program header entries, used for auxv AT_PHNUM
    pub phdr_entry_num: usize,
    /// the address of the ELF interpreter (usually a dynamic linker), if
    /// exists, used for auxv AT_BASE
    pub interpreter_addr: Option<VirtAddr>,
}

/// just some basic validation. passing this does not guarantee the ELF is
/// well-formed or supported.
fn validate_elf(hdr: &Header) -> Result<&Header, SysError> {
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

/// This function does not check magic bytes.
pub fn load_image(file: &File, usp: &mut UserSpaceData) -> Result<ElfMeta, SysError> {
    let size = file.get_attr()?.size;
    if size > MAX_UIMAGE_FILE_SZ {
        return Err(SysError::InvalidArgument);
    }

    let mut elf_hdr_bytes = [0; SIZEOF_EHDR];
    file.read(&mut elf_hdr_bytes)?;
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
    if !phdrs_offset.is_multiple_of(phdr_entry_sz) {
        knoticeln!(
            "ELF program headers offset is not aligned: {:#x}",
            phdrs_offset
        );
        return Err(SysError::InvalidArgument);
    }

    let mut phdrs_vaddr = None;

    let mut phdrs = vec![
        0u8;
        phdr_entry_sz
            .checked_mul(phdr_entry_num)
            .ok_or(SysError::InvalidArgument)?
    ]
    .into_boxed_slice();

    file.seek(phdrs_offset)?;
    file.read(phdrs.as_mut())?;

    // this is really unsafe... refine later.
    let phdrs = unsafe {
        core::slice::from_raw_parts(phdrs.as_ptr().cast::<ProgramHeader>(), phdr_entry_num)
    };

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
            let vaddr_end = VirtAddr::new(self.vaddr.get().wrapping_add(self.memsz as u64));
            if vaddr_end < self.vaddr || vaddr_end.get() > KernelLayout::KSPACE_ADDR {
                return Err(SysError::InvalidArgument);
            }
            Ok(())
        }
    }

    let mut segments = vec![];
    for phdr in phdrs {
        let vaddr = VirtAddr::new(phdr.p_vaddr + load_bias);

        if phdr.p_type == PT_PHDR {
            phdrs_vaddr = Some(vaddr);
        }

        if phdr.p_type == PT_INTERP {
            //return Err(SysError::NotYetImplemented);
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

    let Some(phdrs_addr) = phdrs_vaddr else {
        return Err(SysError::InvalidArgument);
    };

    for seg in &segments {
        unsafe {
            usp.add_segment::<SysError>(
                seg.vaddr,
                seg.memsz,
                seg.filesz,
                &FileDataSource::new(&file, seg.offset),
                seg.prot,
            )?;
        }
    }

    let entry = VirtAddr::new(elf_hdr.e_entry + load_bias);

    kdebugln!("ELF loaded: entry = {:#x}", entry.get());

    Ok(ElfMeta {
        entry,
        phdrs_addr,
        phdr_entry_sz,
        phdr_entry_num,
        interpreter_addr: None, // TODO: dynamic linking
    })
}
