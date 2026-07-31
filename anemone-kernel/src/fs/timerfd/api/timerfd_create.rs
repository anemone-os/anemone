use anemone_abi::syscall::SYS_TIMERFD_CREATE;

use crate::{
    fs::timerfd::create_timerfd,
    prelude::*,
    task::files::{FdFlags, FileDescOps, FileStatusFlags, LinuxOpenCompat, OpenAccessMode},
};

use super::TimerFdCreateFlags;

#[syscall(SYS_TIMERFD_CREATE)]
fn sys_timerfd_create(clockid: i32, flags: TimerFdCreateFlags) -> Result<u64, SysError> {
    let file = create_timerfd(clockid)?;

    let mut status_flags = FileStatusFlags::empty();
    status_flags.set(
        FileStatusFlags::NONBLOCK,
        flags.contains(TimerFdCreateFlags::NONBLOCK),
    );
    file.check_status_flags(status_flags.to_file_op_status_flags())?;

    let fd_flags = if flags.contains(TimerFdCreateFlags::CLOEXEC) {
        FdFlags::CLOSE_ON_EXEC
    } else {
        FdFlags::empty()
    };

    let fd = get_current_task().open_fd_with_description_ops(
        file,
        OpenAccessMode::ReadWrite,
        status_flags,
        LinuxOpenCompat::empty(),
        fd_flags,
        FileDescOps {
            notification_suppressed: true,
            ..FileDescOps::default()
        },
    )?;

    Ok(fd.raw() as u64)
}
