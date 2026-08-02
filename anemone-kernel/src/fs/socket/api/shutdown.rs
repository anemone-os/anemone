use anemone_abi::{
    net::linux::{SHUT_RD, SHUT_RDWR, SHUT_WR},
    syscall::SYS_SHUTDOWN,
};

use crate::{
    fs::socket::{SocketShutdown, SocketShutdownError, socket_from_file},
    prelude::*,
    task::files::Fd,
};

#[syscall(SYS_SHUTDOWN)]
fn sys_shutdown(fd: Fd, how: i32) -> Result<u64, SysError> {
    let desc = get_current_task().get_fd(fd)?;
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    let how = match how {
        SHUT_RD => SocketShutdown::Read,
        SHUT_WR => SocketShutdown::Write,
        SHUT_RDWR => SocketShutdown::ReadWrite,
        _ => return Err(SysError::InvalidArgument),
    };
    socket.shutdown(how).map_err(|error| match error {
        SocketShutdownError::Unsupported => SysError::NotSupported,
        SocketShutdownError::Retired => SysError::BadFileDescriptor,
        SocketShutdownError::NotConnected => {
            // R1 deliberately limits shutdown to an existing connected
            // direction. Remove this notice/rejection only when a follow-up
            // target defines one owner for pre-connection shutdown truth and
            // its handoff into listener admission or a new connection.
            knoticeln!("socket: pre-connection Unix shutdown is unsupported; returning ENOTCONN");
            SysError::NotConnected
        },
    })?;
    Ok(0)
}
