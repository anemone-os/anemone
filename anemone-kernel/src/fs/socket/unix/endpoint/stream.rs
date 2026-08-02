//! Paired connection, directional byte-stream, and stream readiness owner.

use crate::{
    fs::{
        iomux::PollRoute,
        socket::{SocketReceiveError, SocketReceiveRequest, SocketSendError, SocketSendRequest},
    },
    kconfig_defs::UNIX_STREAM_DIRECTION_CAPACITY_BYTES,
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::{EndpointAccessError, EndpointAssociation, EndpointName, EndpointState, endpoint};

static_assert!(
    UNIX_STREAM_DIRECTION_CAPACITY_BYTES > 0,
    "unix_stream_direction_capacity_bytes must be non-zero"
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::fs::socket::unix) enum EndpointSide {
    First,
    Second,
}

impl EndpointSide {
    pub(super) const fn index(self) -> usize {
        match self {
            Self::First => 0,
            Self::Second => 1,
        }
    }

    pub(super) const fn peer(self) -> Self {
        match self {
            Self::First => Self::Second,
            Self::Second => Self::First,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct UnixPollRoute {
    route: PollRoute,
    interests: PollEvent,
}

impl UnixPollRoute {
    fn new(route: &PollRoute, interests: PollEvent) -> Self {
        Self {
            route: route.clone(),
            interests,
        }
    }
}

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
    /// Peer-visible full-close facts. Endpoint operation admission remains in
    /// the corresponding `UnixEndpointCore`; these entries only project the
    /// terminal handoff into connection/readiness semantics.
    endpoint_terminal: [bool; 2],
    /// Entry N owns bytes written by endpoint N and read by its peer.
    directions: [StreamDirection; 2],
    /// Each endpoint owns the routes used to recheck its combined predicates.
    pub(super) routes: [Arc<Vec<UnixPollRoute>>; 2],
}

impl ConnectionState {
    fn new() -> Self {
        Self {
            endpoint_terminal: [false, false],
            directions: [StreamDirection::new(), StreamDirection::new()],
            routes: [Arc::new(Vec::new()), Arc::new(Vec::new())],
        }
    }

    fn revents(&self, side: EndpointSide, interests: PollEvent) -> PollEvent {
        let index = side.index();
        let incoming = &self.directions[side.peer().index()];
        let outgoing = &self.directions[index];
        let mut events = PollEvent::empty();
        if interests.contains(PollEvent::READABLE)
            && (!incoming.bytes.is_empty() || !incoming.writer_open)
        {
            events |= PollEvent::READABLE;
        }
        if interests.contains(PollEvent::WRITABLE)
            && outgoing.reader_open
            && outgoing.available() > 0
        {
            events |= PollEvent::WRITABLE;
        }
        // Full peer close is mandatory even when the caller did not request it.
        if self.endpoint_terminal[side.peer().index()] {
            events |= PollEvent::HANG_UP;
        }
        events
    }
}

#[derive(Debug)]
pub(in crate::fs::socket::unix) struct UnixConnection {
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
}

impl UnixConnection {
    pub(in crate::fs::socket::unix) fn new(names: [Arc<EndpointName>; 2]) -> Arc<Self> {
        Arc::new(Self {
            state: SpinLock::new(ConnectionState::new()),
            read_operations: [Mutex::new(()), Mutex::new(())],
            write_operations: [Mutex::new(()), Mutex::new(())],
            names,
        })
    }
}

fn replacement_routes(
    current: &Arc<Vec<UnixPollRoute>>,
    route: &PollRoute,
    interests: PollEvent,
) -> Arc<Vec<UnixPollRoute>> {
    let retained = current
        .iter()
        .filter(|entry| !entry.route.is_prunable())
        .count();
    let mut replacement = Vec::with_capacity(retained + 1);
    replacement.extend(
        current
            .iter()
            .filter(|entry| !entry.route.is_prunable())
            .cloned(),
    );
    replacement.push(UnixPollRoute::new(route, interests));
    Arc::new(replacement)
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
    connection: &Arc<UnixConnection>,
    side: EndpointSide,
) -> Result<(), EndpointAccessError> {
    match &state.association {
        EndpointAssociation::Connected {
            connection: current,
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
) -> Result<usize, SocketReceiveError> {
    let SocketReceiveRequest::Stream(sink) = request else {
        return Err(SocketReceiveError::Unsupported);
    };
    if sink.remaining() == 0 {
        return Ok(0);
    }
    let endpoint = &endpoint(private).core;
    let (connection, side) = endpoint.connected().map_err(|error| match error {
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
            return if incoming.writer_open {
                Err(SocketReceiveError::WouldBlock)
            } else {
                Ok(0)
            };
        }
        sink.remaining().min(incoming.bytes.len())
    };
    assert!(staged_len > 0, "nonempty Unix read selected no bytes");

    let mut staged = Vec::with_capacity(staged_len);
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
    Ok(copied)
}

pub(super) fn send_unix_stream(
    private: &AnyOpaque,
    request: SocketSendRequest<'_>,
) -> Result<usize, SocketSendError> {
    let SocketSendRequest::Stream(source) = request else {
        return Err(SocketSendError::Unsupported);
    };
    if source.remaining() == 0 {
        return Ok(0);
    }
    let endpoint = &endpoint(private).core;
    let (connection, side) = endpoint.connected().map_err(|error| match error {
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
        if !outgoing.reader_open {
            return Err(SocketSendError::PeerClosed);
        }
        if outgoing.available() == 0 {
            return Err(SocketSendError::WouldBlock);
        }
        source.remaining().min(outgoing.available())
    };
    assert!(staged_len > 0, "writable Unix direction selected no bytes");

    let mut staged = Vec::with_capacity(staged_len);
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
        if !outgoing.reader_open {
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

pub(super) fn poll_unix_stream(
    private: &AnyOpaque,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    let endpoint = &endpoint(private).core;
    let (connection, side) = match endpoint.connected() {
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
        let replacement = replacement_routes(&expected, route, request.interests());
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

pub(super) fn retire_connection_endpoint(connection: &Arc<UnixConnection>, side: EndpointSide) {
    let index = side.index();
    let peer_index = side.peer().index();
    let empty_routes = Arc::new(Vec::new());
    let (own_routes, peer_routes) = {
        let mut state = connection.state.lock();
        assert!(
            !state.endpoint_terminal[index],
            "Unix connection observed duplicate endpoint terminal handoff"
        );

        // Endpoint publication is already withdrawn by the caller. Expose the
        // connection-owned terminal facts next, before any route notification.
        // New attempts fail closed; staged attempts recheck before commit.
        state.endpoint_terminal[index] = true;
        state.directions[index].writer_open = false;
        state.directions[peer_index].reader_open = false;
        let own_routes = core::mem::replace(&mut state.routes[index], empty_routes);
        let peer_routes = state.routes[peer_index].clone();
        (own_routes, peer_routes)
    };

    notify_routes(&own_routes, None, "endpoint retirement");
    notify_routes(&peer_routes, None, "peer final close");
}
