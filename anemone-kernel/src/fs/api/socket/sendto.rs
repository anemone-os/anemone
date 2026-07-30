use anemone_abi::syscall::SYS_SENDTO;
use anemone_net_api::udp::UdpPeer;

use crate::{
    fs::socket::{begin_udp_send, udp_socket_from_file},
    prelude::*,
    task::files::{Fd, FileStatusFlags},
};

use super::abi::{map_send_error, read_payload, read_sockaddr_in, validate_message_flags};

#[syscall(SYS_SENDTO)]
fn sys_sendto(
    fd: Fd,
    buf: u64,
    len: usize,
    flags: i32,
    addr: u64,
    addrlen: u32,
) -> Result<u64, SysError> {
    let task = get_current_task();
    let desc = task.get_fd(fd)?;
    let socket = udp_socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    let per_call_nonblocking = validate_message_flags(flags)?;
    if addr == 0 {
        return Err(SysError::DestinationAddressRequired);
    }
    let (address, port) = read_sockaddr_in(addr, addrlen)?;
    let operation = begin_udp_send(socket).map_err(|error| map_send_error(error, false))?;
    let payload = read_payload(buf, len)?;
    let nonblocking = per_call_nonblocking || desc.file_flags().contains(FileStatusFlags::NONBLOCK);
    operation
        .send(UdpPeer::new(address, port), &payload)
        .map_err(|error| map_send_error(error, nonblocking))?;
    Ok(len as u64)
}
