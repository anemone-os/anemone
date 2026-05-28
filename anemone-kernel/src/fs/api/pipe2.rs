//! pipe2 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/pipe2.2.html

use anemone_abi::fs::linux::open::*;

use crate::{
    fs::pipe::{OpenedPipe, create_anonymous_pipe},
    prelude::{
        handler::{TryFromSyscallArg, syscall_arg_flag32},
        user_access::{UserWriteSlice, user_addr},
        *,
    },
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
        let raw = syscall_arg_flag32(raw)?;

        let flags = PipeFlags::from_bits(raw).ok_or(SysError::InvalidArgument)?;

        Ok(flags)
    }
}

#[syscall(SYS_PIPE2)]
fn sys_pipe2(
    #[validate_with(user_addr)] pipefd: VirtAddr,
    flags: PipeFlags,
) -> Result<u64, SysError> {
    let fd_flags = if flags.contains(PipeFlags::O_CLOEXEC) {
        FdFlags::CLOSE_ON_EXEC
    } else {
        FdFlags::empty()
    };

    let task = get_current_task();

    let OpenedPipe { rx, tx } = create_anonymous_pipe()?;
    let mut rx_file_flags = FileFlags::READ;
    let mut tx_file_flags = FileFlags::WRITE;
    if flags.contains(PipeFlags::O_NONBLOCK) {
        crate::fs::pipe::update_nonblock(&rx, true);
        crate::fs::pipe::update_nonblock(&tx, true);
        rx_file_flags |= FileFlags::NONBLOCK;
        tx_file_flags |= FileFlags::NONBLOCK;
    }
    if flags.contains(PipeFlags::O_DIRECT) {
        tx_file_flags |= FileFlags::DIRECT;
    }

    let rx = task.open_fd(rx, rx_file_flags, fd_flags)?;
    let tx = task.open_fd(tx, tx_file_flags, fd_flags).map_err(|e| {
        task.close_fd(rx);
        e
    })?;

    let usp = task.clone_uspace_handle();
    let mut guard = usp.lock();
    let mut pipefd = UserWriteSlice::<i32>::try_new(pipefd, 2, &mut guard).map_err(|e| {
        task.close_fd(rx);
        task.close_fd(tx);
        e
    })?;
    pipefd.copy_from_slice(&[rx.raw() as i32, tx.raw() as i32]);

    kdebugln!(
        "sys_pipe2: created pipe with rx fd {} and tx fd {}",
        rx.raw(),
        tx.raw()
    );

    Ok(0)
}
