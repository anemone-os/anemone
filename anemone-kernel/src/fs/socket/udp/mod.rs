//! UDP-private Socket state and its static common-front operations.

mod source;

use anemone_net_api::udp::{
    UdpBindError, UdpErrorCause, UdpPeekOutcome, UdpPeer, UdpQueryError, UdpReceiveError,
    UdpReceiveOutcome, UdpSendError,
};

use crate::{
    kconfig_defs::NET_UDP_MAX_PAYLOAD_BYTES,
    net::udp::{BindError, ConnectError, SendError, UdpEndpointPort, create_endpoint},
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::{
    SocketAddress, SocketAddressSink, SocketBindError, SocketConnectError, SocketCreation,
    SocketIoOps, SocketIpv4ExtendedError, SocketOps, SocketOptionError, SocketOptionMutation,
    SocketOptionQuery, SocketOptionValue, SocketPendingError, SocketPreparation, SocketQueryError,
    SocketReceiveError, SocketReceiveOutcome, SocketReceiveRequest, SocketReleaseReason,
    SocketSendError, SocketSendRequest, SocketType,
};
use source::UdpSocketSource;

#[derive(Opaque)]
struct UdpSocketFile {
    source: Arc<UdpSocketSource>,
    /// Serializes UDP state-changing operation attempts. It owns no Endpoint
    /// fact and is deliberately absent from final release.
    operation: Mutex<()>,
}

#[derive(Opaque)]
struct UdpSendSnapshot {
    /// Captured from the explicit destination or Endpoint peer on the first
    /// attempt. It is intentionally stale across wait/retry and reconnect,
    /// and is discarded when this send operation returns.
    destination: UdpPeer,
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

fn connect_udp_socket(
    private: &AnyOpaque,
    address: SocketAddress,
) -> Result<(), SocketConnectError> {
    let socket = udp_private(private);
    let _operation = socket.operation.lock();
    let endpoint = socket.endpoint().ok_or(SocketConnectError::Retired)?;
    match address {
        SocketAddress::Unspecified => endpoint.disconnect().map_err(|error| match error {
            UdpQueryError::UnknownEndpoint => SocketConnectError::Retired,
        }),
        SocketAddress::Ipv4 { address, port } => endpoint
            .connect(UdpPeer::new(address, port))
            .map_err(map_connect_error),
        SocketAddress::UnixPathname(_) => Err(SocketConnectError::Unsupported),
    }
}

fn query_udp_peer(
    private: &AnyOpaque,
    sink: &mut dyn SocketAddressSink,
) -> Result<(), SocketQueryError> {
    let socket = udp_private(private);
    let _operation = socket.operation.lock();
    let peer = socket
        .endpoint()
        .ok_or(SocketQueryError::Retired)?
        .peer()
        .map_err(|error| match error {
            UdpQueryError::UnknownEndpoint => SocketQueryError::Retired,
        })?
        .ok_or(SocketQueryError::NotConnected)?;
    sink.copy_address(Some(SocketAddress::Ipv4 {
        address: peer.address(),
        port: peer.port(),
    }))
    .map_err(SocketQueryError::Copy)
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
        operation,
    } = request
    else {
        return Err(SocketSendError::Unsupported);
    };
    let destination = match destination {
        Some(SocketAddress::Ipv4 { address, port }) => Some(UdpPeer::new(address, port)),
        None => None,
        Some(SocketAddress::Unspecified | SocketAddress::UnixPathname(_)) => {
            return Err(SocketSendError::Unsupported);
        },
    };
    let socket = udp_private(private);
    let _operation = socket.operation.lock();
    let endpoint = socket.endpoint().ok_or(SocketSendError::Retired)?;
    let destination = prepare_udp_send_destination(&endpoint, destination, operation)?;
    // Implicit binding is a persistent commit. Keep the family operation
    // guard across the typed user-copy cursor so later MTU/capacity rejection
    // cannot bypass that commit or change the existing serialization boundary.
    endpoint.ensure_bound().map_err(map_send_error)?;
    let payload = payload
        .bytes(NET_UDP_MAX_PAYLOAD_BYTES)
        .map_err(SocketSendError::Copy)?;
    let len = payload.len();
    endpoint
        .send(Some(destination), payload)
        .map_err(map_send_error)?;
    Ok(len)
}

fn prepare_udp_send_destination(
    endpoint: &UdpEndpointPort,
    explicit: Option<UdpPeer>,
    operation: &mut super::SocketDatagramSendOperation,
) -> Result<UdpPeer, SocketSendError> {
    if operation.family_snapshot::<UdpSendSnapshot>().is_none() {
        let destination = match explicit {
            Some(destination) => destination,
            None => endpoint
                .peer()
                .map_err(|error| match error {
                    UdpQueryError::UnknownEndpoint => SocketSendError::Retired,
                })?
                .ok_or(SocketSendError::DestinationRequired)?,
        };
        operation.install_family_snapshot(UdpSendSnapshot { destination });
    }
    operation
        .family_snapshot::<UdpSendSnapshot>()
        .map(|snapshot| snapshot.destination)
        .ok_or_else(|| panic!("UDP send reused another family's datagram operation snapshot"))
}

fn receive_udp_socket(
    private: &AnyOpaque,
    request: SocketReceiveRequest<'_>,
) -> Result<SocketReceiveOutcome, SocketReceiveError> {
    let SocketReceiveRequest::Datagram { sink, flags } = request else {
        return Err(SocketReceiveError::Unsupported);
    };
    let socket = udp_private(private);
    let _operation = socket.operation.lock();
    let endpoint = socket.endpoint().ok_or(SocketReceiveError::Retired)?;
    if flags.peek {
        match endpoint.peek().map_err(map_receive_error)? {
            UdpPeekOutcome::Datagram(datagram) => {
                copy_udp_datagram(datagram.payload(), datagram.peer(), sink)
            },
            UdpPeekOutcome::PendingError(error) => {
                Err(SocketReceiveError::Pending(map_pending_error(error)))
            },
        }
    } else {
        match endpoint.receive().map_err(map_receive_error)? {
            UdpReceiveOutcome::Datagram(datagram) => {
                copy_udp_datagram(datagram.payload(), datagram.peer(), sink)
            },
            UdpReceiveOutcome::PendingError(error) => {
                Err(SocketReceiveError::Pending(map_pending_error(error)))
            },
        }
    }
}

fn query_udp_option(
    private: &AnyOpaque,
    query: SocketOptionQuery,
) -> Result<SocketOptionValue, SocketOptionError> {
    let socket = udp_private(private);
    let _operation = socket.operation.lock();
    let endpoint = socket.endpoint().ok_or(SocketOptionError::Retired)?;
    match query {
        SocketOptionQuery::ReceiveErrors => endpoint
            .receive_errors_enabled()
            .map(SocketOptionValue::Boolean)
            .map_err(map_option_query_error),
        SocketOptionQuery::PendingError => endpoint
            .take_pending_error()
            .map(|error| SocketOptionValue::PendingError(error.map(map_pending_error)))
            .map_err(map_option_query_error),
        _ => Err(SocketOptionError::Unsupported),
    }
}

fn mutate_udp_option(
    private: &AnyOpaque,
    mutation: SocketOptionMutation,
) -> Result<(), SocketOptionError> {
    let socket = udp_private(private);
    let _operation = socket.operation.lock();
    let endpoint = socket.endpoint().ok_or(SocketOptionError::Retired)?;
    match mutation {
        SocketOptionMutation::ReceiveErrors(enabled) => endpoint
            .set_receive_errors(enabled)
            .map_err(map_option_query_error),
        _ => Err(SocketOptionError::Unsupported),
    }
}

fn detach_udp_extended_error(
    private: &AnyOpaque,
) -> Result<SocketIpv4ExtendedError, SocketReceiveError> {
    let socket = udp_private(private);
    let _operation = socket.operation.lock();
    let endpoint = socket.endpoint().ok_or(SocketReceiveError::Retired)?;
    let record = endpoint
        .detach_error()
        .map_err(map_receive_query_error)?
        .ok_or(SocketReceiveError::WouldBlock)?;
    let (cause, icmp_type, icmp_code, info, destination, offender, quoted_payload) =
        record.into_parts();
    Ok(SocketIpv4ExtendedError {
        cause: map_pending_error(cause),
        icmp_type,
        icmp_code,
        info,
        original_destination: SocketAddress::Ipv4 {
            address: destination.address(),
            port: destination.port(),
        },
        offender,
        quoted_payload,
    })
}

fn map_receive_query_error(error: UdpQueryError) -> SocketReceiveError {
    match error {
        UdpQueryError::UnknownEndpoint => SocketReceiveError::Retired,
    }
}

fn map_option_query_error(error: UdpQueryError) -> SocketOptionError {
    match error {
        UdpQueryError::UnknownEndpoint => SocketOptionError::Retired,
    }
}

const fn map_pending_error(error: UdpErrorCause) -> SocketPendingError {
    match error {
        UdpErrorCause::NetworkUnreachable
        | UdpErrorCause::DestinationNetworkUnknown
        | UdpErrorCause::NetworkProhibited
        | UdpErrorCause::NetworkUnreachableForTypeOfService => {
            SocketPendingError::NetworkUnreachable
        },
        UdpErrorCause::HostUnreachable
        | UdpErrorCause::HostProhibited
        | UdpErrorCause::HostUnreachableForTypeOfService
        | UdpErrorCause::CommunicationProhibited
        | UdpErrorCause::HostPrecedenceViolation
        | UdpErrorCause::PrecedenceCutoff
        | UdpErrorCause::TimeExceeded => SocketPendingError::HostUnreachable,
        UdpErrorCause::ProtocolUnreachable => SocketPendingError::ProtocolOptionNotSupported,
        UdpErrorCause::PortUnreachable => SocketPendingError::ConnectionRefused,
        UdpErrorCause::MessageTooLong => SocketPendingError::MessageTooLong,
        UdpErrorCause::SourceRouteFailed => SocketPendingError::OperationNotSupported,
        UdpErrorCause::DestinationHostUnknown => SocketPendingError::HostDown,
        UdpErrorCause::SourceHostIsolated => SocketPendingError::NoNetwork,
    }
}

fn copy_udp_datagram(
    payload: &[u8],
    peer: UdpPeer,
    sink: &mut dyn super::SocketReceiveSink,
) -> Result<SocketReceiveOutcome, SocketReceiveError> {
    let packet_length = payload.len();
    let copied = sink
        .copy_datagram(
            payload,
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

fn final_release_udp_socket(private: &AnyOpaque, _reason: SocketReleaseReason) {
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

fn map_connect_error(error: ConnectError) -> SocketConnectError {
    match error {
        ConnectError::NoRoute | ConnectError::InterfaceUnavailable => {
            SocketConnectError::Operation(SysError::NetworkUnreachable)
        },
        ConnectError::SourceUnavailable => {
            SocketConnectError::Operation(SysError::AddressNotAvailable)
        },
        ConnectError::Stack(anemone_net_api::udp::UdpConnectError::UnknownEndpoint) => {
            SocketConnectError::Retired
        },
        ConnectError::Stack(anemone_net_api::udp::UdpConnectError::InvalidPeer) => {
            SocketConnectError::InvalidState
        },
        ConnectError::Stack(anemone_net_api::udp::UdpConnectError::UnknownInterface) => {
            SocketConnectError::Operation(SysError::NetworkUnreachable)
        },
        ConnectError::Stack(anemone_net_api::udp::UdpConnectError::UnsupportedSource) => {
            SocketConnectError::Operation(SysError::AddressNotAvailable)
        },
        ConnectError::Stack(anemone_net_api::udp::UdpConnectError::EphemeralPortsExhausted) => {
            SocketConnectError::Operation(SysError::Again)
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
        SendError::Stack(UdpSendError::DestinationRequired) => SocketSendError::DestinationRequired,
        SendError::Stack(UdpSendError::UnknownInterface) => SocketSendError::NetworkUnreachable,
        SendError::Stack(UdpSendError::UnsupportedSource) => SocketSendError::AddressUnavailable,
        SendError::Stack(UdpSendError::InvalidDestination) => SocketSendError::InvalidDestination,
        SendError::Stack(UdpSendError::MessageTooLong { .. }) => SocketSendError::MessageTooLong,
        SendError::Stack(UdpSendError::Pending(error)) => {
            SocketSendError::Pending(map_pending_error(error))
        },
        SendError::Stack(UdpSendError::WouldBlock) => SocketSendError::WouldBlock,
    }
}

fn map_receive_error(error: UdpReceiveError) -> SocketReceiveError {
    match error {
        UdpReceiveError::UnknownEndpoint => SocketReceiveError::Retired,
        UdpReceiveError::WouldBlock => SocketReceiveError::WouldBlock,
    }
}

pub(super) static UDP_SOCKET_OPS: SocketOps = SocketOps {
    io: SocketIoOps::Datagram {
        socket_type: SocketType::Ipv4Udp,
        send: send_udp_socket,
        receive: receive_udp_socket,
    },
    create: Some(prepare_udp_socket),
    create_pair: None,
    bind: Some(bind_udp_socket),
    listen: None,
    connect: Some(connect_udp_socket),
    accept: None,
    shutdown: None,
    local_address: Some(query_udp_socket),
    peer_address: Some(query_udp_peer),
    accepting: udp_is_accepting,
    query_option: Some(query_udp_option),
    mutate_option: Some(mutate_udp_option),
    detach_ipv4_extended_error: Some(detach_udp_extended_error),
    poll: poll_udp_socket,
    final_release: final_release_udp_socket,
};

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    use anemone_abi::fs::linux::{mode, statx};
    use anemone_net_api::Ipv4Address;

    use crate::{
        fs::socket::{
            SocketAddressSink, SocketDatagramSendOperation, SocketSendPayload, prepare_socket,
            socket_file_desc_ops, socket_from_file,
        },
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

    struct FaultSendPayload;

    impl SocketSendPayload for FaultSendPayload {
        fn bytes(&mut self, _maximum: usize) -> Result<&[u8], SysError> {
            Err(SysError::BadAddress)
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
    fn retry_keeps_connected_destination_snapshot_across_reconnect() {
        let (file, creation) =
            prepare_socket(&UDP_SOCKET_OPS).expect("KUnit UDP endpoint must fit");
        let socket = socket_from_file(&file).expect("prepared UDP file must be a Socket");
        let first_peer = Ipv4Address::LOOPBACK;
        let second_peer = Ipv4Address::new([127, 0, 0, 2]);
        assert!(
            socket
                .connect(SocketAddress::Ipv4 {
                    address: first_peer,
                    port: 7,
                })
                .is_ok()
        );

        let mut operation = SocketDatagramSendOperation::new();
        let mut payload = FaultSendPayload;
        assert_eq!(
            socket.send(SocketSendRequest::Datagram {
                destination: None,
                payload: &mut payload,
                operation: &mut operation,
            }),
            Err(SocketSendError::Copy(SysError::BadAddress))
        );
        assert_eq!(
            operation
                .family_snapshot::<UdpSendSnapshot>()
                .expect("first UDP send attempt must install a destination snapshot")
                .destination,
            UdpPeer::new(first_peer, 7)
        );

        assert!(
            socket
                .connect(SocketAddress::Ipv4 {
                    address: second_peer,
                    port: 9,
                })
                .is_ok()
        );
        assert_eq!(
            socket.send(SocketSendRequest::Datagram {
                destination: None,
                payload: &mut payload,
                operation: &mut operation,
            }),
            Err(SocketSendError::Copy(SysError::BadAddress))
        );
        assert_eq!(
            operation
                .family_snapshot::<UdpSendSnapshot>()
                .expect("retried UDP send must retain its first destination snapshot")
                .destination,
            UdpPeer::new(first_peer, 7)
        );

        drop(creation);
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
