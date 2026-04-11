//! mmap system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/mmap.2.html

use crate::{
    mm::uspace::{api::args::*, mmap::AnonymousMapping},
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
        Err(KernelError::NotYetImplemented.into())
    }
}
