use anemone_abi::syscall::SYS_BIND;

use crate::{fs::socket::socket_from_file, prelude::*, task::files::Fd};

use super::abi::{map_bind_error, read_socket_address};

#[syscall(SYS_BIND)]
fn sys_bind(fd: Fd, addr: u64, addrlen: u32) -> Result<u64, SysError> {
    let task = get_current_task();
    let desc = task.get_fd(fd)?;
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    let address = read_socket_address(socket.socket_type(), addr, addrlen)?;
    socket.bind(address).map_err(map_bind_error)?;
    Ok(0)
}
