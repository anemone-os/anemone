use anemone_abi::{
    net::linux::{SHUT_RD, SHUT_RDWR, SHUT_WR},
    syscall::SYS_SHUTDOWN,
};

use crate::{
    fs::socket::{SocketShutdown, SocketShutdownError, socket_from_file},
    prelude::*,
    task::files::Fd,
};

fn shutdown_direction(how: i32) -> Result<SocketShutdown, SysError> {
    match how {
        SHUT_RD => Ok(SocketShutdown::Read),
        SHUT_WR => Ok(SocketShutdown::Write),
        SHUT_RDWR => Ok(SocketShutdown::ReadWrite),
        _ => Err(SysError::InvalidArgument),
    }
}

#[syscall(SYS_SHUTDOWN)]
fn sys_shutdown(fd: Fd, how: i32) -> Result<u64, SysError> {
    let desc = get_current_task().get_fd(fd)?;
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    let how = shutdown_direction(how)?;
    socket.shutdown(how).map_err(|error| match error {
        SocketShutdownError::Unsupported => SysError::NotSupported,
        SocketShutdownError::Retired => SysError::BadFileDescriptor,
        SocketShutdownError::NotConnected => {
            // Current Socket profiles limit shutdown to an existing connected
            // direction. Remove this notice/rejection only when a follow-up
            // target defines one owner for pre-connection shutdown truth and
            // its handoff into listener admission or a new connection.
            knoticeln!("socket: pre-connection shutdown is unsupported; returning ENOTCONN");
            SysError::NotConnected
        },
    })?;
    Ok(0)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn linux_shutdown_values_normalize_before_the_family_owner() {
        assert_eq!(shutdown_direction(SHUT_RD), Ok(SocketShutdown::Read));
        assert_eq!(shutdown_direction(SHUT_WR), Ok(SocketShutdown::Write));
        assert_eq!(shutdown_direction(SHUT_RDWR), Ok(SocketShutdown::ReadWrite));
        assert_eq!(shutdown_direction(-1), Err(SysError::InvalidArgument));
        assert_eq!(shutdown_direction(3), Err(SysError::InvalidArgument));
    }
}
