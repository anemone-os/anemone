use anemone_abi::syscall::SYS_CONNECT;

use crate::{
    fs::{
        iomux::PollEvent,
        socket::{SocketConnectError, socket_from_file, wait_for_socket_operation},
    },
    prelude::*,
    task::files::{Fd, FileStatusFlags},
};

use super::abi::read_socket_connect_address;

#[syscall(SYS_CONNECT)]
fn sys_connect(fd: Fd, addr: u64, addrlen: u32) -> Result<u64, SysError> {
    let task = get_current_task();
    let desc = task.get_fd(fd)?;
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    let address = read_socket_connect_address(socket.socket_type(), addr, addrlen)?;
    let nonblocking = desc.file_flags().contains(FileStatusFlags::NONBLOCK);

    loop {
        match socket.connect(address.clone()) {
            Ok(()) => return Ok(0),
            Err(SocketConnectError::WouldBlock(_)) if nonblocking => {
                return Err(SysError::Again);
            },
            Err(SocketConnectError::WouldBlock(wait)) => {
                // The capability can only subscribe/recheck listener capacity
                // and client lifetime. The next attempt repeats pathname lookup
                // and DAC instead of reusing admission authority across sleep.
                wait_for_socket_operation("sys_connect", &task, &wait, PollEvent::WRITABLE)?;
            },
            Err(SocketConnectError::Unsupported) => return Err(SysError::NotSupported),
            Err(SocketConnectError::Retired) => return Err(SysError::BadFileDescriptor),
            Err(SocketConnectError::InvalidState) => return Err(SysError::InvalidArgument),
            Err(SocketConnectError::AlreadyConnected) => {
                return Err(SysError::AlreadyConnected);
            },
            Err(SocketConnectError::ConnectionRefused) => {
                return Err(SysError::ConnectionRefused);
            },
            Err(SocketConnectError::ProtocolTypeMismatch) => {
                return Err(SysError::ProtocolTypeMismatch);
            },
            Err(SocketConnectError::Operation(error)) => return Err(error),
        }
    }
}
