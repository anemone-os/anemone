//! UDP error option and detached-record projection behind the Socket facade.

use anemone_net_api::udp::{UdpErrorCause, UdpQueryError};

use crate::{
    fs::socket::{
        SocketAddress, SocketIpv4ExtendedError, SocketOptionError, SocketOptionMutation,
        SocketOptionQuery, SocketOptionValue, SocketPendingError, SocketReceiveError,
    },
    utils::any_opaque::AnyOpaque,
};

use super::udp_private;

pub(super) fn query_udp_option(
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

pub(super) fn mutate_udp_option(
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

pub(super) fn detach_udp_extended_error(
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

pub(super) const fn map_pending_error(error: UdpErrorCause) -> SocketPendingError {
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
