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
    prelude::{vma::Protection, *},
    task::execve::binfmt::check_exec_permission,
};

use super::segment::{LoadSegment, map_load_segments};

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

    let phdrs_sz = (elf_hdr.e_phentsize as usize).checked_mul(elf_hdr.e_phnum as usize)?;
    let phdrs_end = (elf_hdr.e_phoff as usize).checked_add(phdrs_sz)?;

    for phdr in phdrs {
        if phdr.p_type == PT_PHDR {
            phdrs_vaddr = Some(VirtAddr::new(phdr.p_vaddr));
            break;
        }

        if phdr.p_type == PT_LOAD {
            let seg_off = phdr.p_offset as usize;
            // tbh idk why we don't use memsz here.
            let seg_filesz = phdr.p_filesz as usize;

            let seg_end = seg_off.checked_add(seg_filesz)?;
            if seg_off <= elf_hdr.e_phoff as usize && phdrs_end <= seg_end {
                let phdr_delta = elf_hdr.e_phoff.checked_sub(seg_off as u64)?;
                phdrs_vaddr = Some(VirtAddr::new(phdr.p_vaddr.checked_add(phdr_delta)?));
                break;
            }
        }
    }

    phdrs_vaddr
}

fn read_program_headers(file: &File, elf_hdr: &Header) -> Result<Box<[ProgramHeader]>, SysError> {
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

    let phdrs_len = phdr_entry_sz
        .checked_mul(phdr_entry_num)
        .ok_or(SysError::InvalidArgument)?;
    let phdrs_end = phdrs_offset
        .checked_add(phdrs_len)
        .ok_or(SysError::InvalidArgument)?;
    let file_size = usize::try_from(file.inode().size()).map_err(|_| SysError::FileTooLarge)?;
    if phdrs_end > file_size {
        return Err(SysError::InvalidArgument);
    }

    let mut phdrs = vec![MaybeUninit::<ProgramHeader>::uninit(); phdr_entry_num].into_boxed_slice();
    let raw_bytes =
        unsafe { core::slice::from_raw_parts_mut(phdrs.as_mut_ptr().cast::<u8>(), phdrs_len) };
    file.seek_set_checked(phdrs_offset)?;
    file.read_exact(raw_bytes)?;

    let ptr = Box::into_raw(phdrs) as *mut [ProgramHeader];
    Ok(unsafe { Box::from_raw(ptr) })
}

fn biased_vaddr(vaddr: u64, load_bias: u64) -> Result<VirtAddr, SysError> {
    vaddr
        .checked_add(load_bias)
        .map(VirtAddr::new)
        .ok_or(SysError::InvalidArgument)
}

fn page_align_up(addr: u64) -> Result<u64, SysError> {
    addr.checked_add(PagingArch::PAGE_SIZE_BYTES as u64 - 1)
        .map(|value| value & !(PagingArch::PAGE_SIZE_BYTES as u64 - 1))
        .ok_or(SysError::InvalidArgument)
}

/// During this process, rolling back will not be performed if any error is
/// encountered, thus leaving the [UserSpace] in a possibly inconsistent state.
pub unsafe fn load_image(file: &File, usp: &mut UserSpace) -> Result<ElfMeta, SysError> {
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

    let phdr_entry_sz = elf_hdr.e_phentsize as usize;
    let phdr_entry_num = elf_hdr.e_phnum as usize;
    let phdrs = read_program_headers(file, elf_hdr)?;
    let file_size = usize::try_from(file.inode().size()).map_err(|_| SysError::FileTooLarge)?;

    let mut dyn_interp = None;
    let mut interp_bias = load_bias;
    let mut segments = vec![];
    for phdr in &phdrs {
        // biased virtual address.
        let vaddr = biased_vaddr(phdr.p_vaddr, load_bias)?;

        if phdr.p_type == PT_INTERP {
            if dyn_interp.is_some() {
                knoticeln!("multiple PT_INTERP segments found");
                return Err(SysError::InvalidArgument);
            }

            let mut buf = vec![0u8; phdr.p_filesz as usize];
            file.seek_set_checked(phdr.p_offset as usize)?;
            file.read_exact(buf.as_mut())?;
            let cstr = CStr::from_bytes_until_nul(&buf).map_err(|_| SysError::InvalidArgument)?;
            let interp = cstr
                .to_str()
                .map_err(|_| SysError::InvalidArgument)?
                .to_string();
            kdebugln!("dynamic linker found: {}", interp);
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

        let seg = LoadSegment::new(
            phdr.p_offset as usize,
            phdr.p_filesz as usize,
            vaddr,
            phdr.p_memsz as usize,
            prot,
            file_size,
        )?;

        interp_bias = page_align_up(interp_bias.max(seg.vaddr_end().get()))?;
        segments.push(seg);
    }

    let Some(phdrs_addr) = find_phdrs_vaddr(elf_hdr, phdrs.as_ref()) else {
        return Err(SysError::InvalidArgument);
    };
    // apply bias.
    let phdrs_addr = biased_vaddr(phdrs_addr.get(), load_bias)?;

    map_load_segments(file, &segments, usp)?;

    let entry = biased_vaddr(elf_hdr.e_entry, load_bias)?;

    if let Some(interp_path) = dyn_interp {
        let interp_path =
            get_current_task().lookup_path(Path::new(&interp_path), ResolveFlags::empty())?;
        check_exec_permission(&interp_path)?;
        kdebugln!("loading interpreter from path: {}", interp_path);
        let interp = load_interpreter(&interp_path.open()?, usp, interp_bias)?;

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
    usp: &mut UserSpace,
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

    let phdrs = read_program_headers(file, elf_hdr)?;
    let file_size = usize::try_from(file.inode().size()).map_err(|_| SysError::FileTooLarge)?;

    let mut segments = vec![];
    for phdr in &phdrs {
        let vaddr = biased_vaddr(phdr.p_vaddr, load_bias)?;

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

        let seg = LoadSegment::new(
            phdr.p_offset as usize,
            phdr.p_filesz as usize,
            vaddr,
            phdr.p_memsz as usize,
            prot,
            file_size,
        )?;

        segments.push(seg);
    }

    let Some(_phdrs_addr) = find_phdrs_vaddr(elf_hdr, &phdrs) else {
        return Err(SysError::InvalidArgument);
    };

    map_load_segments(file, &segments, usp)?;

    let entry = biased_vaddr(elf_hdr.e_entry, load_bias)?;
    let base = VirtAddr::new(load_bias);
    kdebugln!(
        "interpreter ELF loaded: entry = {:#x}, base = {:#x}",
        entry.get(),
        base.get()
    );

    Ok(InterpreterMeta { entry, base })
}
