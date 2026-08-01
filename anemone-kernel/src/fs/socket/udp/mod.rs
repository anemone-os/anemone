//! Anonymous UDP socket file and opened-description lifecycle association.

mod source;

use anemone_net_api::udp::{
    UdpLocalBinding, UdpPeer, UdpQueryError, UdpReceiveError, UdpReceivedDatagram,
};

use crate::{
    net::udp::{BindError, SendError, UdpEndpointPort, create_endpoint},
    prelude::*,
    task::files::{FileDescOps, OpenedFileFinalReleaseCtx},
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use source::UdpSocketSource;

#[derive(Opaque)]
pub(super) struct UdpSocketFile {
    source: Arc<UdpSocketSource>,
    /// Serializes operations on one opened description. It owns no endpoint
    /// state and is deliberately absent from final release.
    operation: Mutex<()>,
}

impl core::fmt::Debug for UdpSocketFile {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UdpSocketFile").finish_non_exhaustive()
    }
}

impl UdpSocketFile {
    fn new(source: Arc<UdpSocketSource>) -> Self {
        Self {
            source,
            operation: Mutex::new(()),
        }
    }

    fn endpoint(&self) -> Option<UdpEndpointPort> {
        self.source.endpoint()
    }
}

/// Owns rollback authority until the fully prepared file description becomes
/// visible in an fd table. Capability clones never own semantic lifetime.
pub(super) struct UdpSocketCreation {
    source: Option<Arc<UdpSocketSource>>,
}

impl UdpSocketCreation {
    pub(super) fn commit(mut self) {
        self.source.take();
    }
}

impl Drop for UdpSocketCreation {
    fn drop(&mut self) {
        let Some(source) = self.source.take() else {
            return;
        };
        let result = source.retire();
        assert!(
            result.is_ok(),
            "UDP socket creation rollback lost its endpoint identity"
        );
    }
}

pub(super) fn prepare_udp_socket() -> Result<(File, UdpSocketCreation), SysError> {
    let endpoint = create_endpoint().map_err(|error| match error {
        anemone_net_api::udp::UdpCreateError::EndpointCapacity => SysError::NoBufferSpace,
    })?;
    let source = match UdpSocketSource::try_new(endpoint.clone()) {
        Ok(source) => source,
        Err(error) => {
            let retired = endpoint.retire();
            assert!(
                retired.is_ok(),
                "UDP source allocation rollback lost its Endpoint"
            );
            return Err(error);
        },
    };
    let creation = UdpSocketCreation {
        source: Some(source.clone()),
    };
    let path = anony_new_inode(InodeType::Socket, &UDP_SOCKET_INODE_OPS, NilOpaque::new())?;
    let file = anony_open_with(
        &path,
        OpenedFile::with_mode(
            &UDP_SOCKET_FILE_OPS,
            FileMode::STREAM,
            AnyOpaque::new(UdpSocketFile::new(source)),
        ),
    )?;
    Ok((file, creation))
}

pub(super) fn udp_socket_from_file(file: &File) -> Option<&UdpSocketFile> {
    file.uses_file_ops(&UDP_SOCKET_FILE_OPS).then(|| {
        file.private::<UdpSocketFile>()
            .expect("UDP Socket FileOps used without UdpSocketFile private state")
    })
}

pub(super) fn bind_udp_socket(
    socket: &UdpSocketFile,
    address: anemone_net_api::Ipv4Address,
    port: u16,
) -> Result<(), BindError> {
    let _operation = socket.operation.lock();
    socket
        .endpoint()
        .ok_or(BindError::Stack(
            anemone_net_api::udp::UdpBindError::UnknownEndpoint,
        ))?
        .bind(address, port)
        .map(|_| ())
}

pub(super) fn query_udp_socket(
    socket: &UdpSocketFile,
) -> Result<(MutexGuard<'_, ()>, Option<UdpLocalBinding>), UdpQueryError> {
    let operation = socket.operation.lock();
    let binding = socket
        .endpoint()
        .ok_or(UdpQueryError::UnknownEndpoint)?
        .binding()?;
    Ok((operation, binding))
}

pub(super) struct UdpSendOperation<'a> {
    endpoint: UdpEndpointPort,
    _operation: MutexGuard<'a, ()>,
}

impl UdpSendOperation<'_> {
    pub(super) fn send(self, peer: UdpPeer, payload: &[u8]) -> Result<(), SendError> {
        self.endpoint.send(peer, payload)
    }
}

pub(super) fn begin_udp_send(socket: &UdpSocketFile) -> Result<UdpSendOperation<'_>, SendError> {
    let operation = socket.operation.lock();
    let endpoint = socket.endpoint().ok_or(SendError::Stack(
        anemone_net_api::udp::UdpSendError::UnknownEndpoint,
    ))?;
    // Implicit binding is a persistent commit. Keep the operation guard across
    // the later user copy so MTU/capacity rejection cannot bypass that commit.
    endpoint.ensure_bound()?;
    Ok(UdpSendOperation {
        endpoint,
        _operation: operation,
    })
}

pub(super) fn receive_udp_socket(
    socket: &UdpSocketFile,
) -> Result<(MutexGuard<'_, ()>, UdpReceivedDatagram), UdpReceiveError> {
    let operation = socket.operation.lock();
    let datagram = socket
        .endpoint()
        .ok_or(UdpReceiveError::UnknownEndpoint)?
        .receive()?;
    Ok((operation, datagram))
}

fn final_release_udp_socket(ctx: OpenedFileFinalReleaseCtx<'_>) {
    assert!(
        ctx.notification_suppressed,
        "UDP socket description lost notification-suppression capability"
    );
    let socket =
        udp_socket_from_file(ctx.file).expect("UDP final-release hook installed on a non-UDP file");
    // Source retirement first withdraws association, reverse publication and
    // routes. No sleeping operation mutex or fd-table lock participates.
    let result = socket.source.retire();
    assert!(
        result.is_ok(),
        "UDP final release lost its endpoint identity"
    );
}

pub(super) fn udp_file_desc_ops() -> FileDescOps {
    FileDescOps {
        final_release: Some(final_release_udp_socket),
        notification_suppressed: true,
        ..FileDescOps::default()
    }
}

fn udp_check_status_flags(_file: &File, flags: FileOpStatusFlags) -> Result<(), SysError> {
    if flags.contains(FileOpStatusFlags::DIRECT) {
        return Err(SysError::InvalidArgument);
    }
    Ok(())
}

static UDP_SOCKET_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::NotSupported),
    write: |_, _, _, _| Err(SysError::NotSupported),
    read_at: |_, _, _, _| Err(SysError::IllegalSeek),
    write_at: |_, _, _, _| Err(SysError::IllegalSeek),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: udp_check_status_flags,
    seek: |_, _, _| Err(SysError::IllegalSeek),
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |file, request| {
        udp_socket_from_file(file)
            .expect("UDP FileOps poll used without UDP source")
            .source
            .poll(request)
    },
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

fn udp_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
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

static UDP_SOCKET_INODE_OPS: InodeOps = InodeOps {
    make_node: reject_make_node,
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: |_| unreachable!("UDP socket files are opened with explicit private state"),
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: udp_get_attr,
};

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    use anemone_abi::fs::linux::{mode, statx};

    #[kunit]
    fn udp_file_association_projects_only_the_udp_socket() {
        let (file, creation) = prepare_udp_socket().expect("KUnit UDP endpoint must fit");
        let socket = udp_socket_from_file(&file).expect("prepared UDP file must classify");
        assert_eq!(query_udp_socket(socket).unwrap().1, None);
        creation.commit();
        final_release_udp_socket(OpenedFileFinalReleaseCtx {
            file: &file,
            access: crate::task::files::OpenAccessMode::ReadWrite,
            notification_suppressed: true,
        });
    }

    #[kunit]
    fn udp_creation_guard_retires_before_publication() {
        let (file, creation) = prepare_udp_socket().expect("KUnit UDP endpoint must fit");
        let socket = udp_socket_from_file(&file).expect("prepared UDP file must classify");
        drop(creation);
        assert!(matches!(
            query_udp_socket(socket),
            Err(anemone_net_api::udp::UdpQueryError::UnknownEndpoint)
        ));
    }

    #[kunit]
    fn udp_inode_projects_linux_socket_type() {
        let (file, creation) = prepare_udp_socket().expect("KUnit UDP endpoint must fit");
        let attr = file
            .inode()
            .get_attr()
            .expect("UDP inode must report attrs");
        assert_eq!(attr.to_linux_stat().st_mode & mode::S_IFMT, mode::S_IFSOCK);
        assert_eq!(
            u32::from(attr.to_linux_statx(statx::BASIC_STATS).stx_mode) & mode::S_IFMT,
            mode::S_IFSOCK
        );
        drop(creation);
    }
}
