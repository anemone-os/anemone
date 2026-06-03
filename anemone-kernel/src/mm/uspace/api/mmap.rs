//! mmap system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/mmap.2.html

use crate::{
    mm::uspace::{
        api::args::*,
        mmap::{AnonymousMapping, FileMapping},
    },
    prelude::{
        vma::{Protection, VmFlags},
        *,
    },
    syscall::user_access::aligned_to,
    task::files::Fd,
};

#[syscall(SYS_MMAP)]
fn sys_mmap(
    #[validate_with(mmap_addr)] addr: Option<VirtAddr>,
    length: u64,
    prot: MmapProt,
    flags: MmapFlags,
    #[validate_with(mmap_fd)] raw_fd: i32,
    #[validate_with(aligned_to::<{ PagingArch::PAGE_SIZE_BYTES }>)] offset: u64,
) -> Result<u64, SysError> {
    kdebugln!(
        "mmap: addr={:?}, length={}, prot={:?}, flags={:?}, fd={:?}, offset={}",
        addr,
        length,
        prot,
        flags,
        raw_fd,
        offset
    );

    let usp = get_current_task().clone_uspace_handle();

    let is_anonymous = flags.aux.contains(AuxMmapFlags::MAP_ANONYMOUS);
    let fixed = flags
        .aux
        .intersects(AuxMmapFlags::MAP_FIXED | AuxMmapFlags::MAP_FIXED_NOREPLACE);
    let clobber = flags.aux.contains(AuxMmapFlags::MAP_FIXED);

    if fixed && addr.is_some_and(|addr| addr.page_offset() != 0) {
        return Err(SysError::InvalidArgument);
    }

    let prot: Protection = prot.into();
    let shared = matches!(
        flags.exclusive,
        ExclusiveMmapFlags::Shared | ExclusiveMmapFlags::SharedValidate
    );
    let flags: VmFlags = flags.aux.try_into()?;

    if is_anonymous {
        if length == 0 {
            return Err(SysError::InvalidArgument);
        }
        if offset != 0 {
            return Err(SysError::InvalidArgument);
        }
        // fd should be -1, but it's not forced by POSIX. So we just ignore it if it's
        // not -1.

        let hint = addr.map(|a| (a.page_down(), fixed));
        let npages = align_up_power_of_2!(length, PagingArch::PAGE_SIZE_BYTES) as usize
            >> PagingArch::PAGE_SIZE_BITS;
        let mapping = AnonymousMapping {
            hint,
            clobber,
            npages,
            prot,
            shared,
            flags,
        };

        let (addr, _guard) = usp
            .map_anonymous(&mapping)
            .map_err(|err| mmap_error_at_syscall_boundary(err, clobber))?;
        Ok(addr.get())
    } else {
        if raw_fd < 0 {
            return Err(SysError::BadFileDescriptor);
        }

        let poffset = offset as usize >> PagingArch::PAGE_SIZE_BITS;
        let fd = Fd::new(raw_fd as u32).ok_or(SysError::BadFileDescriptor)?;
        let file = get_current_task().get_fd(fd)?;
        if file.is_path_only() {
            return Err(SysError::BadFileDescriptor);
        }
        if length == 0 {
            return Err(SysError::InvalidArgument);
        }
        let supported_prot = {
            let mut prot = Protection::empty();
            if file.can_read() {
                prot |= Protection::READ;
                prot |= Protection::EXECUTE;
            }

            if shared {
                if file.can_write() {
                    prot |= Protection::WRITE;
                }
            } else {
                // for private mapping, readable file can be mapped with write
                // permission, because the changes will not be written back to
                // the file.
                if file.can_read() {
                    prot |= Protection::WRITE;
                }
            }

            prot
        };
        if !file.can_read() || !supported_prot.contains(prot) {
            return Err(SysError::AccessDenied);
        }

        let inode = file.vfs_file().inode().clone();

        let hint = addr.map(|a| (a.page_down(), fixed));
        let npages = align_up_power_of_2!(length, PagingArch::PAGE_SIZE_BYTES) as usize
            >> PagingArch::PAGE_SIZE_BITS;
        let mapping = FileMapping {
            hint,
            clobber,
            npages,
            prot,
            shared,
            flags,
            poffset,
            inode,
        };

        let (addr, _guard) = usp
            .map_file(&mapping)
            .map_err(|err| mmap_error_at_syscall_boundary(err, clobber))?;

        Ok(addr.get())
    }
}

fn mmap_error_at_syscall_boundary(err: SysError, clobber: bool) -> SysError {
    match err {
        SysError::AlreadyMapped if !clobber => SysError::AlreadyExists,
        err => err,
    }
}
