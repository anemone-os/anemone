use alloc::vec::Vec;

use crate::{
    fs::{
        iomux::PollEvent,
        socket::{
            SocketSendError, SocketSendPayload, SocketSendRequest, SocketStreamDestination,
            SocketType, send_sigpipe, socket_from_file,
        },
    },
    prelude::*,
    syscall::user_access::user_addr,
    task::files::{Fd, FileStatusFlags},
};
use anemone_abi::syscall::SYS_SENDTO;

use super::{
    abi::{
        map_send_error, read_payload, read_sockaddr_in, validate_raw_socket_address,
        validate_send_message_flags,
    },
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
    let message_flags = validate_send_message_flags(socket.socket_type(), flags)?;
    let nonblocking =
        message_flags.nonblocking || desc.file_flags().contains(FileStatusFlags::NONBLOCK);

    if socket.socket_type() == SocketType::UnixStream {
        let destination = if addr == 0 || addrlen == 0 {
            SocketStreamDestination::Absent
        } else {
            validate_raw_socket_address(addr, addrlen)?;
            SocketStreamDestination::Present
        };
        let segment = if len == 0 {
            None
        } else {
            Some(UserBufferSegment::new(user_addr(buf)?, len))
        };
        let segments = segment.as_ref().map_or(&[][..], core::slice::from_ref);
        let uspace = task.clone_uspace_handle();
        let mut source = UserBufferSource::new(&uspace, segments);

        loop {
            match socket.send(SocketSendRequest::Stream {
                source: &mut source,
                destination,
            }) {
                Ok(sent) => return Ok(sent as u64),
                Err(SocketSendError::WouldBlock) if !nonblocking => {},
                Err(SocketSendError::PeerClosed) => {
                    if !message_flags.no_signal {
                        send_sigpipe();
                    }
                    return Err(SysError::BrokenPipe);
                },
                Err(error) => return Err(map_send_error(error)),
            }
            wait_for_socket_file("sys_sendto", &task, desc.vfs_file(), PollEvent::WRITABLE)?;
        }
    }

    if addr == 0 {
        return Err(SysError::DestinationAddressRequired);
    }
    let peer = read_sockaddr_in(addr, addrlen)?;
    let mut payload = SendPayload {
        address: buf,
        len,
        bytes: None,
    };
    loop {
        match socket.send(SocketSendRequest::Datagram {
            peer: peer.clone(),
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
