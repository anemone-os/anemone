//! dup3 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/dup3.2.html

use anemone_abi::fs::linux::open::O_CLOEXEC;

use crate::{
    prelude::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        *,
    },
    task::files::{Fd, FdFlags},
};

struct Dup3FdFlags {
    cloexec: bool,
}

impl TryFromSyscallArg for Dup3FdFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = syscall_arg_flag32(raw)?;

        if raw & !O_CLOEXEC != 0 {
            return Err(SysError::InvalidArgument);
        }

        let cloexec = (raw & O_CLOEXEC) != 0;

        Ok(Self { cloexec })
    }
}

#[syscall(SYS_DUP3)]
fn sys_dup3(oldfd: Fd, newfd: Fd, flags: Dup3FdFlags) -> Result<u64, SysError> {
    let fd_flags = if flags.cloexec {
        FdFlags::CLOSE_ON_EXEC
    } else {
        FdFlags::empty()
    };

    let task = get_current_task();
    task.dup3(oldfd, newfd, fd_flags)
        .map(|newfd| newfd.raw() as u64)
}
