use anemone_abi::syscall::{SYS_BIND, SYS_GETSOCKNAME};

use crate::{
    fs::socket::{bind_udp_socket, query_udp_socket, udp_socket_from_file},
    prelude::*,
    task::files::Fd,
};

use super::abi::{map_bind_error, map_query_error, read_sockaddr_in, write_sockaddr_in};

#[syscall(SYS_BIND)]
fn sys_bind(fd: Fd, addr: u64, addrlen: u32) -> Result<u64, SysError> {
    let task = get_current_task();
    let desc = task.get_fd(fd)?;
    let socket = udp_socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    let (address, port) = read_sockaddr_in(addr, addrlen)?;
    bind_udp_socket(socket, address, port).map_err(map_bind_error)?;
    Ok(0)
}

#[syscall(SYS_GETSOCKNAME)]
fn sys_getsockname(fd: Fd, addr: u64, addrlen: u64) -> Result<u64, SysError> {
    let task = get_current_task();
    let desc = task.get_fd(fd)?;
    let socket = udp_socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    let (_operation, binding) = query_udp_socket(socket).map_err(map_query_error)?;
    write_sockaddr_in(addr, addrlen, binding)?;
    Ok(0)
}
