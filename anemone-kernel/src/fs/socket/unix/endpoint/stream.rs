//! Paired connection, directional byte-stream, and stream readiness owner.

use crate::{
    fs::socket::{
        SocketReceiveError, SocketReceiveOutcome, SocketReceiveRequest, SocketSendError,
        SocketSendRequest, SocketShutdown, SocketShutdownError, SocketStreamDestination,
    },
    kconfig_defs::UNIX_STREAM_DIRECTION_CAPACITY_BYTES,
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::{
    EndpointAccessError, EndpointAssociation, EndpointName, EndpointSide, EndpointState,
    UnixPeerCredentials, UnixPollRoute, endpoint, replacement_poll_routes,
};

static_assert!(
    UNIX_STREAM_DIRECTION_CAPACITY_BYTES > 0,
    "unix_stream_direction_capacity_bytes must be non-zero"
);

#[derive(Debug)]
struct StreamDirection {
    /// Sole byte-sequence truth for this writer-to-reader direction.
    bytes: VecDeque<u8>,
    writer_open: bool,
    reader_open: bool,
}

impl StreamDirection {
    fn new() -> Self {
        Self {
            bytes: VecDeque::with_capacity(UNIX_STREAM_DIRECTION_CAPACITY_BYTES),
            writer_open: true,
            reader_open: true,
        }
    }

    fn available(&self) -> usize {
        assert!(self.bytes.len() <= UNIX_STREAM_DIRECTION_CAPACITY_BYTES);
        UNIX_STREAM_DIRECTION_CAPACITY_BYTES - self.bytes.len()
    }
}

#[derive(Debug)]
pub(super) struct ConnectionState {
    /// Entry N owns bytes written by endpoint N and read by its peer.
    directions: [StreamDirection; 2],
    /// Each endpoint owns the routes used to recheck its combined predicates.
    pub(super) routes: [Arc<Vec<UnixPollRoute>>; 2],
}

impl ConnectionState {
    fn new() -> Self {
        Self {
            directions: [StreamDirection::new(), StreamDirection::new()],
            routes: [Arc::new(Vec::new()), Arc::new(Vec::new())],
        }
    }

    fn revents(&self, side: EndpointSide, interests: PollEvent) -> PollEvent {
        let index = side.index();
        let incoming = &self.directions[side.peer().index()];
        let outgoing = &self.directions[index];
        let receive_terminal = !incoming.writer_open || !incoming.reader_open;
        let send_terminal = !outgoing.writer_open || !outgoing.reader_open;
        let mut events = PollEvent::empty();
        if interests.contains(PollEvent::READABLE)
            && (!incoming.bytes.is_empty() || receive_terminal)
        {
            events |= PollEvent::READABLE;
        }
        if interests.contains(PollEvent::WRITABLE) && (send_terminal || outgoing.available() > 0) {
            events |= PollEvent::WRITABLE;
        }
        if interests.contains(PollEvent::READ_HANG_UP) && receive_terminal {
            events |= PollEvent::READ_HANG_UP;
        }
        // Full HUP is a projection of the two direction terminal facts, not a
        // separately writable endpoint or readiness bit.
        if receive_terminal && send_terminal {
            events |= PollEvent::HANG_UP;
        }
        events
    }
}

#[derive(Debug)]
pub(in crate::fs::socket::unix) struct UnixStreamConnection {
    /// One lock linearizes endpoint retirement, both directional facts, and
    /// route publication. The two direction entries remain the unique owners
    /// of their byte and terminal facts; this lock is not another truth source.
    pub(super) state: SpinLock<ConnectionState>,
    /// A read gate keeps one selected prefix stable across user copyout.
    read_operations: [Mutex<()>; 2],
    /// A write gate keeps capacity stable while one user prefix is staged.
    write_operations: [Mutex<()>; 2],
    /// Read-only peer views of the endpoint-owned local-name facts. Keeping
    /// these narrow capabilities alive preserves Linux peer-name observation
    /// after the peer's final close without extending binding admission.
    pub(super) names: [Arc<EndpointName>; 2],
    /// Immutable identity snapshots captured before connection publication.
    /// They intentionally survive peer exit and never drive connection state.
    pub(super) credentials: [UnixPeerCredentials; 2],
}

impl UnixStreamConnection {
    pub(in crate::fs::socket::unix) fn new(
        names: [Arc<EndpointName>; 2],
        credentials: [UnixPeerCredentials; 2],
    ) -> Arc<Self> {
        Arc::new(Self {
            state: SpinLock::new(ConnectionState::new()),
            read_operations: [Mutex::new(()), Mutex::new(())],
            write_operations: [Mutex::new(()), Mutex::new(())],
            names,
            credentials,
        })
    }

    pub(super) fn install_routes(&self, side: EndpointSide, routes: Arc<Vec<UnixPollRoute>>) {
        let mut state = self.state.lock();
        let slot = &mut state.routes[side.index()];
        assert!(slot.is_empty(), "Unix connection routes installed twice");
        *slot = routes;
    }
}

fn notify_routes(
    routes: &Arc<Vec<UnixPollRoute>>,
    changed: Option<PollEvent>,
    reason: &'static str,
) {
    let mut candidates = 0usize;
    for entry in routes.iter() {
        if changed.is_none_or(|changed| entry.interests.intersects(changed)) {
            entry.route.notify();
            candidates += 1;
        }
    }
    if candidates > 0 {
        kdebugln!(
            "unix socket: issued {} route hints reason={}",
            candidates,
            reason,
        );
    }
}

fn validate_connection(
    state: &EndpointState,
    connection: &Arc<UnixStreamConnection>,
    side: EndpointSide,
) -> Result<(), EndpointAccessError> {
    match &state.association {
        EndpointAssociation::Connected {
            connection: super::UnixConnection::Stream(current),
            side: current_side,
        } if Arc::ptr_eq(current, connection) && *current_side == side => Ok(()),
        EndpointAssociation::Unconnected => Err(EndpointAccessError::Unconnected),
        EndpointAssociation::Listening(_) => Err(EndpointAccessError::InvalidState),
        EndpointAssociation::Retired => Err(EndpointAccessError::Retired),
        EndpointAssociation::Connected { .. } => {
            panic!("Unix endpoint connection association changed after publication")
        },
    }
}

pub(super) fn receive_unix_stream(
    private: &AnyOpaque,
    request: SocketReceiveRequest<'_>,
) -> Result<SocketReceiveOutcome, SocketReceiveError> {
    let SocketReceiveRequest::Stream { sink, flags } = request else {
        return Err(SocketReceiveError::Unsupported);
    };
    if sink.remaining() == 0 {
        return Ok(SocketReceiveOutcome::byte_stream(0));
    }
    let endpoint = &endpoint(private).core;
    let (connection, side) = endpoint.connected_stream().map_err(|error| match error {
        EndpointAccessError::Unconnected | EndpointAccessError::InvalidState => {
            SocketReceiveError::InvalidState
        },
        EndpointAccessError::Retired => SocketReceiveError::Retired,
    })?;
    let incoming_index = side.peer().index();
    let _operation = connection.read_operations[side.index()].lock();

    let staged_len = {
        let endpoint_state = endpoint.state.lock();
        validate_connection(&endpoint_state, &connection, side).map_err(|error| match error {
            EndpointAccessError::Unconnected | EndpointAccessError::InvalidState => {
                SocketReceiveError::InvalidState
            },
            EndpointAccessError::Retired => SocketReceiveError::Retired,
        })?;
        let connection_state = connection.state.lock();
        let incoming = &connection_state.directions[incoming_index];
        if incoming.bytes.is_empty() {
            return if incoming.writer_open && incoming.reader_open {
                Err(SocketReceiveError::WouldBlock)
            } else {
                Ok(SocketReceiveOutcome::byte_stream(0))
            };
        }
        sink.remaining().min(incoming.bytes.len())
    };
    assert!(staged_len > 0, "nonempty Unix read selected no bytes");

    let mut staged = Vec::new();
    staged
        .try_reserve_exact(staged_len)
        .map_err(|_| SocketReceiveError::Copy(SysError::OutOfMemory))?;
    {
        let state = connection.state.lock();
        let incoming = &state.directions[incoming_index];
        assert!(
            incoming.bytes.len() >= staged_len,
            "Unix read prefix changed outside its operation gate"
        );
        staged.extend(incoming.bytes.iter().take(staged_len));
    }

    let copied = sink.copy_bytes(&staged).map_err(SocketReceiveError::Copy)?;
    assert!(
        copied > 0 && copied <= staged.len(),
        "nonempty Unix read copy made invalid progress"
    );

    if flags.peek {
        let endpoint_state = endpoint.state.lock();
        validate_connection(&endpoint_state, &connection, side).map_err(|error| match error {
            EndpointAccessError::Unconnected | EndpointAccessError::InvalidState => {
                SocketReceiveError::InvalidState
            },
            EndpointAccessError::Retired => SocketReceiveError::Retired,
        })?;
        return Ok(SocketReceiveOutcome::byte_stream(copied));
    }

    let (reader_routes, writer_routes) = {
        let endpoint_state = endpoint.state.lock();
        validate_connection(&endpoint_state, &connection, side).map_err(|error| match error {
            EndpointAccessError::Unconnected | EndpointAccessError::InvalidState => {
                SocketReceiveError::InvalidState
            },
            EndpointAccessError::Retired => SocketReceiveError::Retired,
        })?;
        let mut connection_state = connection.state.lock();
        let incoming = &mut connection_state.directions[incoming_index];
        for expected in &staged[..copied] {
            let actual = incoming
                .bytes
                .pop_front()
                .expect("Unix staged prefix disappeared before read commit");
            assert_eq!(
                actual, *expected,
                "Unix staged prefix changed before commit"
            );
        }
        (
            connection_state.routes[side.index()].clone(),
            connection_state.routes[incoming_index].clone(),
        )
    };
    notify_routes(&reader_routes, Some(PollEvent::READABLE), "read commit");
    notify_routes(&writer_routes, Some(PollEvent::WRITABLE), "read commit");
    Ok(SocketReceiveOutcome::byte_stream(copied))
}

pub(super) fn send_unix_stream(
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
    let endpoint = &endpoint(private).core;
    if destination == SocketStreamDestination::Present {
        return match endpoint.connected() {
            Ok(_) => Err(SocketSendError::AlreadyConnected),
            Err(EndpointAccessError::Unconnected) => Err(SocketSendError::NotConnected),
            Err(EndpointAccessError::InvalidState) => Err(SocketSendError::InvalidState),
            Err(EndpointAccessError::Retired) => Err(SocketSendError::Retired),
        };
    }
    let (connection, side) = endpoint.connected_stream().map_err(|error| match error {
        EndpointAccessError::Unconnected => SocketSendError::NotConnected,
        EndpointAccessError::InvalidState => SocketSendError::InvalidState,
        EndpointAccessError::Retired => SocketSendError::Retired,
    })?;
    let index = side.index();
    let peer_index = side.peer().index();
    let _operation = connection.write_operations[index].lock();

    let staged_len = {
        let endpoint_state = endpoint.state.lock();
        validate_connection(&endpoint_state, &connection, side).map_err(|error| match error {
            EndpointAccessError::Unconnected => SocketSendError::NotConnected,
            EndpointAccessError::InvalidState => SocketSendError::InvalidState,
            EndpointAccessError::Retired => SocketSendError::Retired,
        })?;
        let connection_state = connection.state.lock();
        let outgoing = &connection_state.directions[index];
        if !outgoing.writer_open || !outgoing.reader_open {
            return Err(SocketSendError::PeerClosed);
        }
        if source.remaining() == 0 {
            return Ok(0);
        }
        if outgoing.available() == 0 {
            return Err(SocketSendError::WouldBlock);
        }
        source.remaining().min(outgoing.available())
    };
    assert!(staged_len > 0, "writable Unix direction selected no bytes");

    let mut staged = Vec::new();
    staged
        .try_reserve_exact(staged_len)
        .map_err(|_| SocketSendError::Copy(SysError::OutOfMemory))?;
    staged.resize(staged_len, 0);
    let copied = source
        .copy_bytes(&mut staged)
        .map_err(SocketSendError::Copy)?;
    assert!(
        copied > 0 && copied <= staged.len(),
        "nonempty Unix write copy made invalid progress"
    );

    let (writer_routes, peer_routes) = {
        let endpoint_state = endpoint.state.lock();
        validate_connection(&endpoint_state, &connection, side).map_err(|error| match error {
            EndpointAccessError::Unconnected => SocketSendError::NotConnected,
            EndpointAccessError::InvalidState => SocketSendError::InvalidState,
            EndpointAccessError::Retired => SocketSendError::Retired,
        })?;
        let mut connection_state = connection.state.lock();
        let outgoing = &mut connection_state.directions[index];
        if !outgoing.writer_open || !outgoing.reader_open {
            return Err(SocketSendError::PeerClosed);
        }
        assert!(
            outgoing.available() >= copied,
            "Unix write capacity changed outside its operation gate"
        );
        outgoing.bytes.extend(&staged[..copied]);
        (
            connection_state.routes[index].clone(),
            connection_state.routes[peer_index].clone(),
        )
    };
    notify_routes(&writer_routes, Some(PollEvent::WRITABLE), "write commit");
    notify_routes(&peer_routes, Some(PollEvent::READABLE), "write commit");
    Ok(copied)
}

pub(super) fn shutdown_unix_stream(
    private: &AnyOpaque,
    how: SocketShutdown,
) -> Result<(), SocketShutdownError> {
    let endpoint = &endpoint(private).core;
    let (connection, side) = endpoint.connected_stream().map_err(|error| match error {
        EndpointAccessError::Unconnected | EndpointAccessError::InvalidState => {
            SocketShutdownError::NotConnected
        },
        EndpointAccessError::Retired => SocketShutdownError::Retired,
    })?;
    let index = side.index();
    let incoming_index = side.peer().index();

    // The operation gates are the stable copy/commit boundary. Shutdown waits
    // outside every spinlock, then changes only direction-owned facts.
    let _read_operation = matches!(how, SocketShutdown::Read | SocketShutdown::ReadWrite)
        .then(|| connection.read_operations[index].lock());
    let _write_operation = matches!(how, SocketShutdown::Write | SocketShutdown::ReadWrite)
        .then(|| connection.write_operations[index].lock());

    let (changed, own_routes, peer_routes) = {
        let endpoint_state = endpoint.state.lock();
        validate_connection(&endpoint_state, &connection, side).map_err(|error| match error {
            EndpointAccessError::Unconnected | EndpointAccessError::InvalidState => {
                SocketShutdownError::NotConnected
            },
            EndpointAccessError::Retired => SocketShutdownError::Retired,
        })?;
        let mut state = connection.state.lock();
        let mut changed = false;
        if matches!(how, SocketShutdown::Read | SocketShutdown::ReadWrite) {
            changed |= state.directions[incoming_index].reader_open;
            state.directions[incoming_index].reader_open = false;
        }
        if matches!(how, SocketShutdown::Write | SocketShutdown::ReadWrite) {
            changed |= state.directions[index].writer_open;
            state.directions[index].writer_open = false;
        }
        (
            changed,
            state.routes[index].clone(),
            state.routes[incoming_index].clone(),
        )
    };

    if changed {
        notify_routes(&own_routes, None, "local shutdown");
        notify_routes(&peer_routes, None, "peer shutdown");
    }
    Ok(())
}

pub(super) fn poll_connected_unix_stream(
    private: &AnyOpaque,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    let endpoint = &endpoint(private).core;
    let (connection, side) = match endpoint.connected_stream() {
        Ok(association) => association,
        Err(EndpointAccessError::Unconnected | EndpointAccessError::InvalidState) => {
            return Ok(PollRegisterResult::Unsupported);
        },
        Err(EndpointAccessError::Retired) => {
            return Ok(PollRegisterResult::Ready(PollEvent::HANG_UP));
        },
    };
    let Some(route) = request.route() else {
        let endpoint_state = endpoint.state.lock();
        if validate_connection(&endpoint_state, &connection, side).is_err() {
            return Ok(PollRegisterResult::Ready(PollEvent::HANG_UP));
        }
        let state = connection.state.lock();
        return Ok(PollRegisterResult::Ready(
            state.revents(side, request.interests()),
        ));
    };

    loop {
        let expected = {
            let endpoint_state = endpoint.state.lock();
            if validate_connection(&endpoint_state, &connection, side).is_err() {
                return Ok(PollRegisterResult::Ready(PollEvent::HANG_UP));
            }
            connection.state.lock().routes[side.index()].clone()
        };
        let replacement = replacement_poll_routes(&expected, route, request.interests());
        let (previous, events) = {
            let endpoint_state = endpoint.state.lock();
            if validate_connection(&endpoint_state, &connection, side).is_err() {
                return Ok(PollRegisterResult::Ready(PollEvent::HANG_UP));
            }
            let mut state = connection.state.lock();
            if !Arc::ptr_eq(&state.routes[side.index()], &expected) {
                continue;
            }
            let previous = core::mem::replace(&mut state.routes[side.index()], replacement);
            let events = state.revents(side, request.interests());
            (previous, events)
        };
        drop(previous);
        return Ok(PollRegisterResult::Subscribed(events));
    }
}

pub(super) fn retire_connection_endpoint(
    connection: &Arc<UnixStreamConnection>,
    side: EndpointSide,
) {
    let index = side.index();
    let peer_index = side.peer().index();
    let empty_routes = Arc::new(Vec::new());
    let (own_routes, peer_routes) = {
        let mut state = connection.state.lock();
        // Endpoint publication is already withdrawn by the caller. Expose the
        // connection-owned terminal facts next, before any route notification.
        // New attempts fail closed; staged attempts recheck before commit.
        state.directions[index].writer_open = false;
        state.directions[peer_index].reader_open = false;
        let own_routes = core::mem::replace(&mut state.routes[index], empty_routes);
        let peer_routes = state.routes[peer_index].clone();
        (own_routes, peer_routes)
    };

    notify_routes(&own_routes, None, "endpoint retirement");
    notify_routes(&peer_routes, None, "peer final close");
}
