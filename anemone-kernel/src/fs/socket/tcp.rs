//! Syscall-unreachable TCP integration with the family-neutral Socket front.

use anemone_net_api::tcp::{
    TcpBindError, TcpChildError, TcpConnectError, TcpConnectionObservation, TcpCreateError,
    TcpDisconnectCause, TcpListenError, TcpPeer, TcpQueryError, TcpReceiveError,
    TcpReceiveResolveError, TcpSendError,
};

use crate::{
    kconfig_defs::{NET_TCP_RX_BUFFER_BYTES, NET_TCP_TX_BUFFER_BYTES},
    net::tcp::{BindError, ConnectError, ListenError, TcpEndpointPort, create_endpoint},
    prelude::*,
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use super::{
    SocketAcceptError, SocketAcceptItem, SocketAddress, SocketAddressSink, SocketBindError,
    SocketConnectError, SocketCreation, SocketIoOps, SocketListenError, SocketOps,
    SocketPreparation, SocketQueryError, SocketReceiveError, SocketReceiveOutcome,
    SocketReceiveRequest, SocketSendError, SocketSendRequest, SocketStreamDestination, SocketType,
    SocketWait,
};

#[derive(Opaque)]
struct TcpSocketFile {
    source: Arc<TcpSocketSource>,
    /// Serializes family operation attempts without owning any Stack fact.
    /// Final release deliberately bypasses this sleeping guard.
    operation: Mutex<()>,
}

impl TcpSocketFile {
    fn new(source: Arc<TcpSocketSource>) -> Self {
        Self {
            source,
            operation: Mutex::new(()),
        }
    }
}

struct TcpSocketSource {
    /// The move-only Endpoint capability is the sole kernel association. The
    /// binding, role, peer, stream, cause, and readiness facts remain in the
    /// Stack owner and are queried for each operation.
    endpoint: SpinLock<Option<TcpEndpointPort>>,
}

impl TcpSocketSource {
    fn new(endpoint: TcpEndpointPort) -> Self {
        Self {
            endpoint: SpinLock::new(Some(endpoint)),
        }
    }

    fn with_live<R>(&self, operation: impl FnOnce(&TcpEndpointPort) -> R) -> Option<R> {
        self.endpoint.lock().as_ref().map(operation)
    }

    fn retire(&self) -> bool {
        let endpoint = self.endpoint.lock().take();
        let Some(endpoint) = endpoint else {
            return false;
        };
        endpoint.retire();
        true
    }
}

impl Drop for TcpSocketSource {
    fn drop(&mut self) {
        let endpoint = self.endpoint.lock().take();
        if let Some(endpoint) = endpoint {
            // This is a fail-close assertion path, not a second lifecycle
            // trigger: withdraw and retire first so the diagnostic cannot leak
            // a live Endpoint if an unpublished/final-release owner is lost.
            endpoint.retire();
            panic!("TCP Socket source dropped before lifecycle-owned retirement");
        }
    }
}

#[derive(Opaque)]
struct TcpSocketCreation {
    source: Option<Arc<TcpSocketSource>>,
}

impl TcpSocketCreation {
    fn commit(&mut self) {
        self.source.take();
    }
}

impl Drop for TcpSocketCreation {
    fn drop(&mut self) {
        let Some(source) = self.source.take() else {
            return;
        };
        assert!(
            source.retire(),
            "TCP creation rollback lost its Endpoint capability"
        );
    }
}

fn tcp_private(private: &AnyOpaque) -> &TcpSocketFile {
    private
        .cast::<TcpSocketFile>()
        .expect("TCP SocketOps used without TCP private state")
}

fn tcp_private_from_endpoint(endpoint: TcpEndpointPort) -> AnyOpaque {
    AnyOpaque::new(TcpSocketFile::new(Arc::new(TcpSocketSource::new(endpoint))))
}

fn prepare_tcp_socket() -> Result<SocketPreparation, SysError> {
    let endpoint = create_endpoint().map_err(|error| match error {
        TcpCreateError::EndpointCapacity => SysError::NoBufferSpace,
    })?;
    let source = Arc::new(TcpSocketSource::new(endpoint));
    Ok(SocketPreparation {
        private: AnyOpaque::new(TcpSocketFile::new(source.clone())),
        creation: SocketCreation {
            commit: commit_tcp_socket,
            authority: AnyOpaque::new(TcpSocketCreation {
                source: Some(source),
            }),
        },
    })
}

fn commit_tcp_socket(creation: &mut AnyOpaque) {
    creation
        .cast_mut::<TcpSocketCreation>()
        .expect("TCP creation commit used without TCP creation authority")
        .commit();
}

fn bind_tcp_socket(private: &AnyOpaque, address: SocketAddress) -> Result<(), SocketBindError> {
    let SocketAddress::Ipv4 { address, port } = address else {
        return Err(SocketBindError::Unsupported);
    };
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    socket
        .source
        .with_live(|endpoint| endpoint.bind(address, port))
        .ok_or(SocketBindError::Retired)?
        .map(|_| ())
        .map_err(map_bind_error)
}

fn query_tcp_local_address(
    private: &AnyOpaque,
    sink: &mut dyn SocketAddressSink,
) -> Result<(), SocketQueryError> {
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    let binding = socket
        .source
        .with_live(TcpEndpointPort::binding)
        .ok_or(SocketQueryError::Retired)?
        .map_err(map_query_error)?;
    sink.copy_address(binding.map(|binding| SocketAddress::Ipv4 {
        address: binding.address(),
        port: binding.port(),
    }))
    .map_err(SocketQueryError::Copy)
}

fn query_tcp_peer_address(
    private: &AnyOpaque,
    sink: &mut dyn SocketAddressSink,
) -> Result<(), SocketQueryError> {
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    let observation = socket
        .source
        .with_live(TcpEndpointPort::connection)
        .ok_or(SocketQueryError::Retired)?
        .map_err(map_query_error)?;
    let peer = connection_peer(observation).ok_or(SocketQueryError::NotConnected)?;
    sink.copy_address(Some(SocketAddress::Ipv4 {
        address: peer.address(),
        port: peer.port(),
    }))
    .map_err(SocketQueryError::Copy)
}

fn connect_tcp_socket(
    private: &AnyOpaque,
    address: SocketAddress,
) -> Result<(), SocketConnectError> {
    let SocketAddress::Ipv4 { address, port } = address else {
        return Err(SocketConnectError::Unsupported);
    };
    let peer = TcpPeer::new(address, port);
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    socket
        .source
        .with_live(|endpoint| connect_tcp_endpoint(endpoint, peer))
        .ok_or(SocketConnectError::Retired)?
}

fn connect_tcp_endpoint(
    endpoint: &TcpEndpointPort,
    peer: TcpPeer,
) -> Result<(), SocketConnectError> {
    match endpoint.connection().map_err(|error| match error {
        TcpQueryError::UnknownEndpoint => SocketConnectError::Retired,
        TcpQueryError::WrongRole => SocketConnectError::InvalidState,
    })? {
        TcpConnectionObservation::Idle | TcpConnectionObservation::Bound(_) => endpoint
            .connect(peer)
            .map(|()| Err(SocketConnectError::Started))
            .map_err(map_connect_start_error)?,
        TcpConnectionObservation::Connecting { .. } => Err(SocketConnectError::InProgress),
        TcpConnectionObservation::Connected { .. } => Err(SocketConnectError::AlreadyConnected),
        TcpConnectionObservation::Failed {
            cause: TcpDisconnectCause::Reset,
            ..
        } => Err(SocketConnectError::ConnectionRefused),
        TcpConnectionObservation::Failed {
            cause: TcpDisconnectCause::Timeout,
            ..
        } => Err(SocketConnectError::ConnectionTimedOut),
    }
}

fn listen_tcp_socket(private: &AnyOpaque, _backlog: i32) -> Result<(), SocketListenError> {
    // Stage 2 proves only the internal listener/child route. Linux backlog
    // normalization and per-listener admission remain unpublished Stage 3/5
    // work; the Stack owner still enforces its configured bounded capacity.
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    socket
        .source
        .with_live(TcpEndpointPort::listen)
        .ok_or(SocketListenError::Retired)?
        .map_err(map_listen_error)
}

fn tcp_is_accepting(private: &AnyOpaque) -> Result<bool, SocketQueryError> {
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    let result = socket
        .source
        .with_live(TcpEndpointPort::connection)
        .ok_or(SocketQueryError::Retired)?;
    match result {
        Ok(_) => Ok(false),
        Err(TcpQueryError::WrongRole) => {
            // The current TCP owner vocabulary reserves WrongRole from this
            // query for its sole non-connection role: listening. Re-read it on
            // every call; this result is never cached as Socket role truth.
            Ok(true)
        },
        Err(TcpQueryError::UnknownEndpoint) => Err(SocketQueryError::Retired),
    }
}

fn accept_tcp_socket(private: &AnyOpaque) -> Result<SocketAcceptItem, SocketAcceptError> {
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    let child = socket
        .source
        .with_live(TcpEndpointPort::claim_child)
        .ok_or(SocketAcceptError::Retired)?
        .map_err(map_child_error)?
        .ok_or_else(tcp_accept_would_block)?;
    let endpoint = child.accept().map_err(map_child_error)?;
    let observation = endpoint
        .connection()
        .expect("completed TCP child lost its connection observation");
    let peer =
        connection_peer(observation).expect("completed TCP child did not carry a peer observation");
    Ok(SocketAcceptItem {
        private: tcp_private_from_endpoint(endpoint),
        peer_address: Some(SocketAddress::Ipv4 {
            address: peer.address(),
            port: peer.port(),
        }),
    })
}

fn tcp_accept_would_block() -> SocketAcceptError {
    SocketAcceptError::WouldBlock(SocketWait::new(NilOpaque::new(), poll_unpublished_tcp))
}

fn send_tcp_socket(
    private: &AnyOpaque,
    request: SocketSendRequest<'_>,
) -> Result<usize, SocketSendError> {
    let SocketSendRequest::Stream {
        source,
        destination,
    } = request
    else {
        return Err(SocketSendError::Unsupported);
    };
    if destination == SocketStreamDestination::Present {
        return Err(SocketSendError::AlreadyConnected);
    }
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    let maximum = source.remaining().min(NET_TCP_TX_BUFFER_BYTES);
    if maximum == 0 {
        return Ok(0);
    }
    let mut bytes = vec![0; maximum];
    let copied = source
        .copy_bytes(&mut bytes)
        .map_err(SocketSendError::Copy)?;
    assert!(
        copied <= maximum,
        "TCP source copied beyond its offered prefix"
    );
    if copied == 0 {
        return Ok(0);
    }
    socket
        .source
        .with_live(|endpoint| endpoint.send(&bytes[..copied]))
        .ok_or(SocketSendError::Retired)?
        .map_err(map_send_error)
}

fn receive_tcp_socket(
    private: &AnyOpaque,
    request: SocketReceiveRequest<'_>,
) -> Result<SocketReceiveOutcome, SocketReceiveError> {
    let SocketReceiveRequest::Stream { sink, flags } = request else {
        return Err(SocketReceiveError::Unsupported);
    };
    if flags.peek {
        return Err(SocketReceiveError::Unsupported);
    }
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    let maximum = sink.remaining().min(NET_TCP_RX_BUFFER_BYTES);
    if maximum == 0 {
        return Ok(SocketReceiveOutcome::byte_stream(0));
    }
    let reservation = socket
        .source
        .with_live(|endpoint| endpoint.receive(maximum))
        .ok_or(SocketReceiveError::Retired)?
        .map_err(map_receive_error)?;
    let copied = sink
        .copy_bytes(reservation.bytes())
        .map_err(SocketReceiveError::Copy)?;
    assert!(
        copied <= reservation.bytes().len(),
        "TCP sink copied beyond its owner reservation"
    );
    reservation
        .commit(copied)
        .map_err(map_receive_resolve_error)?;
    Ok(SocketReceiveOutcome::byte_stream(copied))
}

fn poll_unpublished_tcp(
    _private: &AnyOpaque,
    _request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    // The static capability bundle requires a poll entry, but Stage 2 does not
    // publish TCP or claim readiness. Stage 4 must replace this bridge with an
    // owner-predicate source before any creation tuple becomes reachable.
    Err(SysError::NotSupported)
}

fn final_release_tcp_socket(private: &AnyOpaque) {
    // Withdraw the only kernel Endpoint association without taking the
    // sleeping operation guard. Stack retirement and progression remain
    // non-blocking and infallible by the CKPT 2A capability contract.
    assert!(
        tcp_private(private).source.retire(),
        "TCP final release lost its Endpoint capability"
    );
}

fn connection_peer(observation: TcpConnectionObservation) -> Option<TcpPeer> {
    match observation {
        TcpConnectionObservation::Connecting { peer, .. }
        | TcpConnectionObservation::Connected { peer, .. }
        | TcpConnectionObservation::Failed { peer, .. } => Some(peer),
        TcpConnectionObservation::Idle | TcpConnectionObservation::Bound(_) => None,
    }
}

fn map_query_error(error: TcpQueryError) -> SocketQueryError {
    match error {
        TcpQueryError::UnknownEndpoint => SocketQueryError::Retired,
        TcpQueryError::WrongRole => SocketQueryError::NotConnected,
    }
}

fn map_bind_error(error: BindError) -> SocketBindError {
    match error {
        BindError::AddressUnavailable => SocketBindError::AddressUnavailable,
        BindError::Stack(TcpBindError::UnknownEndpoint) => SocketBindError::Retired,
        BindError::Stack(TcpBindError::WrongRole) => SocketBindError::AlreadyBound,
        BindError::Stack(TcpBindError::PortInUse) => SocketBindError::AddressInUse,
        BindError::Stack(TcpBindError::EphemeralPortsExhausted) => {
            SocketBindError::ResourceExhausted
        },
    }
}

fn map_connect_start_error(error: ConnectError) -> SocketConnectError {
    match error {
        ConnectError::NoRoute | ConnectError::InterfaceUnavailable => {
            SocketConnectError::Operation(SysError::NetworkUnreachable)
        },
        ConnectError::SourceUnavailable => {
            SocketConnectError::Operation(SysError::AddressNotAvailable)
        },
        ConnectError::Stack(TcpConnectError::UnknownEndpoint) => SocketConnectError::Retired,
        ConnectError::Stack(TcpConnectError::WrongRole) => SocketConnectError::InvalidState,
        ConnectError::Stack(TcpConnectError::InvalidPeer) => SocketConnectError::InvalidState,
        ConnectError::Stack(TcpConnectError::UnknownInterface) => {
            SocketConnectError::Operation(SysError::NetworkUnreachable)
        },
        ConnectError::Stack(TcpConnectError::UnsupportedSource) => {
            SocketConnectError::Operation(SysError::AddressNotAvailable)
        },
        ConnectError::Stack(TcpConnectError::EngineCapacity) => {
            SocketConnectError::Operation(SysError::NoBufferSpace)
        },
        ConnectError::Stack(TcpConnectError::PortInUse) => {
            SocketConnectError::Operation(SysError::AddressInUse)
        },
        ConnectError::Stack(TcpConnectError::EphemeralPortsExhausted) => {
            SocketConnectError::Operation(SysError::Again)
        },
    }
}

fn map_listen_error(error: ListenError) -> SocketListenError {
    match error {
        ListenError::AddressUnavailable => SocketListenError::InvalidState,
        ListenError::Stack(TcpListenError::UnknownEndpoint) => SocketListenError::Retired,
        ListenError::Stack(
            TcpListenError::UnknownInterface
            | TcpListenError::WrongRole
            | TcpListenError::PortInUse
            | TcpListenError::EphemeralPortsExhausted,
        ) => SocketListenError::InvalidState,
        ListenError::Stack(TcpListenError::EngineCapacity) => SocketListenError::ResourceExhausted,
    }
}

fn map_child_error(error: TcpChildError) -> SocketAcceptError {
    match error {
        TcpChildError::UnknownEndpoint => SocketAcceptError::Retired,
        TcpChildError::WrongRole | TcpChildError::StaleChild | TcpChildError::ChildNotCompleted => {
            SocketAcceptError::InvalidState
        },
        TcpChildError::EndpointCapacity | TcpChildError::EngineCapacity => {
            SocketAcceptError::ResourceExhausted
        },
    }
}

fn map_send_error(error: TcpSendError) -> SocketSendError {
    match error {
        TcpSendError::UnknownEndpoint => SocketSendError::Retired,
        TcpSendError::NotConnected => SocketSendError::NotConnected,
        TcpSendError::WouldBlock => SocketSendError::WouldBlock,
    }
}

fn map_receive_error(error: TcpReceiveError) -> SocketReceiveError {
    match error {
        TcpReceiveError::UnknownEndpoint => SocketReceiveError::Retired,
        TcpReceiveError::NotConnected => SocketReceiveError::InvalidState,
        TcpReceiveError::WouldBlock => SocketReceiveError::WouldBlock,
        TcpReceiveError::ReservationOutstanding => SocketReceiveError::InvalidState,
    }
}

fn map_receive_resolve_error(error: TcpReceiveResolveError) -> SocketReceiveError {
    match error {
        TcpReceiveResolveError::UnknownReservation | TcpReceiveResolveError::InvalidPrefix => {
            SocketReceiveError::InvalidState
        },
    }
}

pub(super) static TCP_SOCKET_OPS: SocketOps = SocketOps {
    io: SocketIoOps::ByteStream {
        socket_type: SocketType::Ipv4Tcp,
        send: send_tcp_socket,
        receive: receive_tcp_socket,
    },
    create: Some(prepare_tcp_socket),
    create_pair: None,
    bind: Some(bind_tcp_socket),
    listen: Some(listen_tcp_socket),
    connect: Some(connect_tcp_socket),
    accept: Some(accept_tcp_socket),
    shutdown: None,
    local_address: Some(query_tcp_local_address),
    peer_address: Some(query_tcp_peer_address),
    accepting: tcp_is_accepting,
    query_option: None,
    mutate_option: None,
    poll: poll_unpublished_tcp,
    final_release: final_release_tcp_socket,
};

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn owner_capacity_exhaustion_remains_typed_at_the_socket_boundary() {
        assert_eq!(
            map_listen_error(ListenError::Stack(TcpListenError::EngineCapacity)),
            SocketListenError::ResourceExhausted
        );
        for error in [
            TcpChildError::EndpointCapacity,
            TcpChildError::EngineCapacity,
        ] {
            assert!(matches!(
                map_child_error(error),
                SocketAcceptError::ResourceExhausted
            ));
        }
    }
}
