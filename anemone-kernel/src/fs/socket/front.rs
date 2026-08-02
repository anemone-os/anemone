//! Family-neutral Socket file, static dispatch, and opened-description hooks.

use anemone_net_api::Ipv4Address;

use crate::{
    prelude::*,
    task::{
        files::{
            FileDescOps, OpenedFileFinalReleaseCtx, OpenedFileReadUserCtx, OpenedFileWriteUserCtx,
        },
        sig::{
            SigNo, Signal,
            info::{SiCode, SigInfoFields, SigKill},
        },
    },
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketType {
    Ipv4Udp,
    UnixStream,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketAddress {
    Ipv4 { address: Ipv4Address, port: u16 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketBindError {
    Unsupported,
    Retired,
    AlreadyBound,
    AddressInUse,
    AddressUnavailable,
    ResourceExhausted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketQueryError {
    Unsupported,
    Retired,
    Copy(SysError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketSendError {
    Unsupported,
    Retired,
    InvalidState,
    AddressInUse,
    AddressUnavailable,
    ResourceExhausted,
    NetworkUnreachable,
    InvalidDestination,
    MessageTooLong,
    PeerClosed,
    WouldBlock,
    Copy(SysError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketReceiveError {
    Unsupported,
    Retired,
    WouldBlock,
    Copy(SysError),
}

pub(super) trait SocketSendPayload {
    fn bytes(&mut self) -> Result<&[u8], SysError>;
}

pub(super) trait SocketAddressSink {
    fn copy_address(&mut self, address: Option<SocketAddress>) -> Result<(), SysError>;
}

pub(super) trait SocketReceiveSink {
    fn copy_datagram(&mut self, payload: &[u8], peer: SocketAddress) -> Result<usize, SysError>;
}

pub(super) trait SocketStreamReadSink {
    fn remaining(&self) -> usize;

    fn copy_bytes(&mut self, bytes: &[u8]) -> Result<usize, SysError>;
}

pub(super) trait SocketStreamWriteSource {
    fn remaining(&self) -> usize;

    fn copy_bytes(&mut self, bytes: &mut [u8]) -> Result<usize, SysError>;
}

pub(super) enum SocketSendRequest<'a> {
    Datagram {
        peer: SocketAddress,
        payload: &'a mut dyn SocketSendPayload,
    },
    Stream(&'a mut dyn SocketStreamWriteSource),
}

pub(super) enum SocketReceiveRequest<'a> {
    Datagram(&'a mut dyn SocketReceiveSink),
    Stream(&'a mut dyn SocketStreamReadSink),
}

pub(super) struct SocketPreparation {
    pub(super) private: AnyOpaque,
    pub(super) creation: SocketCreation,
}

pub(super) struct SocketPairPreparation {
    pub(super) first_private: AnyOpaque,
    pub(super) second_private: AnyOpaque,
}

pub(super) struct SocketOps {
    pub(super) socket_type: SocketType,
    pub(super) create: Option<fn() -> Result<SocketPreparation, SysError>>,
    pub(super) create_pair: Option<fn() -> Result<SocketPairPreparation, SysError>>,
    pub(super) bind: Option<fn(&AnyOpaque, SocketAddress) -> Result<(), SocketBindError>>,
    pub(super) local_address:
        Option<fn(&AnyOpaque, &mut dyn SocketAddressSink) -> Result<(), SocketQueryError>>,
    pub(super) send:
        Option<for<'a> fn(&AnyOpaque, SocketSendRequest<'a>) -> Result<usize, SocketSendError>>,
    pub(super) receive: Option<
        for<'a> fn(&AnyOpaque, SocketReceiveRequest<'a>) -> Result<usize, SocketReceiveError>,
    >,
    pub(super) poll:
        for<'a> fn(&AnyOpaque, &PollRequest<'a>) -> Result<PollRegisterResult, SysError>,
    pub(super) final_release: fn(&AnyOpaque),
}

#[derive(Opaque)]
pub(super) struct Socket {
    /// This immutable descriptor is the sole type witness. The front does not
    /// cache a family tag or any family-owned readiness/lifecycle fact.
    ops: &'static SocketOps,
    private: AnyOpaque,
}

impl core::fmt::Debug for Socket {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Socket")
            .field("socket_type", &self.ops.socket_type)
            .finish_non_exhaustive()
    }
}

impl Socket {
    pub(super) const fn socket_type(&self) -> SocketType {
        self.ops.socket_type
    }

    pub(super) fn bind(&self, address: SocketAddress) -> Result<(), SocketBindError> {
        self.ops.bind.ok_or(SocketBindError::Unsupported)?(&self.private, address)
    }

    pub(super) fn copy_local_address(
        &self,
        sink: &mut dyn SocketAddressSink,
    ) -> Result<(), SocketQueryError> {
        self.ops
            .local_address
            .ok_or(SocketQueryError::Unsupported)?(&self.private, sink)
    }

    pub(super) fn send(&self, request: SocketSendRequest<'_>) -> Result<usize, SocketSendError> {
        self.ops.send.ok_or(SocketSendError::Unsupported)?(&self.private, request)
    }

    pub(super) fn receive(
        &self,
        request: SocketReceiveRequest<'_>,
    ) -> Result<usize, SocketReceiveError> {
        self.ops.receive.ok_or(SocketReceiveError::Unsupported)?(&self.private, request)
    }

    fn poll(&self, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
        (self.ops.poll)(&self.private, request)
    }

    fn final_release(&self) {
        (self.ops.final_release)(&self.private);
    }
}

/// Owns family rollback authority until the prepared opened description is
/// published. The associated create operations alone interpret that authority.
pub(super) struct SocketCreation {
    pub(super) commit: fn(&mut AnyOpaque),
    pub(super) authority: AnyOpaque,
}

impl SocketCreation {
    pub(super) fn commit(mut self) {
        (self.commit)(&mut self.authority);
    }
}

pub(super) fn prepare_socket(ops: &'static SocketOps) -> Result<(File, SocketCreation), SysError> {
    let create = ops.create.ok_or(SysError::NotSupported)?;
    let SocketPreparation { private, creation } = create()?;
    let file = prepare_socket_file(ops, private)?;
    Ok((file, creation))
}

fn prepare_socket_file(ops: &'static SocketOps, private: AnyOpaque) -> Result<File, SysError> {
    let path = anony_new_inode(InodeType::Socket, &SOCKET_INODE_OPS, NilOpaque::new())?;
    let file = anony_open_with(
        &path,
        OpenedFile::with_mode(
            &SOCKET_FILE_OPS,
            FileMode::STREAM,
            AnyOpaque::new(Socket { ops, private }),
        ),
    )?;
    Ok(file)
}

pub(super) fn prepare_socket_pair(ops: &'static SocketOps) -> Result<(File, File), SysError> {
    let create_pair = ops.create_pair.ok_or(SysError::NotSupported)?;
    let SocketPairPreparation {
        first_private,
        second_private,
    } = create_pair()?;
    let first = prepare_socket_file(ops, first_private)?;
    let second = prepare_socket_file(ops, second_private)?;
    Ok((first, second))
}

pub(super) fn socket_from_file(file: &File) -> Option<&Socket> {
    file.uses_file_ops(&SOCKET_FILE_OPS).then(|| {
        file.private::<Socket>()
            .expect("common Socket FileOps used without Socket private state")
    })
}

fn final_release_socket(ctx: OpenedFileFinalReleaseCtx<'_>) {
    assert!(
        ctx.notification_suppressed,
        "Socket description lost notification-suppression capability"
    );
    socket_from_file(ctx.file)
        .expect("Socket final-release hook installed on a non-Socket file")
        .final_release();
}

fn socket_read_user_transaction(ctx: OpenedFileReadUserCtx<'_, '_>) -> Result<usize, SysError> {
    assert!(
        ctx.notification_suppressed,
        "Socket read transaction lost notification-suppression capability"
    );
    socket_read_with_ctx(
        ctx.file,
        ctx.dst,
        ctx.status_flags.to_file_op_status_flags(),
    )
}

fn socket_write_user_transaction(ctx: OpenedFileWriteUserCtx<'_, '_>) -> Result<usize, SysError> {
    assert!(
        ctx.notification_suppressed,
        "Socket write transaction lost notification-suppression capability"
    );
    socket_write_with_ctx(
        ctx.file,
        ctx.src,
        ctx.status_flags.to_file_op_status_flags(),
    )
}

pub(super) fn socket_file_desc_ops() -> FileDescOps {
    FileDescOps {
        read_user_transaction: Some(socket_read_user_transaction),
        write_user_transaction: Some(socket_write_user_transaction),
        notify_read_user_access: false,
        final_release: Some(final_release_socket),
        notification_suppressed: true,
        ..FileDescOps::default()
    }
}

struct SliceReadSink<'a> {
    bytes: &'a mut [u8],
}

impl SocketStreamReadSink for SliceReadSink<'_> {
    fn remaining(&self) -> usize {
        self.bytes.len()
    }

    fn copy_bytes(&mut self, bytes: &[u8]) -> Result<usize, SysError> {
        let copied = self.bytes.len().min(bytes.len());
        self.bytes[..copied].copy_from_slice(&bytes[..copied]);
        Ok(copied)
    }
}

impl SocketStreamReadSink for UserBufferSink<'_> {
    fn remaining(&self) -> usize {
        self.remaining()
    }

    fn copy_bytes(&mut self, bytes: &[u8]) -> Result<usize, SysError> {
        self.write_from_slice(bytes)
    }
}

struct SliceWriteSource<'a> {
    bytes: &'a [u8],
}

impl SocketStreamWriteSource for SliceWriteSource<'_> {
    fn remaining(&self) -> usize {
        self.bytes.len()
    }

    fn copy_bytes(&mut self, bytes: &mut [u8]) -> Result<usize, SysError> {
        let copied = self.bytes.len().min(bytes.len());
        bytes[..copied].copy_from_slice(&self.bytes[..copied]);
        Ok(copied)
    }
}

impl SocketStreamWriteSource for UserBufferSource<'_> {
    fn remaining(&self) -> usize {
        self.remaining()
    }

    fn copy_bytes(&mut self, bytes: &mut [u8]) -> Result<usize, SysError> {
        self.copy_into_slice(bytes)
    }
}

fn socket_read_with_ctx(
    file: &File,
    sink: &mut dyn SocketStreamReadSink,
    flags: FileOpStatusFlags,
) -> Result<usize, SysError> {
    loop {
        let result = socket_from_file(file)
            .expect("common Socket read used without Socket private state")
            .receive(SocketReceiveRequest::Stream(sink));
        match result {
            Ok(read) => return Ok(read),
            Err(SocketReceiveError::WouldBlock) if !flags.contains(FileOpStatusFlags::NONBLOCK) => {
                super::api::wait_for_socket_file(
                    "socket read",
                    &get_current_task(),
                    file,
                    PollEvent::READABLE,
                )?;
            },
            Err(SocketReceiveError::WouldBlock) => return Err(SysError::Again),
            Err(SocketReceiveError::Unsupported) => return Err(SysError::NotSupported),
            Err(SocketReceiveError::Retired) => return Err(SysError::BadFileDescriptor),
            Err(SocketReceiveError::Copy(error)) => return Err(error),
        }
    }
}

fn send_sigpipe() {
    let task = get_current_task();
    task.recv_signal(Signal::new(
        SigNo::SIGPIPE,
        SiCode::Kernel,
        SigInfoFields::Kill(SigKill {
            pid: task.tgid(),
            uid: task.cred().uid.real,
        }),
    ));
}

fn socket_write_with_ctx(
    file: &File,
    source: &mut dyn SocketStreamWriteSource,
    flags: FileOpStatusFlags,
) -> Result<usize, SysError> {
    loop {
        let result = socket_from_file(file)
            .expect("common Socket write used without Socket private state")
            .send(SocketSendRequest::Stream(source));
        match result {
            Ok(written) => return Ok(written),
            Err(SocketSendError::WouldBlock) if !flags.contains(FileOpStatusFlags::NONBLOCK) => {
                super::api::wait_for_socket_file(
                    "socket write",
                    &get_current_task(),
                    file,
                    PollEvent::WRITABLE,
                )?;
            },
            Err(SocketSendError::WouldBlock) => return Err(SysError::Again),
            Err(SocketSendError::PeerClosed) => {
                send_sigpipe();
                return Err(SysError::BrokenPipe);
            },
            Err(SocketSendError::Unsupported) => return Err(SysError::NotSupported),
            Err(SocketSendError::Retired) => return Err(SysError::BadFileDescriptor),
            Err(SocketSendError::Copy(error)) => return Err(error),
            Err(SocketSendError::InvalidState)
            | Err(SocketSendError::AddressInUse)
            | Err(SocketSendError::AddressUnavailable)
            | Err(SocketSendError::ResourceExhausted)
            | Err(SocketSendError::NetworkUnreachable)
            | Err(SocketSendError::InvalidDestination)
            | Err(SocketSendError::MessageTooLong) => {
                unreachable!("datagram-only send outcome reached stream FileOps")
            },
        }
    }
}

fn socket_read(
    file: &File,
    _pos: &mut usize,
    bytes: &mut [u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    socket_read_with_ctx(file, &mut SliceReadSink { bytes }, ctx.status_flags())
}

fn socket_write(
    file: &File,
    _pos: &mut usize,
    bytes: &[u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    socket_write_with_ctx(file, &mut SliceWriteSource { bytes }, ctx.status_flags())
}

fn socket_check_status_flags(_file: &File, flags: FileOpStatusFlags) -> Result<(), SysError> {
    if flags.contains(FileOpStatusFlags::DIRECT) {
        return Err(SysError::InvalidArgument);
    }
    Ok(())
}

static SOCKET_FILE_OPS: FileOps = FileOps {
    read: socket_read,
    write: socket_write,
    read_at: |_, _, _, _| Err(SysError::IllegalSeek),
    write_at: |_, _, _, _| Err(SysError::IllegalSeek),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: socket_check_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |file, request| {
        socket_from_file(file)
            .expect("common Socket FileOps poll used without Socket private state")
            .poll(request)
    },
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

fn socket_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let meta = inode.inode().meta_snapshot();
    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: inode.mode(),
        nlink: meta.nlink,
        uid: meta.uid,
        gid: meta.gid,
        rdev: DeviceId::None,
        size: meta.size,
        atime: meta.atime,
        mtime: meta.mtime,
        ctime: meta.ctime,
    })
}

static SOCKET_INODE_OPS: InodeOps = InodeOps {
    make_node: reject_make_node,
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: |_| unreachable!("Socket files are opened with explicit private state"),
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: socket_get_attr,
};

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    use crate::fs::socket::{UDP_SOCKET_OPS, UNIX_STREAM_SOCKET_OPS};

    #[kunit]
    fn static_descriptor_is_the_only_socket_type_witness() {
        let (file, creation) = prepare_socket(&UDP_SOCKET_OPS).expect("KUnit UDP socket must fit");
        let socket = socket_from_file(&file).expect("prepared file must be a common Socket");
        assert_eq!(socket.socket_type(), SocketType::Ipv4Udp);
        drop(creation);
    }

    #[kunit]
    fn zero_length_stream_requests_preserve_family_semantics() {
        let (first, second) =
            prepare_socket_pair(&UNIX_STREAM_SOCKET_OPS).expect("Unix KUnit pair must fit");
        let first_socket = socket_from_file(&first).expect("first file must be a Socket");
        let second_socket = socket_from_file(&second).expect("second file must be a Socket");

        let mut byte = SliceWriteSource { bytes: b"x" };
        assert_eq!(
            first_socket.send(SocketSendRequest::Stream(&mut byte)),
            Ok(1)
        );
        let mut empty_bytes = [];
        let mut empty = SliceReadSink {
            bytes: &mut empty_bytes,
        };
        assert_eq!(
            second_socket.receive(SocketReceiveRequest::Stream(&mut empty)),
            Ok(0)
        );
        let mut received = [0u8; 1];
        let mut sink = SliceReadSink {
            bytes: &mut received,
        };
        assert_eq!(
            second_socket.receive(SocketReceiveRequest::Stream(&mut sink)),
            Ok(1)
        );
        assert_eq!(received, *b"x");

        second_socket.final_release();
        let mut empty = SliceWriteSource { bytes: b"" };
        assert_eq!(
            first_socket.send(SocketSendRequest::Stream(&mut empty)),
            Ok(0)
        );
        let mut byte = SliceWriteSource { bytes: b"x" };
        assert_eq!(
            first_socket.send(SocketSendRequest::Stream(&mut byte)),
            Err(SocketSendError::PeerClosed)
        );

        let (udp_file, creation) =
            prepare_socket(&UDP_SOCKET_OPS).expect("KUnit UDP endpoint must fit");
        let udp_socket = socket_from_file(&udp_file).expect("UDP file must be a Socket");
        let mut empty = SliceWriteSource { bytes: b"" };
        assert_eq!(
            udp_socket.send(SocketSendRequest::Stream(&mut empty)),
            Err(SocketSendError::Unsupported)
        );
        drop(creation);
    }
}
