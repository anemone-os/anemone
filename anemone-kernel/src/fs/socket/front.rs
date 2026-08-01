//! Family-neutral Socket file, static dispatch, and opened-description hooks.

use anemone_net_api::Ipv4Address;

use crate::{
    prelude::*,
    task::files::{FileDescOps, OpenedFileFinalReleaseCtx},
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SocketType {
    Ipv4Udp,
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

pub(super) struct SocketPreparation {
    pub(super) private: AnyOpaque,
    pub(super) creation: SocketCreation,
}

pub(super) struct SocketOps {
    pub(super) socket_type: SocketType,
    pub(super) create: Option<fn() -> Result<SocketPreparation, SysError>>,
    pub(super) bind: Option<fn(&AnyOpaque, SocketAddress) -> Result<(), SocketBindError>>,
    pub(super) local_address:
        Option<fn(&AnyOpaque, &mut dyn SocketAddressSink) -> Result<(), SocketQueryError>>,
    pub(super) send: Option<
        fn(&AnyOpaque, SocketAddress, &mut dyn SocketSendPayload) -> Result<(), SocketSendError>,
    >,
    pub(super) receive:
        Option<fn(&AnyOpaque, &mut dyn SocketReceiveSink) -> Result<usize, SocketReceiveError>>,
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

    pub(super) fn send(
        &self,
        peer: SocketAddress,
        payload: &mut dyn SocketSendPayload,
    ) -> Result<(), SocketSendError> {
        self.ops.send.ok_or(SocketSendError::Unsupported)?(&self.private, peer, payload)
    }

    pub(super) fn receive(
        &self,
        sink: &mut dyn SocketReceiveSink,
    ) -> Result<usize, SocketReceiveError> {
        self.ops.receive.ok_or(SocketReceiveError::Unsupported)?(&self.private, sink)
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
    let path = anony_new_inode(InodeType::Socket, &SOCKET_INODE_OPS, NilOpaque::new())?;
    let file = anony_open_with(
        &path,
        OpenedFile::with_mode(
            &SOCKET_FILE_OPS,
            FileMode::STREAM,
            AnyOpaque::new(Socket { ops, private }),
        ),
    )?;
    Ok((file, creation))
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

pub(super) fn socket_file_desc_ops() -> FileDescOps {
    FileDescOps {
        final_release: Some(final_release_socket),
        notification_suppressed: true,
        ..FileDescOps::default()
    }
}

fn socket_check_status_flags(_file: &File, flags: FileOpStatusFlags) -> Result<(), SysError> {
    if flags.contains(FileOpStatusFlags::DIRECT) {
        return Err(SysError::InvalidArgument);
    }
    Ok(())
}

static SOCKET_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::NotSupported),
    write: |_, _, _, _| Err(SysError::NotSupported),
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

    use crate::fs::socket::UDP_SOCKET_OPS;

    #[kunit]
    fn static_descriptor_is_the_only_socket_type_witness() {
        let (file, creation) = prepare_socket(&UDP_SOCKET_OPS).expect("KUnit UDP socket must fit");
        let socket = socket_from_file(&file).expect("prepared file must be a common Socket");
        assert_eq!(socket.socket_type(), SocketType::Ipv4Udp);
        drop(creation);
    }
}
