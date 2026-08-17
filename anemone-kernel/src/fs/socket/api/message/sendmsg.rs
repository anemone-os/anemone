use alloc::vec::Vec;
use core::{
    mem::{align_of, offset_of, size_of},
    sync::atomic::{AtomicBool, Ordering},
};

use anemone_abi::{
    fs::linux::IOV_MAX,
    net::linux::{CMsgHdr, MMsgHdr, SCM_RIGHTS, SOL_SOCKET},
    syscall::{SYS_SENDMMSG, SYS_SENDMSG},
};

use crate::{
    fs::{
        UserBufferSegment, UserBufferSource,
        api::read_write::request::{CheckedIoVec, IoVecDirection},
        socket::{
            SocketDatagramSendOperation, SocketRightsSendRequest, SocketSendPayload,
            SocketSendRequest, SocketStreamDestination, SocketWriteSource, front::Socket,
            is_unix_socket_file, retry_socket_send, socket_from_file,
        },
    },
    kconfig_defs::UNIX_SCM_RIGHTS_MAX_FDS_PER_MESSAGE,
    prelude::*,
    syscall::user_access::{UserReadSlice, UserWritePtr, user_addr},
    task::files::{Fd, FileDesc, FileStatusFlags, OpenedDescriptionBundle},
};

static UNIX_FD_REJECTION_DIAGNOSTIC: AtomicBool = AtomicBool::new(false);

fn cmsg_align(length: usize) -> Option<usize> {
    length
        .checked_add(align_of::<CMsgHdr>() - 1)
        .map(|length| length & !(align_of::<CMsgHdr>() - 1))
}

fn parse_rights_control_with(
    length: usize,
    read: &mut dyn FnMut(usize, &mut [u8]) -> Result<(), SysError>,
) -> Result<Vec<Fd>, SysError> {
    let mut fds = Vec::new();
    fds.try_reserve_exact(UNIX_SCM_RIGHTS_MAX_FDS_PER_MESSAGE)
        .map_err(|_| SysError::OutOfMemory)?;
    let mut offset = 0usize;
    while offset < length {
        if offset % align_of::<CMsgHdr>() != 0 || length - offset < size_of::<CMsgHdr>() {
            return Err(SysError::InvalidArgument);
        }
        let mut raw_header = [0u8; size_of::<CMsgHdr>()];
        read(offset, &mut raw_header)?;
        let cmsg_len = usize::try_from(u64::from_ne_bytes(raw_header[0..8].try_into().unwrap()))
            .map_err(|_| SysError::InvalidArgument)?;
        let level = i32::from_ne_bytes(raw_header[8..12].try_into().unwrap());
        let kind = i32::from_ne_bytes(raw_header[12..16].try_into().unwrap());
        if cmsg_len < size_of::<CMsgHdr>() || cmsg_len > length - offset {
            return Err(SysError::InvalidArgument);
        }
        let data_len = cmsg_len - size_of::<CMsgHdr>();
        if data_len % size_of::<i32>() != 0 {
            return Err(SysError::InvalidArgument);
        }
        if level != SOL_SOCKET || kind != SCM_RIGHTS {
            return Err(SysError::NotSupported);
        }
        let count = data_len / size_of::<i32>();
        if fds
            .len()
            .checked_add(count)
            .is_none_or(|count| count > UNIX_SCM_RIGHTS_MAX_FDS_PER_MESSAGE)
        {
            return Err(SysError::InvalidArgument);
        }
        let mut raw_fds = Vec::new();
        raw_fds
            .try_reserve_exact(data_len)
            .map_err(|_| SysError::OutOfMemory)?;
        raw_fds.resize(data_len, 0);
        if data_len != 0 {
            read(offset + size_of::<CMsgHdr>(), &mut raw_fds)?;
        }
        for raw in raw_fds.chunks_exact(size_of::<i32>()) {
            let value = i32::from_ne_bytes(raw.try_into().unwrap());
            let fd = u32::try_from(value)
                .ok()
                .and_then(Fd::new)
                .ok_or(SysError::BadFileDescriptor)?;
            fds.push(fd);
        }
        let next = cmsg_align(cmsg_len)
            .and_then(|span| offset.checked_add(span))
            .ok_or(SysError::InvalidArgument)?;
        if next > length {
            break;
        }
        offset = next;
    }
    Ok(fds)
}

fn capture_send_rights(
    task: &Arc<Task>,
    socket: &Socket,
    control: u64,
    length: u64,
) -> Result<Option<OpenedDescriptionBundle>, SysError> {
    if length == 0 {
        return Ok(None);
    }
    if !socket.supports_rights() {
        knoticeln!("sendmsg: ancillary data is unsupported by the selected Socket profile");
        return Err(SysError::NotSupported);
    }
    let length = usize::try_from(length).map_err(|_| SysError::InvalidArgument)?;
    let base = user_addr(control)?;
    if base.get() % align_of::<CMsgHdr>() as u64 != 0 {
        return Err(SysError::InvalidArgument);
    }
    let uspace = task.clone_uspace_handle();
    let mut read = |offset: usize, bytes: &mut [u8]| {
        let address = base
            .get()
            .checked_add(offset as u64)
            .map(VirtAddr::new)
            .ok_or(SysError::BadAddress)?;
        UserReadSlice::<u8>::try_new(address, bytes.len(), &mut uspace.lock())?.copy_to_slice(bytes)
    };
    let fds = parse_rights_control_with(length, &mut read)?;
    capture_parsed_rights(
        &fds,
        |fds| task.capture_opened_descriptions(fds),
        is_unix_socket_file,
    )
}

fn capture_parsed_rights(
    fds: &[Fd],
    capture: impl FnOnce(&[Fd]) -> Result<OpenedDescriptionBundle, SysError>,
    rejected_file: fn(&File) -> bool,
) -> Result<Option<OpenedDescriptionBundle>, SysError> {
    if fds.is_empty() {
        return Ok(None);
    }
    let bundle = capture(fds)?;
    if bundle.any_file_matches(rejected_file) {
        if !UNIX_FD_REJECTION_DIAGNOSTIC.swap(true, Ordering::Relaxed) {
            knoticeln!("sendmsg: AF_UNIX descriptors are rejected from SCM_RIGHTS");
        }
        return Err(SysError::NotSupported);
    }
    Ok(Some(bundle))
}

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

fn send_stream_rights_message(
    task: &Arc<Task>,
    file: &File,
    socket: &Socket,
    source: &mut dyn SocketWriteSource,
    destination: SocketStreamDestination,
    flags: i32,
    file_nonblocking: bool,
    mut rights: Option<OpenedDescriptionBundle>,
) -> Result<u64, SysError> {
    let message_flags = validate_send_message_flags(socket.socket_type(), flags)?;
    let rights_count = rights.as_ref().map_or(0, OpenedDescriptionBundle::len);
    let wait = (rights_count != 0)
        .then(|| socket.rights_send_wait(rights_count))
        .flatten();
    let sent = retry_socket_send(
        "sys_sendmsg",
        task,
        file,
        wait.as_ref(),
        message_flags.nonblocking || file_nonblocking,
        !message_flags.no_signal,
        || {
            socket.send_rights(SocketRightsSendRequest {
                source,
                destination,
                rights: &mut rights,
            })
        },
        map_send_error,
    )?;
    assert!(
        sent == 0 || rights.is_none(),
        "positive Unix stream send did not transfer its rights bundle"
    );
    Ok(sent as u64)
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

fn drive_send_messages(
    count: usize,
    mut entry_at: impl FnMut(usize) -> Result<u64, SysError>,
    mut send_one: impl FnMut(u64) -> Result<MessageSendResult, SysError>,
    mut write_length: impl FnMut(u64, u32) -> Result<(), SysError>,
) -> Result<u64, SysError> {
    let mut completed = 0u64;
    for index in 0..count {
        let entry = match entry_at(index) {
            Ok(entry) => entry,
            Err(error) if completed == 0 => return Err(error),
            Err(_) => return Ok(completed),
        };
        let result = match send_one(entry) {
            Ok(result) => result,
            Err(error) if completed == 0 => return Err(error),
            Err(_) => return Ok(completed),
        };
        let message_len = match u32::try_from(result.sent) {
            Ok(message_len) => message_len,
            Err(_) if completed == 0 => return Err(SysError::MessageTooLong),
            Err(_) => return Ok(completed),
        };
        match write_length(entry, message_len) {
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

    // Rejection and exact fd capture precede payload copy or family commit.
    // Zero-payload messages still validate and release their captured bundle.
    let rights = if socket.supports_rights() {
        capture_send_rights(
            task,
            socket,
            header.msg_control.bits(),
            header.msg_controllen,
        )?
    } else {
        validate_send_control(header.msg_controllen)?;
        None
    };
    let mut payload = MessagePayload {
        uspace: &uspace,
        segments,
        total,
        bytes: None,
    };
    if message_io == SocketMessageIo::ByteStream {
        let mut source = UserBufferSource::new(&uspace, &payload.segments);
        let sent = if socket.supports_rights() {
            send_stream_rights_message(
                task,
                desc.vfs_file(),
                socket,
                &mut source,
                stream_destination(has_destination),
                flags,
                desc.file_flags().contains(FileStatusFlags::NONBLOCK),
                rights,
            )?
        } else {
            send_stream_message(
                task,
                desc.vfs_file(),
                socket,
                &mut source,
                stream_destination(has_destination),
                flags,
                desc.file_flags().contains(FileStatusFlags::NONBLOCK),
            )?
        };
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
    drive_send_messages(
        count,
        |index| {
            (index as u64)
                .checked_mul(stride)
                .and_then(|offset| messages.checked_add(offset))
                .ok_or(SysError::BadAddress)
        },
        |entry| send_message_on_socket(&task, &desc, socket, entry, flags),
        |entry, message_len| {
            let address = entry
                .checked_add(offset_of!(MMsgHdr, msg_len) as u64)
                .ok_or(SysError::BadAddress)?;
            UserWritePtr::<u32>::try_new(user_addr(address)?, &mut uspace.lock())?
                .write(message_len)
        },
    )
}

#[syscall(SYS_SENDMSG)]
fn sys_sendmsg(fd: Fd, message: u64, flags: i32) -> Result<u64, SysError> {
    send_message(fd, message, flags)
}

#[syscall(SYS_SENDMMSG)]
fn sys_sendmmsg(fd: Fd, messages: u64, vlen: u32, flags: i32) -> Result<u64, SysError> {
    send_messages(fd, messages, vlen, flags)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn append_cmsg(control: &mut Vec<u8>, level: i32, kind: i32, fds: &[i32]) {
        let data_len = fds.len() * size_of::<i32>();
        let length = size_of::<CMsgHdr>() + data_len;
        let span = cmsg_align(length).unwrap();
        let offset = control.len();
        control.resize(offset + span, 0);
        control[offset..offset + 8].copy_from_slice(&(length as u64).to_ne_bytes());
        control[offset + 8..offset + 12].copy_from_slice(&level.to_ne_bytes());
        control[offset + 12..offset + 16].copy_from_slice(&kind.to_ne_bytes());
        for (index, fd) in fds.iter().enumerate() {
            let start = offset + size_of::<CMsgHdr>() + index * size_of::<i32>();
            control[start..start + size_of::<i32>()].copy_from_slice(&fd.to_ne_bytes());
        }
    }

    fn parse(control: &[u8]) -> Result<Vec<Fd>, SysError> {
        parse_rights_control_with(control.len(), &mut |offset, target| {
            let source = control
                .get(offset..offset + target.len())
                .ok_or(SysError::BadAddress)?;
            target.copy_from_slice(source);
            Ok(())
        })
    }

    #[kunit]
    fn native_rights_parser_preserves_multi_cmsg_order_and_empty_groups() {
        let mut control = Vec::new();
        append_cmsg(&mut control, SOL_SOCKET, SCM_RIGHTS, &[]);
        append_cmsg(&mut control, SOL_SOCKET, SCM_RIGHTS, &[7, 3, 11]);
        append_cmsg(&mut control, SOL_SOCKET, SCM_RIGHTS, &[5]);
        assert_eq!(
            parse(&control),
            Ok(vec![
                Fd::new(7).unwrap(),
                Fd::new(3).unwrap(),
                Fd::new(11).unwrap(),
                Fd::new(5).unwrap(),
            ])
        );

        let exact = &control[16..16 + size_of::<CMsgHdr>() + 3 * size_of::<i32>()];
        assert_eq!(parse(exact).unwrap().len(), 3);
    }

    #[kunit]
    fn native_rights_parser_rejects_malformed_unsupported_and_oversized_control() {
        assert_eq!(parse(&[0; 15]), Err(SysError::InvalidArgument));
        assert_eq!(cmsg_align(usize::MAX), None);

        let mut malformed = Vec::new();
        append_cmsg(&mut malformed, SOL_SOCKET, SCM_RIGHTS, &[1]);
        malformed[0..8].copy_from_slice(&15u64.to_ne_bytes());
        assert_eq!(parse(&malformed), Err(SysError::InvalidArgument));

        let mut unsupported = Vec::new();
        append_cmsg(&mut unsupported, SOL_SOCKET + 1, SCM_RIGHTS, &[1]);
        assert_eq!(parse(&unsupported), Err(SysError::NotSupported));

        let too_many = vec![1; UNIX_SCM_RIGHTS_MAX_FDS_PER_MESSAGE + 1];
        let mut oversized = Vec::new();
        append_cmsg(&mut oversized, SOL_SOCKET, SCM_RIGHTS, &too_many);
        assert_eq!(parse(&oversized), Err(SysError::InvalidArgument));
    }

    #[kunit]
    fn native_rights_parser_reports_copy_fault_without_partial_result() {
        let mut control = Vec::new();
        append_cmsg(&mut control, SOL_SOCKET, SCM_RIGHTS, &[1, 2]);
        let mut reads = 0usize;
        let result = parse_rights_control_with(control.len(), &mut |offset, target| {
            reads += 1;
            if reads == 2 {
                return Err(SysError::BadAddress);
            }
            target.copy_from_slice(&control[offset..offset + target.len()]);
            Ok(())
        });
        assert_eq!(result, Err(SysError::BadAddress));
    }

    #[kunit]
    fn rights_capture_propagates_bad_fd_and_rejects_unix_kind_before_commit() {
        let fds = [Fd::new(3).unwrap()];
        let bad = capture_parsed_rights(&fds, |_| Err(SysError::BadFileDescriptor), |_| false);
        assert!(matches!(bad, Err(SysError::BadFileDescriptor)));

        let (bundle, identity) = OpenedDescriptionBundle::for_kunit(1);
        let rejected = capture_parsed_rights(&fds, |_| Ok(bundle), |_| true);
        assert!(matches!(rejected, Err(SysError::NotSupported)));
        assert!(identity.try_lease().is_none());

        let (bundle, identity) = OpenedDescriptionBundle::for_kunit(1);
        let accepted = capture_parsed_rights(&fds, |_| Ok(bundle), |_| false)
            .unwrap()
            .unwrap();
        assert_eq!(accepted.len(), 1);
        drop(accepted);
        assert!(identity.try_lease().is_none());
    }

    #[kunit]
    fn sendmmsg_driver_preserves_partial_and_fail_forward_completion() {
        let mut sent = Vec::new();
        let mut lengths = Vec::new();
        assert_eq!(
            drive_send_messages(
                3,
                |index| Ok(index as u64),
                |entry| {
                    sent.push(entry);
                    Ok(MessageSendResult {
                        sent: entry + 1,
                        complete: entry != 1,
                    })
                },
                |entry, length| {
                    lengths.push((entry, length));
                    Ok(())
                },
            ),
            Ok(2)
        );
        assert_eq!(sent, vec![0, 1]);
        assert_eq!(lengths, vec![(0, 1), (1, 2)]);

        assert_eq!(
            drive_send_messages(
                2,
                |index| Ok(index as u64),
                |_| Err(SysError::BadFileDescriptor),
                |_, _| Ok(()),
            ),
            Err(SysError::BadFileDescriptor)
        );

        let mut commits = Vec::new();
        let result = drive_send_messages(
            3,
            |index| Ok(index as u64),
            |entry| {
                commits.push(entry);
                Ok(MessageSendResult {
                    sent: 1,
                    complete: true,
                })
            },
            |entry, _| {
                if entry == 1 {
                    Err(SysError::BadAddress)
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(result, Ok(1));
        assert_eq!(commits, vec![0, 1]);

        let mut first_commit = 0usize;
        assert_eq!(
            drive_send_messages(
                1,
                |_| Ok(0),
                |_| {
                    first_commit += 1;
                    Ok(MessageSendResult {
                        sent: 1,
                        complete: true,
                    })
                },
                |_, _| Err(SysError::BadAddress),
            ),
            Err(SysError::BadAddress)
        );
        assert_eq!(first_commit, 1);
    }
}
