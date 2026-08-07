use anemone_abi::syscall::SYS_LISTEN;

use crate::{
    fs::socket::{SocketListenError, socket_from_file},
    prelude::*,
    task::files::Fd,
};

#[syscall(SYS_LISTEN)]
fn sys_listen(fd: Fd, backlog: i32) -> Result<u64, SysError> {
    let desc = get_current_task().get_fd(fd)?;
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    socket.listen(backlog).map_err(|error| match error {
        SocketListenError::Unsupported => SysError::NotSupported,
        SocketListenError::Retired => SysError::BadFileDescriptor,
        SocketListenError::InvalidState => SysError::InvalidArgument,
        SocketListenError::ResourceExhausted => SysError::NoBufferSpace,
    })?;
    Ok(0)
}
