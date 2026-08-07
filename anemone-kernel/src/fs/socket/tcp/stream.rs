//! TCP byte-stream, shutdown, and socket-option projection.

use anemone_net_api::tcp::{
    TcpBindError, TcpPendingError, TcpQueryError, TcpReceiveMode, TcpReceiveResolveError,
    TcpShutdownDirection, TcpShutdownError, TcpStreamReceiveError, TcpStreamSendError,
};

use crate::{
    kconfig_defs::{NET_TCP_RX_BUFFER_BYTES, NET_TCP_TX_BUFFER_BYTES},
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::{
    super::{
        SocketOptionError, SocketOptionMutation, SocketOptionQuery, SocketOptionValue,
        SocketPendingError, SocketReceiveError, SocketReceiveOutcome, SocketReceiveRequest,
        SocketSendError, SocketSendRequest, SocketShutdown, SocketShutdownError,
    },
    lifecycle::tcp_private,
};

pub(super) fn send_tcp_socket(
    private: &AnyOpaque,
    request: SocketSendRequest<'_>,
) -> Result<usize, SocketSendError> {
    let SocketSendRequest::Stream {
        source,
        destination: _destination,
    } = request
    else {
        return Err(SocketSendError::Unsupported);
    };
    // Linux copies a connected TCP send name for fault/range validation but
    // tcp_sendmsg ignores its contents. The general adapter owns that copy;
    // this family therefore treats both markers as the same stream operation.
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    let endpoint = socket.source.endpoint().ok_or(SocketSendError::Retired)?;
    let maximum = endpoint
        .stream_observation()
        .map_err(map_stream_query_send_error)?
        .send_capacity()
        .min(source.remaining())
        .min(NET_TCP_TX_BUFFER_BYTES);
    if maximum == 0 {
        let result = endpoint.send_stream(&[]).map_err(map_stream_send_error)?;
        return if source.remaining() == 0 {
            Ok(result)
        } else {
            // The zero-byte owner attempt above first consumes terminal error
            // or broken-stream truth. Success therefore means only that this
            // connected stream currently has no admission capacity.
            Err(SocketSendError::WouldBlock)
        };
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
    let accepted = endpoint
        .send_stream(&bytes[..copied])
        .map_err(map_stream_send_error)?;
    assert_eq!(
        accepted, copied,
        "serialized TCP send accepted less than its owner-advertised capacity"
    );
    Ok(accepted)
}

pub(super) fn receive_tcp_socket(
    private: &AnyOpaque,
    request: SocketReceiveRequest<'_>,
) -> Result<SocketReceiveOutcome, SocketReceiveError> {
    let SocketReceiveRequest::Stream { sink, flags } = request else {
        return Err(SocketReceiveError::Unsupported);
    };
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    let endpoint = socket
        .source
        .endpoint()
        .ok_or(SocketReceiveError::Retired)?;
    let maximum = sink.remaining().min(NET_TCP_RX_BUFFER_BYTES);
    let outcome = endpoint
        .receive_stream(
            maximum,
            if flags.peek {
                TcpReceiveMode::Peek
            } else {
                TcpReceiveMode::Consume
            },
        )
        .map_err(map_stream_receive_error)?;
    let crate::net::tcp::TcpReceiveOutcome::Data(reservation) = outcome else {
        return Ok(SocketReceiveOutcome::byte_stream(0));
    };
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

pub(super) fn shutdown_tcp_socket(
    private: &AnyOpaque,
    direction: SocketShutdown,
) -> Result<(), SocketShutdownError> {
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    socket
        .source
        .endpoint()
        .ok_or(SocketShutdownError::Retired)?
        .shutdown(match direction {
            SocketShutdown::Read => TcpShutdownDirection::Read,
            SocketShutdown::Write => TcpShutdownDirection::Write,
            SocketShutdown::ReadWrite => TcpShutdownDirection::ReadWrite,
        })
        .map(|_| ())
        .map_err(|error| match error {
            TcpShutdownError::UnknownEndpoint => SocketShutdownError::Retired,
            TcpShutdownError::NotConnected => SocketShutdownError::NotConnected,
        })
}

pub(super) fn query_tcp_option(
    private: &AnyOpaque,
    query: SocketOptionQuery,
) -> Result<SocketOptionValue, SocketOptionError> {
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    let endpoint = socket.source.endpoint().ok_or(SocketOptionError::Retired)?;
    match query {
        SocketOptionQuery::ReuseAddress => endpoint
            .reuse_address()
            .map(SocketOptionValue::Boolean)
            .map_err(map_bind_option_error),
        SocketOptionQuery::PendingError => endpoint
            .consume_pending_error()
            .map(|error| SocketOptionValue::PendingError(error.map(map_pending_error)))
            .map_err(map_query_option_error),
        SocketOptionQuery::TcpNoDelay => endpoint
            .no_delay()
            .map(SocketOptionValue::Boolean)
            .map_err(map_query_option_error),
        _ => Err(SocketOptionError::Unsupported),
    }
}

pub(super) fn mutate_tcp_option(
    private: &AnyOpaque,
    mutation: SocketOptionMutation,
) -> Result<(), SocketOptionError> {
    let socket = tcp_private(private);
    let _operation = socket.operation.lock();
    let endpoint = socket.source.endpoint().ok_or(SocketOptionError::Retired)?;
    match mutation {
        SocketOptionMutation::ReuseAddress(enabled) => endpoint
            .set_reuse_address(enabled)
            .map_err(map_bind_option_error),
        SocketOptionMutation::TcpNoDelay(enabled) => endpoint
            .set_no_delay(enabled)
            .map_err(map_query_option_error),
        _ => Err(SocketOptionError::Unsupported),
    }
}

fn map_stream_query_send_error(error: TcpQueryError) -> SocketSendError {
    match error {
        TcpQueryError::UnknownEndpoint => SocketSendError::Retired,
        TcpQueryError::WrongRole => SocketSendError::NotConnected,
    }
}

fn map_stream_send_error(error: TcpStreamSendError) -> SocketSendError {
    match error {
        TcpStreamSendError::UnknownEndpoint => SocketSendError::Retired,
        TcpStreamSendError::NotConnected => SocketSendError::NotConnected,
        TcpStreamSendError::WouldBlock => SocketSendError::WouldBlock,
        TcpStreamSendError::BrokenStream => SocketSendError::PeerClosed,
        TcpStreamSendError::ConnectionRefused => SocketSendError::ConnectionRefused,
        TcpStreamSendError::ConnectionReset => SocketSendError::ConnectionReset,
        TcpStreamSendError::TimedOut => SocketSendError::ConnectionTimedOut,
    }
}

fn map_stream_receive_error(error: TcpStreamReceiveError) -> SocketReceiveError {
    match error {
        TcpStreamReceiveError::UnknownEndpoint => SocketReceiveError::Retired,
        TcpStreamReceiveError::NotConnected => SocketReceiveError::NotConnected,
        TcpStreamReceiveError::WouldBlock => SocketReceiveError::WouldBlock,
        TcpStreamReceiveError::ReservationOutstanding => SocketReceiveError::InvalidState,
        TcpStreamReceiveError::ConnectionRefused => SocketReceiveError::ConnectionRefused,
        TcpStreamReceiveError::ConnectionReset => SocketReceiveError::ConnectionReset,
        TcpStreamReceiveError::TimedOut => SocketReceiveError::ConnectionTimedOut,
    }
}

fn map_bind_option_error(error: TcpBindError) -> SocketOptionError {
    match error {
        TcpBindError::UnknownEndpoint => SocketOptionError::Retired,
        TcpBindError::WrongRole
        | TcpBindError::PortInUse
        | TcpBindError::EphemeralPortsExhausted => SocketOptionError::InvalidValue,
    }
}

fn map_query_option_error(error: TcpQueryError) -> SocketOptionError {
    match error {
        TcpQueryError::UnknownEndpoint => SocketOptionError::Retired,
        TcpQueryError::WrongRole => SocketOptionError::InvalidValue,
    }
}

const fn map_pending_error(error: TcpPendingError) -> SocketPendingError {
    match error {
        TcpPendingError::ConnectionRefused => SocketPendingError::ConnectionRefused,
        TcpPendingError::ConnectionReset => SocketPendingError::ConnectionReset,
        TcpPendingError::TimedOut => SocketPendingError::TimedOut,
    }
}

fn map_receive_resolve_error(error: TcpReceiveResolveError) -> SocketReceiveError {
    match error {
        TcpReceiveResolveError::UnknownReservation | TcpReceiveResolveError::InvalidPrefix => {
            SocketReceiveError::InvalidState
        },
    }
}
