use anemone_abi::syscall::SYS_GETPEERNAME;

use crate::{
    fs::socket::{SocketAddress, SocketAddressSink, SocketType, socket_from_file},
    prelude::*,
    task::files::Fd,
};

use super::abi::{map_query_error, write_socket_address};

struct PeerAddressSink {
    socket_type: SocketType,
    addr: u64,
    addrlen: u64,
}

impl SocketAddressSink for PeerAddressSink {
    fn copy_address(&mut self, address: Option<SocketAddress>) -> Result<(), SysError> {
        write_socket_address(self.socket_type, self.addr, self.addrlen, address)
    }
}

#[syscall(SYS_GETPEERNAME)]
fn sys_getpeername(fd: Fd, addr: u64, addrlen: u64) -> Result<u64, SysError> {
    let task = get_current_task();
    let desc = task.get_fd(fd)?;
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    socket
        .copy_peer_address(&mut PeerAddressSink {
            socket_type: socket.socket_type(),
            addr,
            addrlen,
        })
        .map_err(map_query_error)?;
    Ok(0)
}
