use anemone_abi::syscall::SYS_SENDTO;
use anemone_net_api::udp::{UdpPeer, UdpSendError};

use crate::{
    fs::{
        iomux::PollEvent,
        socket::{begin_udp_send, udp_socket_from_file},
    },
    net::udp::SendError,
    prelude::*,
    task::files::{Fd, FileStatusFlags},
};

use super::{
    abi::{map_send_error, read_payload, read_sockaddr_in, validate_message_flags},
    wait_for_udp_file,
};

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
    let mut operation = begin_udp_send(socket).map_err(map_send_error)?;
    let payload = read_payload(buf, len)?;
    let nonblocking = per_call_nonblocking || desc.file_flags().contains(FileStatusFlags::NONBLOCK);
    let peer = UdpPeer::new(address, port);

    loop {
        match operation.send(peer, &payload) {
            Ok(()) => return Ok(len as u64),
            Err(SendError::Stack(UdpSendError::WouldBlock)) if !nonblocking => {},
            Err(error) => return Err(map_send_error(error)),
        }

        // The consumed operation dropped its File guard before the shared
        // wait owner can register or schedule. The kernel payload remains the
        // transaction copy across every current selection/admission retry.
        wait_for_udp_file("sys_sendto", &task, desc.vfs_file(), PollEvent::WRITABLE)?;
        operation = begin_udp_send(socket).map_err(map_send_error)?;
    }
}
