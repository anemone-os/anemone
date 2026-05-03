//! uname system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/uname.2.html

use anemone_abi::uts::linux::OldUtsName;

use crate::prelude::{
    user_access::{UserWritePtr, user_addr},
    *,
};

fn copy_from_partial(src: &[u8], dst: &mut [u8]) {
    let len = src.len().min(dst.len());
    dst[..len].copy_from_slice(&src[..len]);
}

#[syscall(SYS_UNAME)]
fn sys_uname(#[validate_with(user_addr)] buf: VirtAddr) -> Result<u64, SysError> {
    let mut uname = OldUtsName::ZEROED;

    copy_from_partial(SYSNAME, &mut uname.sysname);
    copy_from_partial(NODENAME, &mut uname.nodename);
    copy_from_partial(RELEASE, &mut uname.release);
    copy_from_partial(VERSION, &mut uname.version);
    copy_from_partial(MACHINE, &mut uname.machine);

    let usp = get_current_task().clone_uspace();
    let mut guard = usp.write();

    let mut buf = UserWritePtr::<OldUtsName>::try_new(buf, &mut guard)?;
    buf.write(uname);

    Ok(0)
}
