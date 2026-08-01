use anemone_abi::syscall::SYS_GETSOCKNAME;

use crate::{
    fs::socket::{SocketAddress, SocketAddressSink, socket_from_file},
    prelude::*,
    task::files::Fd,
};

use super::abi::{map_query_error, write_sockaddr_in};

struct LocalAddressSink {
    addr: u64,
    addrlen: u64,
}

impl SocketAddressSink for LocalAddressSink {
    fn copy_address(&mut self, address: Option<SocketAddress>) -> Result<(), SysError> {
        write_sockaddr_in(self.addr, self.addrlen, address)
    }
}

#[syscall(SYS_GETSOCKNAME)]
fn sys_getsockname(fd: Fd, addr: u64, addrlen: u64) -> Result<u64, SysError> {
    let task = get_current_task();
    let desc = task.get_fd(fd)?;
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    socket
        .copy_local_address(&mut LocalAddressSink { addr, addrlen })
        .map_err(map_query_error)?;
    Ok(0)
}
