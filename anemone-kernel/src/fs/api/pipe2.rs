//! pipe2 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/pipe2.2.html

use anemone_abi::fs::linux::open::O_CLOEXEC;

use crate::{
    fs::pipe::{OpenedPipe, create_anonymous_pipe},
    prelude::{dt::UserWritePtr, *},
};

#[syscall(SYS_PIPE2)]
fn sys_pipe2(pipefd: UserWritePtr<[i32; 2]>, flags: u32) -> Result<u64, SysError> {
    if flags & !O_CLOEXEC != 0 {
        return Err(KernelError::InvalidArgument.into());
    }

    let fd_flags = FdFlags::from_linux_open_flags(flags);
    let task = clone_current_task();

    let OpenedPipe { rx, tx } = create_anonymous_pipe()?;

    let rx = task.open_fd(rx, FileFlags::READ, fd_flags);
    let tx = task.open_fd(tx, FileFlags::WRITE, fd_flags);

    if let Err(e) = pipefd.safe_write([rx as i32, tx as i32]) {
        task.close_fd(rx);
        task.close_fd(tx);
        return Err(e);
    }

    kdebugln!("sys_pipe2: created pipe with rx fd {} and tx fd {}", rx, tx);

    Ok(0)
}
