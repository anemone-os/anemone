use alloc::vec::Vec;

use crate::{
    fs::socket::{
        SocketDatagramSendOperation, SocketIoOps, SocketSendPayload, SocketSendRequest,
        SocketStreamDestination, SocketType, retry_socket_send, socket_from_file,
    },
    prelude::*,
    syscall::user_access::user_addr,
    task::files::{Fd, FileStatusFlags},
};
use anemone_abi::syscall::SYS_SENDTO;

use super::abi::{
    map_send_error, read_payload, read_socket_address, validate_raw_socket_address,
    validate_send_message_flags,
};

struct SendPayload {
    address: u64,
    len: usize,
    bytes: Option<Vec<u8>>,
}

impl SocketSendPayload for SendPayload {
    fn bytes(&mut self, maximum: usize) -> Result<&[u8], SysError> {
        if self.bytes.is_none() {
            self.bytes = Some(read_payload(self.address, self.len, maximum)?);
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

    if matches!(
        socket.socket_type(),
        SocketType::UnixStream | SocketType::UnixSeqpacket
    ) {
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
        let operation_wait = match socket.io() {
            SocketIoOps::ByteStream { .. } => None,
            SocketIoOps::Seqpacket { .. } => Some(socket.seqpacket_send_wait(source.remaining())),
            SocketIoOps::Datagram { .. } => {
                unreachable!("connection-oriented Socket type used a non-stream I/O bundle")
            },
        };

        return retry_socket_send(
            "sys_sendto",
            &task,
            desc.vfs_file(),
            operation_wait.as_ref(),
            nonblocking,
            !message_flags.no_signal,
            || {
                socket.send(match socket.socket_type() {
                    SocketType::UnixStream => SocketSendRequest::Stream {
                        source: &mut source,
                        destination,
                    },
                    SocketType::UnixSeqpacket => SocketSendRequest::Seqpacket {
                        source: &mut source,
                        destination,
                    },
                    _ => unreachable!("Unix send branch selected a non-Unix socket"),
                })
            },
            map_send_error,
        )
        .map(|sent| sent as u64);
    }

    let destination = if addr == 0 {
        None
    } else {
        Some(read_socket_address(socket.socket_type(), addr, addrlen)?)
    };
    let mut payload = SendPayload {
        address: buf,
        len,
        bytes: None,
    };
    let mut operation = SocketDatagramSendOperation::new();
    // Each family attempt releases its operation guard before the shared retry
    // owner waits. Payload and the family's opaque destination/policy/selection
    // snapshot remain operation-local across capacity retries.
    retry_socket_send(
        "sys_sendto",
        &task,
        desc.vfs_file(),
        None,
        nonblocking,
        false,
        || {
            socket.send(SocketSendRequest::Datagram {
                destination: destination.clone(),
                payload: &mut payload,
                operation: &mut operation,
            })
        },
        map_send_error,
    )
    .map(|sent| sent as u64)
}
