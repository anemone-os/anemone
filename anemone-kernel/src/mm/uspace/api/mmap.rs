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
    syscall::dt::*,
};

#[syscall(SYS_MMAP)]
fn sys_mmap(
    #[validate_with(user_addr.nullable())] addr: Option<VirtAddr>,
    #[validate_with(nonzero)] length: u64,
    prot: MmapProt,
    flags: MmapFlags,
    fd: usize,
    #[validate_with(aligned_to::<{ PagingArch::PAGE_SIZE_BYTES }>)] offset: u64,
) -> Result<u64, SysError> {
    let usp = with_current_task(|task| task.clone_uspace().expect("user task should have uspace"));

    let is_anonymous = flags.aux.contains(AuxMmapFlags::MAP_ANONYMOUS);
    let fixed = flags
        .aux
        .intersects(AuxMmapFlags::MAP_FIXED | AuxMmapFlags::MAP_FIXED_NOREPLACE);

    if fixed && addr.is_some_and(|addr| addr.page_offset() != 0) {
        return Err(KernelError::InvalidArgument.into());
    }

    let hint = addr.map(|a| (a.page_down(), fixed));
    let clobber = flags.aux.contains(AuxMmapFlags::MAP_FIXED);
    let npages = align_up_power_of_2!(length, PagingArch::PAGE_SIZE_BYTES) as usize
        >> PagingArch::PAGE_SIZE_BITS;
    let prot: Protection = prot.into();
    let shared = matches!(
        flags.exclusive,
        ExclusiveMmapFlags::Shared | ExclusiveMmapFlags::SharedValidate
    );
    let flags: VmFlags = flags.aux.try_into()?;

    if is_anonymous {
        if offset != 0 {
            return Err(KernelError::InvalidArgument.into());
        }
        // fd should be -1, but it's not forced by POSIX. So we just ignore it if it's
        // not -1.

        let mapping = AnonymousMapping {
            hint,
            clobber,
            npages,
            prot,
            shared,
            flags,
        };

        usp.write()
            .map_anonymous(&mapping)
            .map(|addr| addr.get())
            .map_err(Into::into)
    } else {
        let poffset = offset as usize >> PagingArch::PAGE_SIZE_BITS;
        let file = with_current_task(|task| task.get_fd(fd).ok_or(KernelError::BadFileDescriptor))?;
        let supported_prot = {
            let mut prot = Protection::empty();
            let file_flags = file.file_flags();
            if file_flags.contains(FileFlags::READ) {
                prot |= Protection::READ;
                prot |= Protection::EXECUTE;
            }

            if shared {
                if file_flags.contains(FileFlags::WRITE) {
                    prot |= Protection::WRITE;
                }
            } else {
                // for private mapping, readable file can be mapped with write
                // permission, because the changes will not be written back to
                // the file.
                if file_flags.contains(FileFlags::READ) {
                    prot |= Protection::WRITE;
                }
            }

            prot
        };
        if !supported_prot.contains(prot) {
            return Err(MmError::PermissionDenied.into());
        }

        let inode = file.vfs_file().inode().clone();

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

        usp.write()
            .map_file(&mapping)
            .map(|addr| addr.get())
            .map_err(Into::into)
    }
}
