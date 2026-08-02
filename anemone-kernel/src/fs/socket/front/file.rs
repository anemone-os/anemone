//! Common Socket file, opened-description, and anonymous-inode integration.

use crate::{
    prelude::*,
    task::files::{
        FileDescOps, OpenedFileFinalReleaseCtx, OpenedFileReadUserCtx, OpenedFileWriteUserCtx,
    },
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use super::{
    Socket, SocketOps, SocketReceiveError, SocketReceiveFlags, SocketReceiveRequest,
    SocketSendError, SocketSendRequest, SocketStreamDestination, SocketStreamReadSink,
    SocketStreamWriteSource,
    operation::{retry_socket_receive, retry_socket_send},
};

pub(super) fn prepare_socket_file(
    ops: &'static SocketOps,
    private: AnyOpaque,
) -> Result<File, SysError> {
    let path = prepare_socket_path()?;
    Ok(prepare_socket_file_at(&path, ops, private))
}

pub(super) fn prepare_socket_path() -> Result<PathRef, SysError> {
    anony_new_inode(InodeType::Socket, &SOCKET_INODE_OPS, NilOpaque::new())
}

pub(super) fn prepare_socket_file_at(
    path: &PathRef,
    ops: &'static SocketOps,
    private: AnyOpaque,
) -> File {
    anony_open_with(
        path,
        OpenedFile::with_mode(
            &SOCKET_FILE_OPS,
            FileMode::STREAM,
            AnyOpaque::new(Socket { ops, private }),
        ),
    )
    .expect("explicit anonymous Socket open is infallible")
}

pub(in crate::fs::socket) fn socket_from_file(file: &File) -> Option<&Socket> {
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

pub(in crate::fs::socket) fn socket_file_desc_ops() -> FileDescOps {
    FileDescOps {
        read_user_transaction: Some(socket_read_user_transaction),
        write_user_transaction: Some(socket_write_user_transaction),
        notify_read_user_access: false,
        final_release: Some(final_release_socket),
        notification_suppressed: true,
        ..FileDescOps::default()
    }
}

pub(super) struct SliceReadSink<'a> {
    pub(super) bytes: &'a mut [u8],
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

pub(super) struct SliceWriteSource<'a> {
    pub(super) bytes: &'a [u8],
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
    let task = get_current_task();
    retry_socket_receive(
        "socket read",
        &task,
        file,
        flags.contains(FileOpStatusFlags::NONBLOCK),
        || {
            socket_from_file(file)
                .expect("common Socket read used without Socket private state")
                .receive(SocketReceiveRequest::Stream {
                    sink,
                    flags: SocketReceiveFlags { peek: false },
                })
        },
        map_stream_file_receive_error,
    )
}

fn map_stream_file_receive_error(error: SocketReceiveError) -> SysError {
    match error {
        SocketReceiveError::WouldBlock => SysError::Again,
        SocketReceiveError::Unsupported => SysError::NotSupported,
        SocketReceiveError::Retired => SysError::BadFileDescriptor,
        SocketReceiveError::InvalidState => SysError::InvalidArgument,
        SocketReceiveError::Copy(error) => error,
    }
}

fn socket_write_with_ctx(
    file: &File,
    source: &mut dyn SocketStreamWriteSource,
    flags: FileOpStatusFlags,
) -> Result<usize, SysError> {
    let task = get_current_task();
    retry_socket_send(
        "socket write",
        &task,
        file,
        flags.contains(FileOpStatusFlags::NONBLOCK),
        true,
        || {
            socket_from_file(file)
                .expect("common Socket write used without Socket private state")
                .send(SocketSendRequest::Stream {
                    source,
                    destination: SocketStreamDestination::Absent,
                })
        },
        map_stream_file_send_error,
    )
}

fn map_stream_file_send_error(error: SocketSendError) -> SysError {
    match error {
        SocketSendError::WouldBlock => SysError::Again,
        SocketSendError::PeerClosed => SysError::BrokenPipe,
        SocketSendError::Unsupported => SysError::NotSupported,
        SocketSendError::Retired => SysError::BadFileDescriptor,
        SocketSendError::NotConnected => SysError::NotConnected,
        SocketSendError::AlreadyConnected => SysError::AlreadyConnected,
        SocketSendError::Copy(error) => error,
        SocketSendError::InvalidState
        | SocketSendError::AddressInUse
        | SocketSendError::AddressUnavailable
        | SocketSendError::ResourceExhausted
        | SocketSendError::NetworkUnreachable
        | SocketSendError::InvalidDestination
        | SocketSendError::MessageTooLong => {
            unreachable!("datagram-only send outcome reached stream FileOps")
        },
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
