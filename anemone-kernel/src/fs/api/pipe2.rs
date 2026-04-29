//! pipe2 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/pipe2.2.html

use anemone_abi::fs::linux::open::*;

use crate::{
    fs::pipe::{OpenedPipe, create_anonymous_pipe},
    prelude::{dt::UserWritePtr, handler::TryFromSyscallArg, *},
};

bitflags! {
    #[derive(Debug)]
    struct PipeFlags: u32 {
        const O_CLOEXEC = O_CLOEXEC;
        const O_DIRECT = O_DIRECT;
        const O_NONBLOCK = O_NONBLOCK;
        // O_NOTIFICATION_PIPE
    }
}

impl TryFromSyscallArg for PipeFlags {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        if (raw >> 32) != 0 {
            return Err(SysError::InvalidArgument);
        }

        let raw = raw as u32;

        let flags = PipeFlags::from_bits(raw).ok_or(SysError::InvalidArgument)?;

        if flags.intersects(PipeFlags::O_DIRECT | PipeFlags::O_NONBLOCK) {
            return Err(SysError::NotYetImplemented);
        }

        Ok(flags)
    }
}

#[syscall(SYS_PIPE2)]
fn sys_pipe2(pipefd: UserWritePtr<[i32; 2]>, flags: PipeFlags) -> Result<u64, SysError> {
    // O_DIRECT and O_NONBLOCK nyi.
    let fd_flags = if flags.contains(PipeFlags::O_CLOEXEC) {
        FdFlags::CLOSE_ON_EXEC
    } else {
        FdFlags::empty()
    };

    let task = get_current_task();

    let OpenedPipe { rx, tx } = create_anonymous_pipe()?;

    let rx = task
        .open_fd(rx, FileFlags::READ, fd_flags)
        .ok_or(SysError::NoMoreFd)?;
    let tx = task
        .open_fd(tx, FileFlags::WRITE, fd_flags)
        .ok_or(SysError::NoMoreFd)?;

    if let Err(e) = pipefd.safe_write([rx.raw() as i32, tx.raw() as i32]) {
        task.close_fd(rx);
        task.close_fd(tx);
        return Err(e);
    }

    kdebugln!(
        "sys_pipe2: created pipe with rx fd {} and tx fd {}",
        rx.raw(),
        tx.raw()
    );

    Ok(0)
}
