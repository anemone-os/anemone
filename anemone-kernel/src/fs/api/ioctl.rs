//! ioctl system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/ioctl.2.html

use crate::{
    prelude::*,
    syscall::handler::TryFromSyscallArg,
    task::files::Fd,
};

fn lookup_ioctl_arg_fd(raw_fd: u64) -> Result<IoctlArgFile, SysError> {
    let fd = Fd::try_from_syscall_arg(raw_fd)?;
    let task = get_current_task();
    let file = task.get_fd(fd)?;

    Ok(IoctlArgFile::new(file.vfs_file().clone(), file.ioctl_access()))
}

#[syscall(SYS_IOCTL)]
fn sys_ioctl(fd: Fd, cmd: u32, arg: u64) -> Result<u64, SysError> {
    kdebugln!(
        "sys_ioctl: fd={:?}, cmd={:#x}, arg={:#x}",
        fd,
        cmd,
        arg
    );

    let task = get_current_task();
    let file = task.get_fd(fd)?;
    let target_access = file.ioctl_access();
    if target_access.is_path_only() {
        return Err(SysError::BadFileDescriptor);
    }

    let vfs_file = file.vfs_file().clone();
    let usp = task.clone_uspace_handle();
    drop(file);
    drop(task);

    let arg_fd_lookup = IoctlArgFdLookup::new(lookup_ioctl_arg_fd);
    let ctx = IoctlCtx::new(cmd, arg, target_access, usp, &arg_fd_lookup);
    vfs_file.ioctl(ctx)
}
