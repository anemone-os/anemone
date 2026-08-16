//! Syscall-reachable TCP candidate integration with the family-neutral Socket
//! front.

mod lifecycle;
mod source;
mod stream;

use lifecycle::*;
use source::*;
use stream::*;

use anemone_net_api::tcp::{
    TcpBindError, TcpChildError, TcpConnectError, TcpConnectResult, TcpCreateError,
    TcpEndpointFacts, TcpListenBacklog, TcpListenError, TcpPeer, TcpPendingError, TcpQueryError,
    TcpReleaseReason,
};

use crate::{
    kconfig_defs::NET_TCP_LISTENER_COMPLETED_CAPACITY,
    net::tcp::{BindError, ConnectError, ListenError, create_endpoint},
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::{
    SocketAcceptError, SocketAcceptItem, SocketAddress, SocketAddressSink, SocketBindError,
    SocketConnectError, SocketCreation, SocketIoOps, SocketIoctlError, SocketIoctlRequest,
    SocketIoctlResponse, SocketListenError, SocketOps, SocketOptionMutation, SocketOptionQuery,
    SocketOptionValue, SocketPreparation, SocketQueryError, SocketReleaseReason, SocketType,
};

fn bind_tcp_socket(private: &AnyOpaque, address: SocketAddress) -> Result<(), SocketBindError> {
    let SocketAddress::Ipv4 { address, port } = address else {
        return Err(SocketBindError::Unsupported);
    };
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    socket
        .source
        .endpoint()
        .ok_or(SocketBindError::Retired)?
        .bind(address, port)
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
        .endpoint()
        .ok_or(SocketQueryError::Retired)?
        .binding()
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
    let peer = socket
        .source
        .endpoint()
        .ok_or(SocketQueryError::Retired)?
        .peer()
        .map_err(map_query_error)?
        .ok_or(SocketQueryError::NotConnected)?;
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
    let endpoint = socket
        .source
        .endpoint()
        .ok_or(SocketConnectError::Retired)?;
    match endpoint.connect_result().map_err(|error| match error {
        TcpQueryError::UnknownEndpoint => SocketConnectError::Retired,
        TcpQueryError::WrongRole => SocketConnectError::InvalidState,
    })? {
        TcpConnectResult::Idle | TcpConnectResult::Bound(_) => {
            endpoint.connect(peer).map_err(map_connect_start_error)?;
            Err(SocketConnectError::Started(TcpSocketSource::connect_wait(
                &socket.source,
            )))
        },
        TcpConnectResult::Connecting { .. } => Err(SocketConnectError::InProgress(
            TcpSocketSource::connect_wait(&socket.source),
        )),
        TcpConnectResult::Connected { .. } => Err(SocketConnectError::AlreadyConnected),
        TcpConnectResult::Failed(TcpPendingError::ConnectionRefused) => {
            Err(SocketConnectError::ConnectionRefused)
        },
        TcpConnectResult::Failed(TcpPendingError::TimedOut) => {
            Err(SocketConnectError::ConnectionTimedOut)
        },
        TcpConnectResult::Failed(TcpPendingError::ConnectionReset) => {
            Err(SocketConnectError::ConnectionReset)
        },
        TcpConnectResult::Terminal => Err(SocketConnectError::ConnectionAborted),
    }
}

fn listen_tcp_socket(private: &AnyOpaque, backlog: i32) -> Result<(), SocketListenError> {
    // Linux's signed backlog is normalized before crossing the owner fence.
    // The accepted R0 policy maps a negative or oversized request to the
    // configured admission ceiling; zero remains a real zero backlog.
    let normalized = normalize_tcp_backlog(backlog);
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    socket
        .source
        .endpoint()
        .ok_or(SocketListenError::Retired)?
        .listen_with_backlog(normalized)
        .map_err(map_listen_error)
}

const fn normalize_tcp_backlog(backlog: i32) -> TcpListenBacklog {
    TcpListenBacklog::new(if backlog < 0 {
        NET_TCP_LISTENER_COMPLETED_CAPACITY
    } else if backlog as usize > NET_TCP_LISTENER_COMPLETED_CAPACITY {
        NET_TCP_LISTENER_COMPLETED_CAPACITY
    } else {
        backlog as usize
    })
}

fn tcp_is_accepting(private: &AnyOpaque) -> Result<bool, SocketQueryError> {
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    socket
        .source
        .endpoint()
        .ok_or(SocketQueryError::Retired)?
        .is_listening()
        .map_err(map_query_error)
}

fn accept_tcp_socket(private: &AnyOpaque) -> Result<SocketAcceptItem, SocketAcceptError> {
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    let child = socket
        .source
        .endpoint()
        .ok_or(SocketAcceptError::Retired)?
        .claim_child()
        .map_err(map_child_error)?;
    let Some(child) = child else {
        return Err(SocketAcceptError::WouldBlock(TcpSocketSource::accept_wait(
            &socket.source,
        )));
    };
    let endpoint = child.accept().map_err(map_child_error)?;
    let peer = endpoint
        .access()
        .peer()
        .expect("completed TCP child lost its owner query")
        .expect("completed TCP child did not carry a peer");
    Ok(SocketAcceptItem {
        private: tcp_private_from_accepted_endpoint(endpoint)
            .map_err(|_| SocketAcceptError::ResourceExhausted)?,
        peer_address: Some(SocketAddress::Ipv4 {
            address: peer.address(),
            port: peer.port(),
        }),
    })
}

fn poll_tcp_socket(
    private: &AnyOpaque,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    tcp_private(private).source.poll(request)
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
            SocketConnectError::Operation(SysError::AddressNotAvailable)
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

fn ioctl_tcp_socket(
    private: &AnyOpaque,
    request: SocketIoctlRequest,
) -> Result<SocketIoctlResponse, SocketIoctlError> {
    let SocketIoctlRequest::ReadableBytes = request;
    let facts = tcp_private(private)
        .source
        .endpoint()
        .ok_or(SocketIoctlError::Retired)?
        .facts()
        .map_err(|error| match error {
            TcpQueryError::UnknownEndpoint => SocketIoctlError::Retired,
            TcpQueryError::WrongRole => SocketIoctlError::InvalidState,
        })?;
    let readable = match facts {
        TcpEndpointFacts::Idle | TcpEndpointFacts::Bound => 0,
        TcpEndpointFacts::Listener { .. } => return Err(SocketIoctlError::InvalidState),
        TcpEndpointFacts::Connection(connection) => connection.received_bytes(),
    };
    Ok(SocketIoctlResponse::ReadableBytes(readable))
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
    shutdown: Some(shutdown_tcp_socket),
    local_address: Some(query_tcp_local_address),
    peer_address: Some(query_tcp_peer_address),
    accepting: tcp_is_accepting,
    query_option: Some(query_tcp_option),
    mutate_option: Some(mutate_tcp_option),
    ioctl: Some(ioctl_tcp_socket),
    detach_ipv4_extended_error: None,
    poll: poll_tcp_socket,
    final_release: final_release_tcp_socket,
};

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::fs::socket::SocketOptionError;

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
        assert!(matches!(
            map_connect_start_error(ConnectError::Stack(
                TcpConnectError::EphemeralPortsExhausted
            )),
            SocketConnectError::Operation(SysError::AddressNotAvailable)
        ));
    }

    #[kunit]
    fn backlog_normalization_clamps_before_crossing_the_owner_fence() {
        assert_eq!(
            normalize_tcp_backlog(-1).get(),
            NET_TCP_LISTENER_COMPLETED_CAPACITY
        );
        assert_eq!(normalize_tcp_backlog(0).get(), 0);
        assert_eq!(normalize_tcp_backlog(1).get(), 1);
        assert_eq!(normalize_tcp_backlog(10).get(), 10);
        assert_eq!(
            normalize_tcp_backlog(i32::MAX).get(),
            NET_TCP_LISTENER_COMPLETED_CAPACITY
        );
    }

    #[kunit]
    fn unpublished_descriptor_projects_tcp_options_without_caching_owner_truth() {
        let SocketPreparation { private, creation } = prepare_tcp_socket().unwrap();
        creation.commit();
        assert_eq!(
            query_tcp_option(&private, SocketOptionQuery::PendingError),
            Ok(SocketOptionValue::PendingError(None))
        );
        mutate_tcp_option(&private, SocketOptionMutation::ReuseAddress(true)).unwrap();
        mutate_tcp_option(&private, SocketOptionMutation::TcpNoDelay(true)).unwrap();
        assert_eq!(
            query_tcp_option(&private, SocketOptionQuery::ReuseAddress),
            Ok(SocketOptionValue::Boolean(true))
        );
        assert_eq!(
            query_tcp_option(&private, SocketOptionQuery::TcpNoDelay),
            Ok(SocketOptionValue::Boolean(true))
        );
        final_release_tcp_socket(&private, SocketReleaseReason::FinalRelease);
        assert_eq!(
            query_tcp_option(&private, SocketOptionQuery::PendingError),
            Err(SocketOptionError::Retired)
        );
    }

    #[kunit]
    fn ioctl_dispatch_projects_idle_tcp_and_rejects_retired_endpoints() {
        let SocketPreparation { private, creation } = prepare_tcp_socket().unwrap();
        assert_eq!(
            ioctl_tcp_socket(&private, SocketIoctlRequest::ReadableBytes),
            Ok(SocketIoctlResponse::ReadableBytes(0))
        );
        drop(creation);
        assert_eq!(
            ioctl_tcp_socket(&private, SocketIoctlRequest::ReadableBytes),
            Err(SocketIoctlError::Retired)
        );
    }
}
