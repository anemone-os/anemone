use alloc::vec::Vec;
use core::mem::offset_of;

use anemone_abi::{
    fs::linux::IOV_MAX,
    net::linux::MMsgHdr,
    syscall::{SYS_SENDMMSG, SYS_SENDMSG},
};

use crate::{
    fs::{
        UserBufferSegment, UserBufferSource,
        api::read_write::request::{CheckedIoVec, IoVecDirection},
        socket::{
            SocketDatagramSendOperation, SocketSendPayload, SocketSendRequest,
            SocketStreamDestination, SocketWriteSource, front::Socket, retry_socket_send,
            socket_from_file,
        },
    },
    prelude::*,
    syscall::user_access::{UserWritePtr, user_addr},
    task::files::{Fd, FileDesc, FileStatusFlags},
};

use super::{message_iovecs, normalized_name_len, read_message_header};
use crate::fs::socket::api::{
    abi::{
        map_send_error, read_socket_address, validate_raw_socket_address,
        validate_send_message_flags,
    },
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

pub(super) fn message_segments(
    iovecs: &[CheckedIoVec],
) -> Result<(Vec<UserBufferSegment>, usize), SysError> {
    let mut total = 0usize;
    let mut segments = Vec::new();
    segments
        .try_reserve_exact(iovecs.len())
        .map_err(|_| SysError::OutOfMemory)?;
    for iovec in iovecs {
        total = total
            .checked_add(iovec.len)
            .ok_or(SysError::MessageTooLong)?;
        segments.push(UserBufferSegment::new(iovec.base, iovec.len));
    }
    Ok((segments, total))
}

pub(super) const fn stream_destination(has_destination: bool) -> SocketStreamDestination {
    if has_destination {
        SocketStreamDestination::Present
    } else {
        SocketStreamDestination::Absent
    }
}

pub(super) fn send_stream_message(
    task: &Arc<Task>,
    file: &File,
    socket: &Socket,
    source: &mut dyn SocketWriteSource,
    destination: SocketStreamDestination,
    flags: i32,
    file_nonblocking: bool,
) -> Result<u64, SysError> {
    let message_flags = validate_send_message_flags(socket.socket_type(), flags)?;
    retry_socket_send(
        "sys_sendmsg",
        task,
        file,
        None,
        message_flags.nonblocking || file_nonblocking,
        !message_flags.no_signal,
        || {
            socket.send(SocketSendRequest::Stream {
                source,
                destination,
            })
        },
        map_send_error,
    )
    .map(|sent| sent as u64)
}

pub(super) fn validate_send_control(length: u64) -> Result<(), SysError> {
    if length == 0 {
        return Ok(());
    }
    // No current Socket profile has an ancillary producer or parser.
    knoticeln!("sendmsg: ancillary data is unsupported by the selected Socket profile");
    Err(SysError::NotSupported)
}

struct MessageSendResult {
    sent: u64,
    complete: bool,
}

fn send_message_on_socket(
    task: &Arc<Task>,
    desc: &FileDesc,
    socket: &Socket,
    message: u64,
    flags: i32,
) -> Result<MessageSendResult, SysError> {
    let message_io = socket_abi_profile(socket.socket_type()).message_io();
    if message_io == SocketMessageIo::Unsupported {
        return Err(SysError::NotSupported);
    }

    let header = read_message_header(message)?;
    let name_len = normalized_name_len(header)?;
    let has_destination = name_len != 0;
    let destination = if name_len == 0 {
        None
    } else if message_io == SocketMessageIo::ByteStream {
        // Linux copies and range-checks a TCP msg_name but inet_sendmsg and
        // tcp_sendmsg do not interpret its family or address. Preserve those
        // raw-copy faults without turning the bytes into a destination.
        validate_raw_socket_address(header.msg_name.bits(), name_len as u32)?;
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

    // Rejection precedes payload copy or family commit; it cannot be replaced
    // with a success-no-op.
    validate_send_control(header.msg_controllen)?;
    let mut payload = MessagePayload {
        uspace: &uspace,
        segments,
        total,
        bytes: None,
    };
    if message_io == SocketMessageIo::ByteStream {
        let mut source = UserBufferSource::new(&uspace, &payload.segments);
        let sent = send_stream_message(
            task,
            desc.vfs_file(),
            socket,
            &mut source,
            stream_destination(has_destination),
            flags,
            desc.file_flags().contains(FileStatusFlags::NONBLOCK),
        )?;
        return Ok(MessageSendResult {
            sent,
            complete: source.remaining() == 0,
        });
    }
    let message_flags = validate_send_message_flags(socket.socket_type(), flags)?;
    let nonblocking =
        message_flags.nonblocking || desc.file_flags().contains(FileStatusFlags::NONBLOCK);
    let mut operation = SocketDatagramSendOperation::new();
    retry_socket_send(
        "sys_sendmsg",
        task,
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
    .map(|sent| MessageSendResult {
        sent: sent as u64,
        complete: true,
    })
}

pub(super) fn send_message(fd: Fd, message: u64, flags: i32) -> Result<u64, SysError> {
    let task = get_current_task();
    let desc = task.get_fd(fd)?;
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    send_message_on_socket(&task, &desc, socket, message, flags).map(|result| result.sent)
}

fn send_messages(fd: Fd, messages: u64, vlen: u32, flags: i32) -> Result<u64, SysError> {
    let task = get_current_task();
    // Keep one opened-description identity for the whole batch, as Linux does
    // with one sockfd lookup before its sequential single-message loop.
    let desc = task.get_fd(fd)?;
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    let count = (vlen as usize).min(IOV_MAX);
    let stride = size_of::<MMsgHdr>() as u64;
    let uspace = task.clone_uspace_handle();
    let mut completed = 0u64;

    for index in 0..count {
        let offset = (index as u64)
            .checked_mul(stride)
            .ok_or(SysError::BadAddress);
        let entry =
            offset.and_then(|offset| messages.checked_add(offset).ok_or(SysError::BadAddress));
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if completed == 0 => return Err(error),
            Err(_) => return Ok(completed),
        };
        let result = match send_message_on_socket(&task, &desc, socket, entry, flags) {
            Ok(result) => result,
            Err(error) if completed == 0 => return Err(error),
            Err(_) => return Ok(completed),
        };
        let message_len = match u32::try_from(result.sent) {
            Ok(message_len) => message_len,
            Err(_) if completed == 0 => return Err(SysError::MessageTooLong),
            Err(_) => return Ok(completed),
        };
        let length_address = entry
            .checked_add(offset_of!(MMsgHdr, msg_len) as u64)
            .ok_or(SysError::BadAddress);
        let write_result = length_address.and_then(|address| {
            UserWritePtr::<u32>::try_new(user_addr(address)?, &mut uspace.lock())?
                .write(message_len)
        });
        match write_result {
            Ok(()) => completed += 1,
            Err(error) if completed == 0 => return Err(error),
            Err(_) => return Ok(completed),
        }
        if !result.complete {
            break;
        }
    }
    Ok(completed)
}

#[syscall(SYS_SENDMSG)]
fn sys_sendmsg(fd: Fd, message: u64, flags: i32) -> Result<u64, SysError> {
    send_message(fd, message, flags)
}

#[syscall(SYS_SENDMMSG)]
fn sys_sendmmsg(fd: Fd, messages: u64, vlen: u32, flags: i32) -> Result<u64, SysError> {
    send_messages(fd, messages, vlen, flags)
}
