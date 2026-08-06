use core::mem::{offset_of, size_of};

use anemone_abi::{
    net::linux::{
        CMsgHdr, IP_RECVERR, IPPROTO_IP, MSG_CTRUNC, MSG_ERRQUEUE, MSG_TRUNC, MsgHdr,
        SO_EE_ORIGIN_ICMP, SockAddrIn, SockExtendedErr,
    },
    syscall::SYS_RECVMSG,
};

use crate::{
    fs::{
        UserBufferSegment, UserBufferSink,
        api::read_write::request::{CheckedIoVec, IoVecDirection},
        socket::{
            SocketAddress, SocketAddressSink, SocketIpv4ExtendedError, SocketReadSink,
            SocketReceiveFlags, SocketReceiveOutcome, SocketReceiveRequest, SocketReceiveSink,
            front::Socket, pending_error_to_sys_error, retry_socket_receive, socket_from_file,
        },
    },
    prelude::*,
    syscall::user_access::{UserWritePtr, UserWriteSlice, user_addr},
    task::files::{Fd, FileStatusFlags},
};

use super::{message_iovecs, normalized_name_len, read_message_header};
use crate::fs::socket::api::{
    abi::{map_receive_error, validate_recvmsg_flags, write_socket_address},
    profile::{SocketMessageIo, socket_abi_profile},
};
use zerocopy::IntoBytes;

const IPV4_ERROR_CMSG_LEN: usize =
    size_of::<CMsgHdr>() + size_of::<SockExtendedErr>() + size_of::<SockAddrIn>();

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

fn write_message_field<T: zerocopy::IntoBytes + zerocopy::Immutable>(
    message: u64,
    offset: usize,
    value: T,
) -> Result<(), SysError> {
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

fn error_control_bytes(
    record: &SocketIpv4ExtendedError,
    capacity: usize,
) -> ([u8; IPV4_ERROR_CMSG_LEN], usize, bool) {
    if capacity < size_of::<CMsgHdr>() {
        return ([0; IPV4_ERROR_CMSG_LEN], 0, true);
    }
    let copied = capacity.min(IPV4_ERROR_CMSG_LEN);
    let header = CMsgHdr {
        cmsg_len: copied as u64,
        cmsg_level: IPPROTO_IP,
        cmsg_type: IP_RECVERR,
    };
    let error = SockExtendedErr {
        ee_errno: pending_error_to_sys_error(record.cause).as_errno() as u32,
        ee_origin: SO_EE_ORIGIN_ICMP,
        ee_type: record.icmp_type,
        ee_code: record.icmp_code,
        ee_pad: 0,
        ee_info: record.info,
        ee_data: 0,
    };
    let offender = SockAddrIn::new(record.offender.octets(), 0);
    let mut bytes = [0u8; IPV4_ERROR_CMSG_LEN];
    let header_end = size_of::<CMsgHdr>();
    let error_end = header_end + size_of::<SockExtendedErr>();
    bytes[..header_end].copy_from_slice(header.as_bytes());
    bytes[header_end..error_end].copy_from_slice(error.as_bytes());
    bytes[error_end..].copy_from_slice(offender.as_bytes());
    (bytes, copied, copied < IPV4_ERROR_CMSG_LEN)
}

fn receive_error_message(
    socket: &Socket,
    message: u64,
    header: MsgHdr,
    iovecs: &[CheckedIoVec],
) -> Result<u64, SysError> {
    // Detach precedes every payload/name/control/header copy. Linux consumes
    // the skb even when a later user access faults; the Endpoint must never
    // regain ownership or reorder the FIFO after this point.
    let record = socket
        .detach_ipv4_extended_error()
        .map_err(map_receive_error)?;
    let payload_len = record.quoted_payload.len();
    let uspace = get_current_task().clone_uspace_handle();
    let mut sink = MessageReceiveSink {
        uspace: &uspace,
        iovecs,
        peer: None,
    };
    let copied = sink.copy_datagram(&record.quoted_payload, record.original_destination.clone())?;

    if !header.msg_name.is_null() {
        let name_len = message
            .checked_add(offset_of!(MsgHdr, msg_namelen) as u64)
            .ok_or(SysError::BadAddress)?;
        write_socket_address(
            socket.socket_type(),
            header.msg_name.bits(),
            name_len,
            Some(record.original_destination.clone()),
        )?;
    }

    let control_capacity =
        usize::try_from(header.msg_controllen).map_err(|_| SysError::InvalidArgument)?;
    let (control, control_copied, control_truncated) =
        error_control_bytes(&record, control_capacity);
    if control_copied != 0 {
        let address = user_addr(header.msg_control.bits())?;
        UserWriteSlice::<u8>::try_new(address, control_copied, &mut uspace.lock())?
            .copy_from_slice(&control[..control_copied])?;
    }

    let mut output_flags = MSG_ERRQUEUE as u32;
    if copied < payload_len {
        output_flags |= MSG_TRUNC as u32;
    }
    if control_truncated {
        output_flags |= MSG_CTRUNC as u32;
    }
    write_message_field(message, offset_of!(MsgHdr, msg_flags), output_flags)?;
    write_message_field(
        message,
        offset_of!(MsgHdr, msg_controllen),
        control_copied as u64,
    )?;

    // MSG_TRUNC is still reported as an output flag for a short quoted
    // payload, but Linux errqueue receive returns the copied length even when
    // MSG_TRUNC was also supplied as an input flag.
    Ok(copied as u64)
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
    let message_flags = validate_recvmsg_flags(socket.socket_type(), flags)?;
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
    let message_flags = validate_recvmsg_flags(socket.socket_type(), flags)?;
    if message_flags.error_queue {
        return receive_error_message(socket, message, header, &iovecs);
    }
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
            header.msg_name.bits(),
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

    use anemone_net_api::Ipv4Address;

    #[kunit]
    fn datagram_result_projects_truncation_without_a_second_length_truth() {
        let outcome = SocketReceiveOutcome::datagram(2, 8);
        assert_eq!(receive_return(outcome, false), 2);
        assert_eq!(receive_return(outcome, true), 8);
    }

    #[kunit]
    fn ipv4_error_control_projection_preserves_linux_alignment_and_truncation() {
        let record = SocketIpv4ExtendedError {
            cause: crate::fs::socket::SocketPendingError::ConnectionRefused,
            icmp_type: 3,
            icmp_code: 3,
            info: 0,
            original_destination: SocketAddress::Ipv4 {
                address: Ipv4Address::LOOPBACK,
                port: 53,
            },
            offender: Ipv4Address::LOOPBACK,
            quoted_payload: b"marker".to_vec(),
        };
        let (full, copied, truncated) = error_control_bytes(&record, IPV4_ERROR_CMSG_LEN);
        assert_eq!(copied, 48);
        assert!(!truncated);
        assert_eq!(u64::from_ne_bytes(full[0..8].try_into().unwrap()), 48);
        assert_eq!(
            u32::from_ne_bytes(full[16..20].try_into().unwrap()),
            SysError::ConnectionRefused.as_errno() as u32
        );
        assert_eq!(&full[20..24], &[SO_EE_ORIGIN_ICMP, 3, 3, 0]);
        assert_eq!(
            u16::from_ne_bytes(full[32..34].try_into().unwrap()),
            anemone_abi::net::linux::AF_INET as u16
        );

        let (_, copied, truncated) = error_control_bytes(&record, size_of::<CMsgHdr>() + 4);
        assert_eq!(copied, 20);
        assert!(truncated);
        let (_, copied, truncated) = error_control_bytes(&record, size_of::<CMsgHdr>() - 1);
        assert_eq!(copied, 0);
        assert!(truncated);
    }
}
