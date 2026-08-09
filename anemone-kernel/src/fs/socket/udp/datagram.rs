//! UDP address and datagram operations behind the static Socket facade.

use anemone_net_api::udp::{
    UdpBindError, UdpPeekOutcome, UdpPeer, UdpQueryError, UdpReceiveError, UdpReceiveOutcome,
    UdpSendError,
};

use crate::{
    fs::socket::{
        SocketAddress, SocketAddressSink, SocketBindError, SocketConnectError,
        SocketDatagramSendOperation, SocketQueryError, SocketReceiveError, SocketReceiveOutcome,
        SocketReceiveRequest, SocketReceiveSink, SocketSendError, SocketSendRequest,
    },
    kconfig_defs::NET_UDP_MAX_PAYLOAD_BYTES,
    net::udp::{BindError, ConnectError, SendError, UdpEndpointPort},
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::{error::map_pending_error, udp_private};

#[derive(Opaque)]
pub(super) struct UdpSendSnapshot {
    /// Captured from the explicit destination or Endpoint peer on the first
    /// attempt. It is intentionally stale across wait/retry and reconnect,
    /// and is discarded when this send operation returns.
    pub(super) destination: UdpPeer,
}

pub(super) fn bind_udp_socket(
    private: &AnyOpaque,
    address: SocketAddress,
) -> Result<(), SocketBindError> {
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

pub(super) fn query_udp_socket(
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

pub(super) fn connect_udp_socket(
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
        SocketAddress::UnixPathname(_) | SocketAddress::Netlink { .. } => {
            Err(SocketConnectError::Unsupported)
        },
    }
}

pub(super) fn query_udp_peer(
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

pub(super) fn udp_is_accepting(_private: &AnyOpaque) -> Result<bool, SocketQueryError> {
    Ok(false)
}

pub(super) fn send_udp_socket(
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
        Some(
            SocketAddress::Unspecified
            | SocketAddress::UnixPathname(_)
            | SocketAddress::Netlink { .. },
        ) => {
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
    operation: &mut SocketDatagramSendOperation,
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

pub(super) fn receive_udp_socket(
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

fn copy_udp_datagram(
    payload: &[u8],
    peer: UdpPeer,
    sink: &mut dyn SocketReceiveSink,
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
