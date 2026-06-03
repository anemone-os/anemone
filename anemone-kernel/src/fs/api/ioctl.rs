//! ioctl system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/ioctl.2.html

use anemone_abi::fs::linux::ioctl::FIONREAD;

use crate::{prelude::*, syscall::user_access::UserWritePtr, task::files::Fd};

#[syscall(SYS_IOCTL)]
fn sys_ioctl(fd: Fd, cmd: u32, arg: u64) -> Result<u64, SysError> {
    kdebugln!(
        "sys_ioctl: fd={:?}, cmd={:#x}, arg={:#x}",
        fd,
        cmd,
        arg
    );

    match cmd {
        FIONREAD => {
            let task = get_current_task();
            let file = task.get_fd(fd)?;
            let nbytes = crate::fs::pipe::readable_bytes(file.vfs_file())?;
            let usp = task.clone_uspace_handle();
            let mut guard = usp.lock();
            UserWritePtr::<i32>::try_new(VirtAddr::new(arg), &mut guard)?
                .write(nbytes as i32);
            Ok(0)
        },
        _ => {
            knoticeln!("[NYI] sys_ioctl command {:#x} is not supported yet", cmd);
            Err(SysError::NotYetImplemented)
        },
    }
}
