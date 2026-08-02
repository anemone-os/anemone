use alloc::vec::Vec;

use crate::{
    fs::{
        iomux::PollEvent,
        socket::{SocketSendError, SocketSendPayload, SocketSendRequest, socket_from_file},
    },
    prelude::*,
    task::files::{Fd, FileStatusFlags},
};
use anemone_abi::syscall::SYS_SENDTO;

use super::{
    abi::{map_send_error, read_payload, read_sockaddr_in, validate_message_flags},
    wait_for_socket_file,
};

struct SendPayload {
    address: u64,
    len: usize,
    bytes: Option<Vec<u8>>,
}

impl SocketSendPayload for SendPayload {
    fn bytes(&mut self) -> Result<&[u8], SysError> {
        if self.bytes.is_none() {
            self.bytes = Some(read_payload(self.address, self.len)?);
        }
        Ok(self.bytes.as_deref().unwrap())
    }
}

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
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    let per_call_nonblocking = validate_message_flags(flags)?;
    if addr == 0 {
        return Err(SysError::DestinationAddressRequired);
    }
    let peer = read_sockaddr_in(addr, addrlen)?;
    let mut payload = SendPayload {
        address: buf,
        len,
        bytes: None,
    };
    let nonblocking = per_call_nonblocking || desc.file_flags().contains(FileStatusFlags::NONBLOCK);

    loop {
        match socket.send(SocketSendRequest::Datagram {
            peer,
            payload: &mut payload,
        }) {
            Ok(sent) => return Ok(sent as u64),
            Err(SocketSendError::WouldBlock) if !nonblocking => {},
            Err(error) => return Err(map_send_error(error)),
        }

        // The family attempt released its operation guard before the shared
        // wait owner can register or schedule. The kernel payload remains the
        // transaction copy across every current selection/admission retry.
        wait_for_socket_file("sys_sendto", &task, desc.vfs_file(), PollEvent::WRITABLE)?;
    }
}
