//! mremap system call.

use crate::{
    mm::uspace::mmap::RemapMapping,
    prelude::{
        user_access::{aligned_to, user_addr, SyscallArgValidatorExt},
        *,
    },
};

use super::{args::MremapFlags, checked_page_count};

#[syscall(SYS_MREMAP)]
fn sys_mremap(
    #[validate_with(aligned_to::<{ PagingArch::PAGE_SIZE_BYTES }>.and_then(user_addr))]
    old_addr: VirtAddr,
    old_size: u64,
    new_size: u64,
    flags: MremapFlags,
    #[validate_with(aligned_to::<{ PagingArch::PAGE_SIZE_BYTES }>.and_then(user_addr).nullable())]
    new_addr: Option<VirtAddr>,
) -> Result<u64, SysError> {
    if flags.contains(MremapFlags::MREMAP_DONTUNMAP) {
        kwarningln!("mremap: MREMAP_DONTUNMAP is not supported");
        return Err(SysError::InvalidArgument);
    }

    let may_move = flags.contains(MremapFlags::MREMAP_MAYMOVE);
    let fixed = flags.contains(MremapFlags::MREMAP_FIXED);
    if fixed && !may_move {
        return Err(SysError::InvalidArgument);
    }

    let old_npages = checked_page_count(old_size)?;
    let new_npages = checked_page_count(new_size)?;
    let old_range = VirtPageRange::new(old_addr.page_down(), old_npages as u64);
    let fixed_target = if fixed {
        Some(new_addr.ok_or(SysError::InvalidArgument)?.page_down())
    } else {
        None
    };

    let usp = get_current_task().clone_uspace_handle();
    let (addr, _guard) = usp.remap_range(&RemapMapping {
        old_range,
        new_npages,
        may_move,
        fixed_target,
    })?;

    Ok(addr.get())
}
