//! madvise system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/madvise.2.html

use anemone_abi::process::linux::mmap;

use crate::prelude::{user_access::user_addr, *};

use super::checked_user_page_range;

enum MadviseBehavior {
    Noop,
    Discard,
    Unsupported,
}

fn madvise_behavior(advice: i32) -> Result<MadviseBehavior, SysError> {
    match advice {
        mmap::MADV_NORMAL
        | mmap::MADV_RANDOM
        | mmap::MADV_SEQUENTIAL
        | mmap::MADV_WILLNEED
        | mmap::MADV_DONTFORK
        | mmap::MADV_DOFORK
        | mmap::MADV_HUGEPAGE
        | mmap::MADV_NOHUGEPAGE
        | mmap::MADV_DONTDUMP
        | mmap::MADV_DODUMP
        | mmap::MADV_COLD
        | mmap::MADV_PAGEOUT => Ok(MadviseBehavior::Noop),
        mmap::MADV_DONTNEED => Ok(MadviseBehavior::Discard),
        mmap::MADV_FREE
        | mmap::MADV_REMOVE
        | mmap::MADV_MERGEABLE
        | mmap::MADV_UNMERGEABLE
        | mmap::MADV_WIPEONFORK
        | mmap::MADV_KEEPONFORK
        | mmap::MADV_DONTNEED_LOCKED
        | mmap::MADV_HWPOISON => Ok(MadviseBehavior::Unsupported),
        _ => Err(SysError::InvalidArgument),
    }
}

#[syscall(SYS_MADVISE)]
fn sys_madvise(
    #[validate_with(user_addr)] addr: VirtAddr,
    size: u64,
    advice: i32,
) -> Result<u64, SysError> {
    let behavior = madvise_behavior(advice)?;
    if addr.page_offset() != 0 {
        return Err(SysError::InvalidArgument);
    }

    let Some(range) = checked_user_page_range(addr, size)? else {
        return Ok(0);
    };

    let usp = get_current_task().clone_uspace_handle();
    usp.validate_mapped_range(range)?;

    match behavior {
        MadviseBehavior::Noop => Ok(0),
        MadviseBehavior::Discard => {
            // let _guard = usp.discard_range(range)?;
            Ok(0)
        },
        MadviseBehavior::Unsupported => Err(SysError::InvalidArgument),
    }
}
