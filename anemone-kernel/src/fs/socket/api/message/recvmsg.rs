use core::mem::offset_of;

use anemone_abi::{
    net::linux::{MSG_TRUNC, MsgHdr},
    syscall::SYS_RECVMSG,
};

use crate::{
    fs::{
        UserBufferSegment, UserBufferSink,
        api::read_write::request::{CheckedIoVec, IoVecDirection},
        socket::{
            SocketAddress, SocketAddressSink, SocketReadSink, SocketReceiveFlags,
            SocketReceiveOutcome, SocketReceiveRequest, SocketReceiveSink, front::Socket,
            retry_socket_receive, socket_from_file,
        },
    },
    prelude::*,
    syscall::user_access::{UserWritePtr, UserWriteSlice, user_addr},
    task::files::{Fd, FileStatusFlags},
};

use super::{message_iovecs, normalized_name_len, read_message_header};
use crate::fs::socket::api::{
    abi::{map_receive_error, validate_receive_message_flags, write_socket_address},
    profile::{SocketMessageIo, socket_abi_profile},
};

struct MessageReceiveSink<'a> {
    uspace: &'a UserSpaceHandle,
    iovecs: &'a [CheckedIoVec],
    peer: Option<SocketAddress>,
}

#[derive(Default)]
struct PeerCapture(Option<SocketAddress>);

impl SocketAddressSink for PeerCapture {
    fn copy_address(&mut self, address: Option<SocketAddress>) -> Result<(), SysError> {
        self.0 = address;
        Ok(())
    }
}

impl SocketReceiveSink for MessageReceiveSink<'_> {
    fn copy_datagram(&mut self, payload: &[u8], peer: SocketAddress) -> Result<usize, SysError> {
        let mut copied = 0usize;
        let mut guard = self.uspace.lock();
        for iovec in self.iovecs {
            if copied == payload.len() {
                break;
            }
            let len = iovec.len.min(payload.len() - copied);
            if len == 0 {
                continue;
            }
            let mut destination = UserWriteSlice::<u8>::try_new(iovec.base, len, &mut guard)?;
            match destination.copy_from_slice_partial(&payload[copied..copied + len]) {
                Ok(written) => {
                    assert_eq!(written, len, "successful message payload copy was short");
                    copied += written;
                },
                Err(error) => {
                    assert!(
                        error.copied() < len,
                        "failed message payload copy reported full progress"
                    );
                    // Linux returns the copy fault even when a prior payload
                    // prefix is already visible. The Endpoint owner still
                    // decides detach versus peek, so this error cannot requeue
                    // a non-peek datagram or consume a peeked one.
                    return Err(error.error());
                },
            }
        }
        self.peer = Some(peer);
        Ok(copied)
    }
}

fn write_message_field<T: Copy>(message: u64, offset: usize, value: T) -> Result<(), SysError> {
    let address = message
        .checked_add(offset as u64)
        .ok_or(SysError::BadAddress)?;
    let address = user_addr(address)?;
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    UserWritePtr::<T>::try_new(address, &mut uspace.lock())?.write(value)
}

fn receive_return(outcome: SocketReceiveOutcome, truncate_result: bool) -> usize {
    if truncate_result {
        outcome
            .packet_length()
            .expect("message receive returned a non-datagram outcome")
    } else {
        outcome.copied()
    }
}

pub(super) trait StreamMessageOutput {
    fn write_name_len_zero(&mut self) -> Result<(), SysError>;
    fn write_flags_zero(&mut self) -> Result<(), SysError>;
    fn write_control_len_zero(&mut self) -> Result<(), SysError>;
}

struct UserStreamMessageOutput {
    message: u64,
}

impl StreamMessageOutput for UserStreamMessageOutput {
    fn write_name_len_zero(&mut self) -> Result<(), SysError> {
        write_message_field(self.message, offset_of!(MsgHdr, msg_namelen), 0i32)
    }

    fn write_flags_zero(&mut self) -> Result<(), SysError> {
        write_message_field(self.message, offset_of!(MsgHdr, msg_flags), 0u32)
    }

    fn write_control_len_zero(&mut self) -> Result<(), SysError> {
        write_message_field(self.message, offset_of!(MsgHdr, msg_controllen), 0u64)
    }
}

pub(super) fn write_stream_message_output(
    output: &mut dyn StreamMessageOutput,
    has_name: bool,
) -> Result<(), SysError> {
    if has_name {
        // Linux tcp_recvmsg does not project getpeername semantics through
        // recvmsg; inet_recvmsg reports an empty name instead.
        output.write_name_len_zero()?;
    }
    output.write_flags_zero()?;
    output.write_control_len_zero()
}

pub(super) fn receive_stream_message(
    task: &Arc<Task>,
    file: &File,
    socket: &Socket,
    sink: &mut dyn SocketReadSink,
    flags: i32,
    file_nonblocking: bool,
) -> Result<SocketReceiveOutcome, SysError> {
    let message_flags = validate_receive_message_flags(socket.socket_type(), flags)?;
    retry_socket_receive(
        "sys_recvmsg",
        task,
        file,
        message_flags.nonblocking || file_nonblocking,
        || {
            socket.receive(SocketReceiveRequest::Stream {
                sink,
                flags: SocketReceiveFlags {
                    peek: message_flags.peek,
                },
            })
        },
        map_receive_error,
    )
}

pub(super) fn receive_message(fd: Fd, message: u64, flags: i32) -> Result<u64, SysError> {
    let task = get_current_task();
    let desc = task.get_fd(fd)?;
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    let message_io = socket_abi_profile(socket.socket_type()).message_io();
    if message_io == SocketMessageIo::Unsupported {
        return Err(SysError::NotSupported);
    }

    let header = read_message_header(message)?;
    let _name_len = normalized_name_len(header)?;
    let uspace = task.clone_uspace_handle();
    let iovecs = message_iovecs(&uspace, header, IoVecDirection::Destination)?;
    if message_io == SocketMessageIo::ByteStream {
        let mut segments = Vec::new();
        segments
            .try_reserve_exact(iovecs.len())
            .map_err(|_| SysError::OutOfMemory)?;
        segments.extend(
            iovecs
                .iter()
                .map(|iovec| UserBufferSegment::new(iovec.base, iovec.len)),
        );
        let mut stream_sink = UserBufferSink::new(&uspace, &segments);
        let outcome = receive_stream_message(
            &task,
            desc.vfs_file(),
            socket,
            &mut stream_sink,
            flags,
            desc.file_flags().contains(FileStatusFlags::NONBLOCK),
        )?;
        write_stream_message_output(
            &mut UserStreamMessageOutput { message },
            !header.msg_name.is_null(),
        )?;
        return Ok(outcome.copied() as u64);
    }
    let message_flags = validate_receive_message_flags(socket.socket_type(), flags)?;
    let nonblocking =
        message_flags.nonblocking || desc.file_flags().contains(FileStatusFlags::NONBLOCK);
    let mut sink = MessageReceiveSink {
        uspace: &uspace,
        iovecs: &iovecs,
        peer: None,
    };

    let outcome = retry_socket_receive(
        "sys_recvmsg",
        &task,
        desc.vfs_file(),
        nonblocking,
        || {
            socket.receive(SocketReceiveRequest::Datagram {
                sink: &mut sink,
                flags: SocketReceiveFlags {
                    peek: message_flags.peek,
                },
            })
        },
        map_receive_error,
    )?;
    let peer = sink
        .peer
        .take()
        .expect("UDP message receive omitted its peer outcome");

    // Linux exposes these outputs in payload -> name -> flags -> controllen
    // order. A later fault retains every earlier prefix and never requeues a
    // non-peek datagram that the Endpoint owner already detached.
    if !header.msg_name.is_null() {
        let name_len = message
            .checked_add(offset_of!(MsgHdr, msg_namelen) as u64)
            .ok_or(SysError::BadAddress)?;
        write_socket_address(
            socket.socket_type(),
            header.msg_name as u64,
            name_len,
            Some(peer),
        )?;
    }
    let output_flags = if outcome.packet_length().unwrap() > outcome.copied() {
        MSG_TRUNC as u32
    } else {
        0
    };
    write_message_field(message, offset_of!(MsgHdr, msg_flags), output_flags)?;
    write_message_field(message, offset_of!(MsgHdr, msg_controllen), 0u64)?;

    Ok(receive_return(outcome, message_flags.truncate_result) as u64)
}

#[syscall(SYS_RECVMSG)]
fn sys_recvmsg(fd: Fd, message: u64, flags: i32) -> Result<u64, SysError> {
    receive_message(fd, message, flags)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn datagram_result_projects_truncation_without_a_second_length_truth() {
        let outcome = SocketReceiveOutcome::datagram(2, 8);
        assert_eq!(receive_return(outcome, false), 2);
        assert_eq!(receive_return(outcome, true), 8);
    }
}
