use alloc::vec::Vec;

use anemone_abi::syscall::SYS_SENDMSG;

use crate::{
    fs::{
        UserBufferSegment, UserBufferSource,
        api::read_write::request::{CheckedIoVec, IoVecDirection},
        socket::{
            SocketDatagramSendOperation, SocketSendPayload, SocketSendRequest, retry_socket_send,
            socket_from_file,
        },
    },
    prelude::*,
    task::files::{Fd, FileStatusFlags},
};

use super::{message_iovecs, normalized_name_len, read_message_header};
use crate::fs::socket::api::{
    abi::{map_send_error, read_socket_address, validate_send_message_flags},
    profile::{SocketMessageIo, socket_abi_profile},
};

struct MessagePayload<'a> {
    uspace: &'a UserSpaceHandle,
    segments: Vec<UserBufferSegment>,
    total: usize,
    bytes: Option<Vec<u8>>,
}

impl SocketSendPayload for MessagePayload<'_> {
    fn bytes(&mut self, maximum: usize) -> Result<&[u8], SysError> {
        if self.total > maximum {
            return Err(SysError::MessageTooLong);
        }
        if self.bytes.is_none() {
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(self.total)
                .map_err(|_| SysError::OutOfMemory)?;
            bytes.resize(self.total, 0);
            let mut source = UserBufferSource::new(self.uspace, &self.segments);
            let copied = source.copy_into_slice(&mut bytes)?;
            // A datagram cannot expose ordinary writev's partial-success rule:
            // every user segment is copied before the one family commit.
            if copied != self.total {
                return Err(SysError::BadAddress);
            }
            self.bytes = Some(bytes);
        }
        Ok(self.bytes.as_deref().unwrap())
    }
}

fn message_segments(iovecs: &[CheckedIoVec]) -> Result<(Vec<UserBufferSegment>, usize), SysError> {
    let mut total = 0usize;
    let mut segments = Vec::new();
    segments
        .try_reserve_exact(iovecs.len())
        .map_err(|_| SysError::OutOfMemory)?;
    for iovec in iovecs {
        total = total
            .checked_add(iovec.len)
            .expect("checked message iovec total overflowed");
        segments.push(UserBufferSegment::new(iovec.base, iovec.len));
    }
    Ok((segments, total))
}

#[syscall(SYS_SENDMSG)]
fn sys_sendmsg(fd: Fd, message: u64, flags: i32) -> Result<u64, SysError> {
    let task = get_current_task();
    let desc = task.get_fd(fd)?;
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    if socket_abi_profile(socket.socket_type()).message_io() != SocketMessageIo::Datagram {
        return Err(SysError::NotSupported);
    }

    let header = read_message_header(message)?;
    let name_len = normalized_name_len(header)?;
    let destination = if name_len == 0 {
        None
    } else {
        Some(read_socket_address(
            socket.socket_type(),
            header.msg_name.bits(),
            name_len as u32,
        )?)
    };
    let uspace = task.clone_uspace_handle();
    let iovecs = message_iovecs(&uspace, header, IoVecDirection::Source)?;
    let (segments, total) = message_segments(&iovecs)?;

    // R1 has no ancillary producer or parser. Rejecting a nonempty control
    // surface is ABI-visible and must happen before payload copy or commit;
    // Stage 2B may not replace this with a success-no-op.
    if header.msg_controllen != 0 {
        knoticeln!("sendmsg: ancillary data is outside the UDP extension R1 target");
        return Err(SysError::NotSupported);
    }
    let message_flags = validate_send_message_flags(socket.socket_type(), flags)?;
    let nonblocking =
        message_flags.nonblocking || desc.file_flags().contains(FileStatusFlags::NONBLOCK);
    let mut payload = MessagePayload {
        uspace: &uspace,
        segments,
        total,
        bytes: None,
    };
    let mut operation = SocketDatagramSendOperation::new();
    retry_socket_send(
        "sys_sendmsg",
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
