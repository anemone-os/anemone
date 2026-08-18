use core::mem::{align_of, offset_of, size_of};

use anemone_abi::{
    net::linux::{
        CMsgHdr, IP_RECVERR, IPPROTO_IP, MSG_CTRUNC, MSG_ERRQUEUE, MSG_TRUNC, MsgHdr, SCM_RIGHTS,
        SO_EE_ORIGIN_ICMP, SOL_SOCKET, SockAddrIn, SockExtendedErr,
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
            SocketRightsReceiveOutcome, front::Socket, pending_error_to_sys_error,
            retry_socket_receive, socket_from_file,
        },
    },
    prelude::*,
    syscall::user_access::{UserWritePtr, UserWriteSlice, user_addr},
    task::files::{Fd, FdFlags, FileStatusFlags},
};

use super::{message_iovecs, normalized_name_len, read_message_header};
use crate::fs::socket::api::{
    abi::{map_query_error, map_receive_error, validate_recvmsg_flags, write_socket_address},
    profile::{SocketMessageIo, socket_abi_profile},
};
use zerocopy::IntoBytes;

const IPV4_ERROR_CMSG_LEN: usize =
    size_of::<CMsgHdr>() + size_of::<SockExtendedErr>() + size_of::<SockAddrIn>();

fn cmsg_space(data_len: usize) -> Option<usize> {
    size_of::<CMsgHdr>()
        .checked_add(data_len)
        .and_then(|length| length.checked_add(align_of::<CMsgHdr>() - 1))
        .map(|length| length & !(align_of::<CMsgHdr>() - 1))
}

fn control_fd_capacity(header: MsgHdr, source_count: usize) -> Result<usize, SysError> {
    if header.msg_control.is_null() {
        return Ok(0);
    }
    let capacity = usize::try_from(header.msg_controllen).map_err(|_| SysError::InvalidArgument)?;
    for count in (1..=source_count).rev() {
        let data_len = count
            .checked_mul(size_of::<i32>())
            .ok_or(SysError::InvalidArgument)?;
        if cmsg_space(data_len).is_some_and(|span| span <= capacity) {
            return Ok(count);
        }
    }
    Ok(0)
}

fn rights_control_bytes(fds: &[Fd]) -> Vec<u8> {
    assert!(!fds.is_empty());
    let data_len = fds
        .len()
        .checked_mul(size_of::<i32>())
        .expect("SCM_RIGHTS control data length overflow");
    let cmsg_len = size_of::<CMsgHdr>()
        .checked_add(data_len)
        .expect("SCM_RIGHTS cmsg length overflow");
    let span = cmsg_space(data_len).expect("SCM_RIGHTS control span overflow");
    let mut bytes = vec![0u8; span];
    let header = CMsgHdr {
        cmsg_len: cmsg_len as u64,
        cmsg_level: SOL_SOCKET,
        cmsg_type: SCM_RIGHTS,
    };
    bytes[..size_of::<CMsgHdr>()].copy_from_slice(header.as_bytes());
    for (index, fd) in fds.iter().enumerate() {
        let start = size_of::<CMsgHdr>() + index * size_of::<i32>();
        bytes[start..start + size_of::<i32>()].copy_from_slice(&(fd.raw() as i32).to_ne_bytes());
    }
    bytes
}

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

trait UnixRightsMessageOutput {
    fn write_name(&mut self) -> Result<(), SysError>;
    fn write_control(&mut self, control: &[u8]) -> Result<(), SysError>;
    fn write_flags(&mut self, flags: u32) -> Result<(), SysError>;
    fn write_control_len(&mut self, length: u64) -> Result<(), SysError>;
}

struct UserUnixRightsMessageOutput<'a> {
    task: &'a Arc<Task>,
    socket: &'a Socket,
    message: u64,
    header: MsgHdr,
}

impl UnixRightsMessageOutput for UserUnixRightsMessageOutput<'_> {
    fn write_name(&mut self) -> Result<(), SysError> {
        let mut peer = PeerCapture::default();
        self.socket
            .copy_peer_address(&mut peer)
            .map_err(map_query_error)?;
        let name_len = self
            .message
            .checked_add(offset_of!(MsgHdr, msg_namelen) as u64)
            .ok_or(SysError::BadAddress)?;
        write_socket_address(
            self.socket.socket_type(),
            self.header.msg_name.bits(),
            name_len,
            peer.0,
        )
    }

    fn write_control(&mut self, control: &[u8]) -> Result<(), SysError> {
        let address = user_addr(self.header.msg_control.bits())?;
        let uspace = self.task.clone_uspace_handle();
        UserWriteSlice::<u8>::try_new(address, control.len(), &mut uspace.lock())?
            .copy_from_slice(control)
    }

    fn write_flags(&mut self, flags: u32) -> Result<(), SysError> {
        write_message_field(self.message, offset_of!(MsgHdr, msg_flags), flags)
    }

    fn write_control_len(&mut self, length: u64) -> Result<(), SysError> {
        write_message_field(self.message, offset_of!(MsgHdr, msg_controllen), length)
    }
}

fn write_unix_rights_message_output(
    output: &mut dyn UnixRightsMessageOutput,
    has_name: bool,
    control: Option<&[u8]>,
    truncated: bool,
) -> Result<(), SysError> {
    if has_name {
        output.write_name()?;
    }
    if let Some(control) = control {
        output.write_control(control)?;
    }
    output.write_flags(if truncated { MSG_CTRUNC as u32 } else { 0 })?;
    output.write_control_len(control.map_or(0, |bytes| bytes.len() as u64))
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

fn receive_unix_stream_message(
    task: &Arc<Task>,
    desc: &crate::task::files::FileDesc,
    socket: &Socket,
    message: u64,
    header: MsgHdr,
    sink: &mut dyn SocketReadSink,
    flags: i32,
) -> Result<u64, SysError> {
    let message_flags = validate_recvmsg_flags(socket.socket_type(), flags)?;
    let mut outcome: SocketRightsReceiveOutcome = retry_socket_receive(
        "sys_recvmsg",
        task,
        desc.vfs_file(),
        message_flags.nonblocking || desc.file_flags().contains(FileStatusFlags::NONBLOCK),
        || {
            socket.receive_rights(
                sink,
                SocketReceiveFlags {
                    peek: message_flags.peek,
                },
            )
        },
        map_receive_error,
    )?;

    // The direction transaction is complete. Name, control and header output
    // are intentionally fail-forward; no later fault can requeue bytes/rights.
    let mut install = if let Some(rights) = outcome.take_rights() {
        let maximum = control_fd_capacity(header, rights.len())?;
        let fd_flags = if message_flags.close_on_exec {
            FdFlags::CLOSE_ON_EXEC
        } else {
            FdFlags::empty()
        };
        Some(task.prepare_opened_description_install(rights, maximum, fd_flags)?)
    } else {
        None
    };
    let source_count = install.as_ref().map_or(0, |plan| plan.source_count());
    let installed_count = install.as_ref().map_or(0, |plan| plan.fds().len());
    let truncated = installed_count < source_count;
    let control = install
        .as_ref()
        .filter(|plan| !plan.fds().is_empty())
        .map(|plan| rights_control_bytes(plan.fds()));
    write_unix_rights_message_output(
        &mut UserUnixRightsMessageOutput {
            task,
            socket,
            message,
            header,
        },
        !header.msg_name.is_null(),
        control.as_deref(),
        truncated,
    )?;

    if let Some(plan) = install.take() {
        plan.commit();
    }
    Ok(outcome.copied() as u64)
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
        if socket.supports_rights() {
            return receive_unix_stream_message(
                &task,
                &desc,
                socket,
                message,
                header,
                &mut stream_sink,
                flags,
            );
        }
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

    #[kunit]
    fn rights_control_capacity_uses_complete_native_spans_and_largest_prefix() {
        let mut header = MsgHdr {
            msg_control: anemone_abi::RawUserAddr64::from_bits(8),
            msg_controllen: 0,
            ..MsgHdr::default()
        };
        assert_eq!(control_fd_capacity(header, 3), Ok(0));
        header.msg_controllen = (size_of::<CMsgHdr>() - 1) as u64;
        assert_eq!(control_fd_capacity(header, 3), Ok(0));
        header.msg_controllen = cmsg_space(size_of::<i32>()).unwrap() as u64;
        assert_eq!(control_fd_capacity(header, 3), Ok(2));
        header.msg_controllen = cmsg_space(2 * size_of::<i32>()).unwrap() as u64;
        assert_eq!(control_fd_capacity(header, 3), Ok(2));
        header.msg_controllen = cmsg_space(3 * size_of::<i32>()).unwrap() as u64;
        assert_eq!(control_fd_capacity(header, 3), Ok(3));

        header.msg_control = anemone_abi::RawUserAddr64::NULL;
        assert_eq!(control_fd_capacity(header, 3), Ok(0));
    }

    #[kunit]
    fn rights_control_projection_encodes_actual_fd_count_and_zero_padding() {
        let fds = [
            Fd::new(3).unwrap(),
            Fd::new(9).unwrap(),
            Fd::new(11).unwrap(),
        ];
        let bytes = rights_control_bytes(&fds);
        assert_eq!(bytes.len(), cmsg_space(3 * size_of::<i32>()).unwrap());
        assert_eq!(
            u64::from_ne_bytes(bytes[0..8].try_into().unwrap()) as usize,
            size_of::<CMsgHdr>() + 3 * size_of::<i32>()
        );
        assert_eq!(
            i32::from_ne_bytes(bytes[8..12].try_into().unwrap()),
            SOL_SOCKET
        );
        assert_eq!(
            i32::from_ne_bytes(bytes[12..16].try_into().unwrap()),
            SCM_RIGHTS
        );
        assert_eq!(i32::from_ne_bytes(bytes[16..20].try_into().unwrap()), 3);
        assert_eq!(i32::from_ne_bytes(bytes[20..24].try_into().unwrap()), 9);
        assert_eq!(i32::from_ne_bytes(bytes[24..28].try_into().unwrap()), 11);
        assert!(bytes[28..].iter().all(|byte| *byte == 0));
    }

    #[derive(Default)]
    struct RecordingRightsOutput {
        calls: Vec<&'static str>,
        fault_at: Option<&'static str>,
        flags: Option<u32>,
        control_len: Option<u64>,
    }

    impl RecordingRightsOutput {
        fn record(&mut self, call: &'static str) -> Result<(), SysError> {
            self.calls.push(call);
            if self.fault_at == Some(call) {
                Err(SysError::BadAddress)
            } else {
                Ok(())
            }
        }
    }

    impl UnixRightsMessageOutput for RecordingRightsOutput {
        fn write_name(&mut self) -> Result<(), SysError> {
            self.record("name")
        }

        fn write_control(&mut self, _control: &[u8]) -> Result<(), SysError> {
            self.record("control")
        }

        fn write_flags(&mut self, flags: u32) -> Result<(), SysError> {
            self.flags = Some(flags);
            self.record("flags")
        }

        fn write_control_len(&mut self, length: u64) -> Result<(), SysError> {
            self.control_len = Some(length);
            self.record("controllen")
        }
    }

    #[kunit]
    fn unix_rights_output_is_ordered_and_faults_before_fd_publication() {
        let control = [1u8; 24];
        let mut output = RecordingRightsOutput::default();
        write_unix_rights_message_output(&mut output, true, Some(&control), true).unwrap();
        assert_eq!(output.calls, ["name", "control", "flags", "controllen"]);
        assert_eq!(output.flags, Some(MSG_CTRUNC as u32));
        assert_eq!(output.control_len, Some(control.len() as u64));

        for fault in ["name", "control", "flags", "controllen"] {
            let mut output = RecordingRightsOutput {
                fault_at: Some(fault),
                ..RecordingRightsOutput::default()
            };
            assert_eq!(
                write_unix_rights_message_output(&mut output, true, Some(&control), false),
                Err(SysError::BadAddress)
            );
            assert_eq!(output.calls.last(), Some(&fault));
        }

        let mut absent = RecordingRightsOutput::default();
        write_unix_rights_message_output(&mut absent, false, None, true).unwrap();
        assert_eq!(absent.calls, ["flags", "controllen"]);
        assert_eq!(absent.flags, Some(MSG_CTRUNC as u32));
        assert_eq!(absent.control_len, Some(0));
    }
}
