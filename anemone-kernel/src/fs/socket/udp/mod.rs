//! UDP-private Socket state and its static common-front operations.

mod source;

use anemone_net_api::udp::{UdpBindError, UdpPeer, UdpQueryError, UdpReceiveError, UdpSendError};

use crate::{
    kconfig_defs::NET_UDP_MAX_PAYLOAD_BYTES,
    net::udp::{BindError, SendError, UdpEndpointPort, create_endpoint},
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::{
    SocketAddress, SocketAddressSink, SocketBindError, SocketCreation, SocketOps,
    SocketPreparation, SocketQueryError, SocketReceiveError, SocketReceiveOutcome,
    SocketReceiveRequest, SocketSendError, SocketSendRequest, SocketType,
};
use source::UdpSocketSource;

#[derive(Opaque)]
struct UdpSocketFile {
    source: Arc<UdpSocketSource>,
    /// Serializes UDP state-changing operation attempts. It owns no Endpoint
    /// fact and is deliberately absent from final release.
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

/// Owns rollback authority until the common Socket description is published.
/// Capability clones never own semantic lifetime.
#[derive(Opaque)]
struct UdpSocketCreation {
    source: Option<Arc<UdpSocketSource>>,
}

impl UdpSocketCreation {
    fn commit(&mut self) {
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

fn udp_private(private: &AnyOpaque) -> &UdpSocketFile {
    private
        .cast::<UdpSocketFile>()
        .expect("UDP SocketOps used without UDP private state")
}

fn prepare_udp_socket() -> Result<SocketPreparation, SysError> {
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
    Ok(SocketPreparation {
        private: AnyOpaque::new(UdpSocketFile::new(source.clone())),
        creation: SocketCreation {
            commit: commit_udp_socket,
            authority: AnyOpaque::new(UdpSocketCreation {
                source: Some(source),
            }),
        },
    })
}

fn commit_udp_socket(creation: &mut AnyOpaque) {
    creation
        .cast_mut::<UdpSocketCreation>()
        .expect("UDP creation commit used without UDP creation authority")
        .commit();
}

fn bind_udp_socket(private: &AnyOpaque, address: SocketAddress) -> Result<(), SocketBindError> {
    let SocketAddress::Ipv4 { address, port } = address else {
        return Err(SocketBindError::Unsupported);
    };
    let socket = udp_private(private);
    let _operation = socket.operation.lock();
    socket
        .endpoint()
        .ok_or(SocketBindError::Retired)?
        .bind(address, port)
        .map(|_| ())
        .map_err(map_bind_error)
}

fn query_udp_socket(
    private: &AnyOpaque,
    sink: &mut dyn SocketAddressSink,
) -> Result<(), SocketQueryError> {
    let socket = udp_private(private);
    let _operation = socket.operation.lock();
    let address = socket
        .endpoint()
        .ok_or(SocketQueryError::Retired)?
        .binding()
        .map(|binding| {
            binding.map(|binding| SocketAddress::Ipv4 {
                address: binding.address(),
                port: binding.port(),
            })
        })
        .map_err(|error| match error {
            UdpQueryError::UnknownEndpoint => SocketQueryError::Retired,
        })?;
    sink.copy_address(address).map_err(SocketQueryError::Copy)
}

fn udp_is_accepting(_private: &AnyOpaque) -> Result<bool, SocketQueryError> {
    Ok(false)
}

fn send_udp_socket(
    private: &AnyOpaque,
    request: SocketSendRequest<'_>,
) -> Result<usize, SocketSendError> {
    let SocketSendRequest::Datagram {
        destination,
        payload,
        operation: _,
    } = request
    else {
        return Err(SocketSendError::Unsupported);
    };
    let (address, port) = match destination {
        Some(SocketAddress::Ipv4 { address, port }) => (address, port),
        None => return Err(SocketSendError::DestinationRequired),
        Some(SocketAddress::Unspecified | SocketAddress::UnixPathname(_)) => {
            return Err(SocketSendError::Unsupported);
        },
    };
    let socket = udp_private(private);
    let _operation = socket.operation.lock();
    let endpoint = socket.endpoint().ok_or(SocketSendError::Retired)?;
    // Implicit binding is a persistent commit. Keep the family operation
    // guard across the typed user-copy cursor so later MTU/capacity rejection
    // cannot bypass that commit or change the existing serialization boundary.
    endpoint.ensure_bound().map_err(map_send_error)?;
    let payload = payload
        .bytes(NET_UDP_MAX_PAYLOAD_BYTES)
        .map_err(SocketSendError::Copy)?;
    let len = payload.len();
    endpoint
        .send(UdpPeer::new(address, port), payload)
        .map_err(map_send_error)?;
    Ok(len)
}

fn receive_udp_socket(
    private: &AnyOpaque,
    request: SocketReceiveRequest<'_>,
) -> Result<SocketReceiveOutcome, SocketReceiveError> {
    let SocketReceiveRequest::Datagram { sink, flags } = request else {
        return Err(SocketReceiveError::Unsupported);
    };
    if flags.peek {
        return Err(SocketReceiveError::Unsupported);
    }
    let socket = udp_private(private);
    let _operation = socket.operation.lock();
    let datagram = socket
        .endpoint()
        .ok_or(SocketReceiveError::Retired)?
        .receive()
        .map_err(|error| match error {
            UdpReceiveError::UnknownEndpoint => SocketReceiveError::Retired,
            UdpReceiveError::WouldBlock => SocketReceiveError::WouldBlock,
        })?;
    let peer = datagram.peer();
    let packet_length = datagram.payload().len();
    let copied = sink
        .copy_datagram(
            datagram.payload(),
            SocketAddress::Ipv4 {
                address: peer.address(),
                port: peer.port(),
            },
        )
        .map_err(SocketReceiveError::Copy)?;
    Ok(SocketReceiveOutcome::datagram(copied, packet_length))
}

fn poll_udp_socket(
    private: &AnyOpaque,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    udp_private(private).source.poll(request)
}

fn final_release_udp_socket(private: &AnyOpaque) {
    // Source retirement first withdraws association, reverse publication and
    // routes. No sleeping operation mutex or fd-table lock participates.
    let result = udp_private(private).source.retire();
    assert!(
        result.is_ok(),
        "UDP final release lost its endpoint identity"
    );
}

fn map_bind_error(error: BindError) -> SocketBindError {
    match error {
        BindError::AddressUnavailable => SocketBindError::AddressUnavailable,
        BindError::Stack(UdpBindError::UnknownEndpoint) => SocketBindError::Retired,
        BindError::Stack(UdpBindError::AlreadyBound) => SocketBindError::AlreadyBound,
        BindError::Stack(UdpBindError::PortInUse) => SocketBindError::AddressInUse,
        BindError::Stack(UdpBindError::EphemeralPortsExhausted) => {
            SocketBindError::ResourceExhausted
        },
    }
}

fn map_send_error(error: SendError) -> SocketSendError {
    match error {
        SendError::Bind(UdpBindError::UnknownEndpoint) => SocketSendError::Retired,
        SendError::Bind(UdpBindError::AlreadyBound) => SocketSendError::InvalidState,
        SendError::Bind(UdpBindError::PortInUse) => SocketSendError::AddressInUse,
        SendError::Bind(UdpBindError::EphemeralPortsExhausted) => {
            SocketSendError::ResourceExhausted
        },
        SendError::NoRoute | SendError::InterfaceUnavailable => SocketSendError::NetworkUnreachable,
        SendError::SourceUnavailable => SocketSendError::AddressUnavailable,
        SendError::Stack(UdpSendError::UnknownEndpoint) => SocketSendError::Retired,
        SendError::Stack(UdpSendError::UnboundEndpoint) => SocketSendError::InvalidState,
        SendError::Stack(UdpSendError::UnknownInterface) => SocketSendError::NetworkUnreachable,
        SendError::Stack(UdpSendError::UnsupportedSource) => SocketSendError::AddressUnavailable,
        SendError::Stack(UdpSendError::InvalidDestination) => SocketSendError::InvalidDestination,
        SendError::Stack(UdpSendError::MessageTooLong { .. }) => SocketSendError::MessageTooLong,
        SendError::Stack(UdpSendError::WouldBlock) => SocketSendError::WouldBlock,
    }
}

pub(super) static UDP_SOCKET_OPS: SocketOps = SocketOps {
    socket_type: SocketType::Ipv4Udp,
    file_io: super::SocketFileIo::Unsupported,
    create: Some(prepare_udp_socket),
    create_pair: None,
    bind: Some(bind_udp_socket),
    listen: None,
    connect: None,
    accept: None,
    shutdown: None,
    local_address: Some(query_udp_socket),
    peer_address: None,
    accepting: udp_is_accepting,
    send: Some(send_udp_socket),
    send_wait: None,
    receive: Some(receive_udp_socket),
    query_option: None,
    mutate_option: None,
    poll: poll_udp_socket,
    final_release: final_release_udp_socket,
};

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    use anemone_abi::fs::linux::{mode, statx};

    use crate::{
        fs::socket::{SocketAddressSink, prepare_socket, socket_file_desc_ops, socket_from_file},
        task::files::{OpenAccessMode, OpenedFileFinalReleaseCtx},
    };

    #[derive(Default)]
    struct AddressCapture(Option<SocketAddress>);

    impl SocketAddressSink for AddressCapture {
        fn copy_address(&mut self, address: Option<SocketAddress>) -> Result<(), SysError> {
            self.0 = address;
            Ok(())
        }
    }

    #[kunit]
    fn common_file_association_projects_udp_through_static_ops() {
        let (file, creation) =
            prepare_socket(&UDP_SOCKET_OPS).expect("KUnit UDP endpoint must fit");
        let socket = socket_from_file(&file).expect("prepared UDP file must be a Socket");
        let mut address = AddressCapture::default();
        socket.copy_local_address(&mut address).unwrap();
        assert_eq!(address.0, None);
        creation.commit();
        (socket_file_desc_ops().final_release.unwrap())(OpenedFileFinalReleaseCtx {
            file: &file,
            access: OpenAccessMode::ReadWrite,
            notification_suppressed: true,
        });
    }

    #[kunit]
    fn common_creation_guard_retires_udp_before_publication() {
        let (file, creation) =
            prepare_socket(&UDP_SOCKET_OPS).expect("KUnit UDP endpoint must fit");
        let socket = socket_from_file(&file).expect("prepared UDP file must be a Socket");
        drop(creation);
        assert_eq!(
            socket.copy_local_address(&mut AddressCapture::default()),
            Err(SocketQueryError::Retired)
        );
    }

    #[kunit]
    fn common_socket_inode_projects_linux_socket_type() {
        let (file, creation) =
            prepare_socket(&UDP_SOCKET_OPS).expect("KUnit UDP endpoint must fit");
        let attr = file
            .inode()
            .get_attr()
            .expect("Socket inode must report attrs");
        assert_eq!(attr.to_linux_stat().st_mode & mode::S_IFMT, mode::S_IFSOCK);
        assert_eq!(
            u32::from(attr.to_linux_statx(statx::BASIC_STATS).stx_mode) & mode::S_IFMT,
            mode::S_IFSOCK
        );
        drop(creation);
    }
}
