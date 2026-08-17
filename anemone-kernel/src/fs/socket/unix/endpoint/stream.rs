//! Paired connection, directional byte-stream, and stream readiness owner.

use crate::{
    fs::socket::{
        SocketReadSink, SocketReceiveError, SocketReceiveFlags, SocketReceiveOutcome,
        SocketReceiveRequest, SocketRightsReceiveOutcome, SocketRightsSendRequest, SocketSendError,
        SocketSendRequest, SocketShutdown, SocketShutdownError, SocketStreamDestination,
        SocketWait, SocketWriteSource,
    },
    kconfig_defs::{
        UNIX_SCM_RIGHTS_DIRECTION_CAPACITY_FDS, UNIX_SCM_RIGHTS_MAX_FDS_PER_MESSAGE,
        UNIX_STREAM_DIRECTION_CAPACITY_BYTES,
    },
    prelude::*,
    task::files::OpenedDescriptionBundle,
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
static_assert!(
    UNIX_SCM_RIGHTS_MAX_FDS_PER_MESSAGE > 0,
    "unix_scm_rights_max_fds_per_message must be non-zero"
);
static_assert!(
    UNIX_SCM_RIGHTS_DIRECTION_CAPACITY_FDS >= UNIX_SCM_RIGHTS_MAX_FDS_PER_MESSAGE,
    "SCM_RIGHTS per-message maximum must fit the direction capacity"
);

#[derive(Debug)]
struct RightsMarker {
    /// Absolute position of the first byte committed with this bundle.
    position: u64,
    rights: OpenedDescriptionBundle,
}

#[derive(Debug)]
struct StreamDirection {
    /// Sole byte-sequence truth for this writer-to-reader direction.
    bytes: VecDeque<u8>,
    /// Absolute position corresponding to `bytes.front()`.
    head_position: u64,
    /// Ordered markers share the byte sequence truth; no marker can exist
    /// without its identified byte remaining in this direction.
    rights: VecDeque<RightsMarker>,
    rights_count: usize,
    writer_open: bool,
    reader_open: bool,
}

impl StreamDirection {
    fn new() -> Self {
        Self {
            bytes: VecDeque::with_capacity(UNIX_STREAM_DIRECTION_CAPACITY_BYTES),
            head_position: 0,
            rights: VecDeque::new(),
            rights_count: 0,
            writer_open: true,
            reader_open: true,
        }
    }

    fn available(&self) -> usize {
        assert!(self.bytes.len() <= UNIX_STREAM_DIRECTION_CAPACITY_BYTES);
        UNIX_STREAM_DIRECTION_CAPACITY_BYTES - self.bytes.len()
    }

    fn tail_position(&self) -> u64 {
        self.head_position
            .checked_add(self.bytes.len() as u64)
            .expect("Unix stream byte position overflow")
    }

    fn available_rights(&self) -> usize {
        assert!(self.rights_count <= UNIX_SCM_RIGHTS_DIRECTION_CAPACITY_FDS);
        UNIX_SCM_RIGHTS_DIRECTION_CAPACITY_FDS - self.rights_count
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

    pub(super) fn readable_bytes(&self, side: EndpointSide) -> usize {
        self.state.lock().directions[side.peer().index()]
            .bytes
            .len()
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

fn receive_unix_stream_transaction(
    private: &AnyOpaque,
    sink: &mut dyn SocketReadSink,
    flags: SocketReceiveFlags,
) -> Result<SocketRightsReceiveOutcome, SocketReceiveError> {
    if sink.remaining() == 0 {
        return Ok(SocketRightsReceiveOutcome::new(0, None));
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
                Ok(SocketRightsReceiveOutcome::new(0, None))
            };
        }
        let mut selected = sink.remaining().min(incoming.bytes.len());
        if let Some(second) = incoming.rights.get(1) {
            let before_second = usize::try_from(
                second
                    .position
                    .checked_sub(incoming.head_position)
                    .expect("Unix rights marker preceded stream head"),
            )
            .expect("Unix rights marker distance exceeded usize");
            selected = selected.min(before_second);
        }
        selected
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
        let state = connection.state.lock();
        let incoming = &state.directions[incoming_index];
        let copied_end = incoming
            .head_position
            .checked_add(copied as u64)
            .expect("Unix peek position overflow");
        let rights = incoming
            .rights
            .front()
            .filter(|marker| marker.position < copied_end)
            .map(|marker| marker.rights.try_duplicate())
            .transpose()
            .map_err(SocketReceiveError::Copy)?;
        return Ok(SocketRightsReceiveOutcome::new(copied, rights));
    }

    let (reader_routes, writer_routes, rights) = {
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
        incoming.head_position = incoming
            .head_position
            .checked_add(copied as u64)
            .expect("Unix stream head position overflow");
        let rights = if incoming
            .rights
            .front()
            .is_some_and(|marker| marker.position < incoming.head_position)
        {
            let marker = incoming
                .rights
                .pop_front()
                .expect("visible Unix rights marker disappeared");
            incoming.rights_count = incoming
                .rights_count
                .checked_sub(marker.rights.len())
                .expect("Unix rights capacity charge underflow");
            Some(marker.rights)
        } else {
            None
        };
        (
            connection_state.routes[side.index()].clone(),
            connection_state.routes[incoming_index].clone(),
            rights,
        )
    };
    notify_routes(&reader_routes, Some(PollEvent::READABLE), "read commit");
    notify_routes(&writer_routes, Some(PollEvent::WRITABLE), "read commit");
    Ok(SocketRightsReceiveOutcome::new(copied, rights))
}

pub(super) fn receive_unix_stream_rights(
    private: &AnyOpaque,
    sink: &mut dyn SocketReadSink,
    flags: SocketReceiveFlags,
) -> Result<SocketRightsReceiveOutcome, SocketReceiveError> {
    receive_unix_stream_transaction(private, sink, flags)
}

pub(super) fn receive_unix_stream(
    private: &AnyOpaque,
    request: SocketReceiveRequest<'_>,
) -> Result<SocketReceiveOutcome, SocketReceiveError> {
    let SocketReceiveRequest::Stream { sink, flags } = request else {
        return Err(SocketReceiveError::Unsupported);
    };
    let mut outcome = receive_unix_stream_transaction(private, sink, flags)?;
    let copied = outcome.copied();
    // Ordinary read/recv has no control consumer. Any marker reached by this
    // byte transaction is silently discarded after all owner guards/gates.
    drop(outcome.take_rights());
    Ok(SocketReceiveOutcome::byte_stream(copied))
}

fn send_unix_stream_transaction(
    private: &AnyOpaque,
    source: &mut dyn SocketWriteSource,
    destination: SocketStreamDestination,
    rights: &mut Option<OpenedDescriptionBundle>,
) -> Result<usize, SocketSendError> {
    let rights_count = rights.as_ref().map_or(0, OpenedDescriptionBundle::len);
    assert!(rights_count <= UNIX_SCM_RIGHTS_MAX_FDS_PER_MESSAGE);
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
        if outgoing.available() == 0 || outgoing.available_rights() < rights_count {
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
        assert!(
            outgoing.available_rights() >= rights_count,
            "Unix rights capacity changed outside its operation gate"
        );
        if let Some(bundle) = rights.take() {
            let position = outgoing.tail_position();
            outgoing.rights_count = outgoing
                .rights_count
                .checked_add(bundle.len())
                .expect("Unix rights capacity charge overflow");
            outgoing.rights.push_back(RightsMarker {
                position,
                rights: bundle,
            });
        }
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

pub(super) fn send_unix_stream_rights(
    private: &AnyOpaque,
    request: SocketRightsSendRequest<'_>,
) -> Result<usize, SocketSendError> {
    send_unix_stream_transaction(private, request.source, request.destination, request.rights)
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
    let mut rights = None;
    send_unix_stream_transaction(private, source, destination, &mut rights)
}

#[derive(Debug, Opaque)]
struct RightsSendWaitSource {
    endpoint: Arc<super::UnixEndpointCore>,
    rights_count: usize,
}

fn rights_send_ready(direction: &StreamDirection, rights_count: usize) -> bool {
    !direction.writer_open
        || !direction.reader_open
        || (direction.available() > 0 && direction.available_rights() >= rights_count)
}

fn poll_rights_send_wait(
    private: &AnyOpaque,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    let source = private
        .cast::<RightsSendWaitSource>()
        .expect("Unix SCM_RIGHTS send wait used without its source");
    let endpoint = &source.endpoint;
    loop {
        let (connection, side) = match &endpoint.state.lock().association {
            EndpointAssociation::Connected {
                connection: super::UnixConnection::Stream(connection),
                side,
            } => (connection.clone(), *side),
            EndpointAssociation::Unconnected
            | EndpointAssociation::Listening(_)
            | EndpointAssociation::Connected { .. }
            | EndpointAssociation::Retired => {
                return Ok(PollRegisterResult::Ready(PollEvent::WRITABLE));
            },
        };
        let expected = connection.state.lock().routes[side.index()].clone();
        let Some(route) = request.route() else {
            let endpoint_state = endpoint.state.lock();
            if validate_connection(&endpoint_state, &connection, side).is_err() {
                return Ok(PollRegisterResult::Ready(PollEvent::WRITABLE));
            }
            let state = connection.state.lock();
            let ready = rights_send_ready(&state.directions[side.index()], source.rights_count);
            return Ok(PollRegisterResult::Ready(
                ready
                    .then_some(PollEvent::WRITABLE)
                    .unwrap_or_else(PollEvent::empty),
            ));
        };
        let replacement = replacement_poll_routes(&expected, route, request.interests());
        let (previous, ready) = {
            let endpoint_state = endpoint.state.lock();
            if validate_connection(&endpoint_state, &connection, side).is_err() {
                return Ok(PollRegisterResult::Ready(PollEvent::WRITABLE));
            }
            let mut state = connection.state.lock();
            if !Arc::ptr_eq(&state.routes[side.index()], &expected) {
                continue;
            }
            let previous = core::mem::replace(&mut state.routes[side.index()], replacement);
            let ready = rights_send_ready(&state.directions[side.index()], source.rights_count);
            (previous, ready)
        };
        drop(previous);
        return Ok(PollRegisterResult::Subscribed(
            ready
                .then_some(PollEvent::WRITABLE)
                .unwrap_or_else(PollEvent::empty),
        ));
    }
}

pub(super) fn prepare_rights_send_wait(private: &AnyOpaque, rights_count: usize) -> SocketWait {
    SocketWait::new(
        AnyOpaque::new(RightsSendWaitSource {
            endpoint: endpoint(private).core.clone(),
            rights_count,
        }),
        poll_rights_send_wait,
    )
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
    let (own_routes, peer_routes, discarded_bytes, discarded_rights) = {
        let mut state = connection.state.lock();
        // Endpoint publication is already withdrawn by the caller. Expose the
        // connection-owned terminal facts next, before any route notification.
        // New attempts fail closed; staged attempts recheck before commit.
        state.directions[index].writer_open = false;
        let incoming = &mut state.directions[peer_index];
        incoming.reader_open = false;
        let discarded_bytes = core::mem::take(&mut incoming.bytes);
        incoming.head_position = incoming
            .head_position
            .checked_add(discarded_bytes.len() as u64)
            .expect("Unix retirement byte position overflow");
        let discarded_rights = core::mem::take(&mut incoming.rights);
        incoming.rights_count = 0;
        let own_routes = core::mem::replace(&mut state.routes[index], empty_routes);
        let peer_routes = state.routes[peer_index].clone();
        (own_routes, peer_routes, discarded_bytes, discarded_rights)
    };

    notify_routes(&own_routes, None, "endpoint retirement");
    notify_routes(&peer_routes, None, "peer final close");
    // Association withdrawal made this inbound payload unreachable. Drop the
    // detached carrier only after the connection guard and route notification.
    drop(discarded_bytes);
    drop(discarded_rights);
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    struct Bytes<'a> {
        bytes: &'a [u8],
        limit: usize,
    }

    impl SocketWriteSource for Bytes<'_> {
        fn remaining(&self) -> usize {
            self.bytes.len()
        }

        fn copy_bytes(&mut self, target: &mut [u8]) -> Result<usize, SysError> {
            let copied = self.bytes.len().min(target.len()).min(self.limit);
            target[..copied].copy_from_slice(&self.bytes[..copied]);
            Ok(copied)
        }
    }

    struct Capture {
        bytes: Vec<u8>,
        capacity: usize,
    }

    impl SocketReadSink for Capture {
        fn remaining(&self) -> usize {
            self.capacity
        }

        fn copy_bytes(&mut self, bytes: &[u8]) -> Result<usize, SysError> {
            let copied = bytes.len().min(self.capacity);
            self.bytes.extend_from_slice(&bytes[..copied]);
            Ok(copied)
        }
    }

    fn pair() -> (AnyOpaque, AnyOpaque) {
        let pair = super::super::prepare_unix_pair().unwrap();
        (pair.first_private, pair.second_private)
    }

    fn send_bytes(private: &AnyOpaque, bytes: &[u8]) -> Result<usize, SocketSendError> {
        let mut source = Bytes {
            bytes,
            limit: usize::MAX,
        };
        send_unix_stream(
            private,
            SocketSendRequest::Stream {
                source: &mut source,
                destination: SocketStreamDestination::Absent,
            },
        )
    }

    fn send_rights(
        private: &AnyOpaque,
        bytes: &[u8],
        rights: &mut Option<OpenedDescriptionBundle>,
    ) -> Result<usize, SocketSendError> {
        let mut source = Bytes {
            bytes,
            limit: usize::MAX,
        };
        send_unix_stream_rights(
            private,
            SocketRightsSendRequest {
                source: &mut source,
                destination: SocketStreamDestination::Absent,
                rights,
            },
        )
    }

    fn receive_rights(
        private: &AnyOpaque,
        capacity: usize,
        peek: bool,
    ) -> Result<(SocketRightsReceiveOutcome, Vec<u8>), SocketReceiveError> {
        let mut sink = Capture {
            bytes: Vec::new(),
            capacity,
        };
        let outcome = receive_unix_stream_rights(private, &mut sink, SocketReceiveFlags { peek })?;
        Ok((outcome, sink.bytes))
    }

    #[kunit]
    fn markers_follow_byte_positions_stop_before_the_second_group_and_peek_duplicates() {
        let (first, second) = pair();
        assert_eq!(send_bytes(&first, b"a"), Ok(1));

        let (first_bundle, first_identity) = OpenedDescriptionBundle::for_kunit(2);
        let mut first_rights = Some(first_bundle);
        assert_eq!(send_rights(&first, b"bc", &mut first_rights), Ok(2));
        assert!(first_rights.is_none());

        let (second_bundle, second_identity) = OpenedDescriptionBundle::for_kunit(1);
        let mut second_rights = Some(second_bundle);
        assert_eq!(send_rights(&first, b"def", &mut second_rights), Ok(3));
        assert!(second_rights.is_none());

        let (mut before, bytes) = receive_rights(&second, 1, false).unwrap();
        assert_eq!(bytes, b"a");
        assert!(before.take_rights().is_none());

        for _ in 0..2 {
            let (mut peeked, bytes) = receive_rights(&second, usize::MAX, true).unwrap();
            assert_eq!(bytes, b"bc");
            let duplicate = peeked.take_rights().unwrap();
            assert_eq!(duplicate.len(), 2);
            drop(duplicate);
            assert!(first_identity.try_lease().is_some());
        }

        let (mut consumed, bytes) = receive_rights(&second, usize::MAX, false).unwrap();
        assert_eq!(bytes, b"bc");
        assert_eq!(consumed.take_rights().unwrap().len(), 2);
        drop(consumed);
        assert!(first_identity.try_lease().is_none());
        assert!(second_identity.try_lease().is_some());

        let mut ordinary = Capture {
            bytes: Vec::new(),
            capacity: usize::MAX,
        };
        assert_eq!(
            receive_unix_stream(
                &second,
                SocketReceiveRequest::Stream {
                    sink: &mut ordinary,
                    flags: SocketReceiveFlags { peek: false },
                },
            )
            .unwrap()
            .copied(),
            3
        );
        assert_eq!(ordinary.bytes, b"def");
        assert!(second_identity.try_lease().is_none());

        super::super::final_release_unix_endpoint(&first);
        super::super::final_release_unix_endpoint(&second);
    }

    #[kunit]
    fn zero_and_partial_send_transfer_rights_only_with_positive_commit() {
        let (first, second) = pair();
        let (zero_bundle, zero_identity) = OpenedDescriptionBundle::for_kunit(1);
        let mut zero_rights = Some(zero_bundle);
        assert_eq!(send_rights(&first, b"", &mut zero_rights), Ok(0));
        assert!(zero_rights.is_some());
        drop(zero_rights);
        assert!(zero_identity.try_lease().is_none());

        let (bundle, identity) = OpenedDescriptionBundle::for_kunit(1);
        let mut rights = Some(bundle);
        let mut source = Bytes {
            bytes: b"partial",
            limit: 3,
        };
        assert_eq!(
            send_unix_stream_rights(
                &first,
                SocketRightsSendRequest {
                    source: &mut source,
                    destination: SocketStreamDestination::Absent,
                    rights: &mut rights,
                },
            ),
            Ok(3)
        );
        assert!(rights.is_none());
        let (mut outcome, bytes) = receive_rights(&second, usize::MAX, false).unwrap();
        assert_eq!(bytes, b"par");
        drop(outcome.take_rights());
        assert!(identity.try_lease().is_none());

        super::super::final_release_unix_endpoint(&first);
        super::super::final_release_unix_endpoint(&second);
    }

    #[kunit]
    fn rights_capacity_has_a_request_specific_wait_without_changing_public_writable() {
        let (first, second) = pair();
        for count in [253, 253, 253, 253, 12] {
            let (bundle, _) = OpenedDescriptionBundle::for_kunit(count);
            let mut rights = Some(bundle);
            assert_eq!(send_rights(&first, b"x", &mut rights), Ok(1));
        }

        let (blocked_bundle, blocked_identity) = OpenedDescriptionBundle::for_kunit(1);
        let mut blocked = Some(blocked_bundle);
        assert_eq!(
            send_rights(&first, b"y", &mut blocked),
            Err(SocketSendError::WouldBlock)
        );
        assert!(blocked.is_some());
        let public =
            poll_connected_unix_stream(&first, &PollRequest::snapshot(PollEvent::WRITABLE))
                .unwrap()
                .expect_ready("public Unix stream writability");
        assert!(public.contains(PollEvent::WRITABLE));
        let wait = prepare_rights_send_wait(&first, 1);
        assert_eq!(
            wait.poll(&PollRequest::snapshot(PollEvent::WRITABLE))
                .unwrap(),
            PollRegisterResult::Ready(PollEvent::empty())
        );

        let (mut released, bytes) = receive_rights(&second, 1, false).unwrap();
        assert_eq!(bytes, b"x");
        drop(released.take_rights());
        assert_eq!(
            wait.poll(&PollRequest::snapshot(PollEvent::WRITABLE))
                .unwrap(),
            PollRegisterResult::Ready(PollEvent::WRITABLE)
        );
        assert_eq!(send_rights(&first, b"y", &mut blocked), Ok(1));
        assert!(blocked.is_none());

        super::super::final_release_unix_endpoint(&second);
        assert!(blocked_identity.try_lease().is_none());
        super::super::final_release_unix_endpoint(&first);
    }

    #[kunit]
    fn shutdown_preserves_queued_rights_but_retirement_discards_only_unreachable_inbound() {
        let (first, second) = pair();
        let (queued_bundle, queued_identity) = OpenedDescriptionBundle::for_kunit(1);
        let mut queued = Some(queued_bundle);
        assert_eq!(send_rights(&first, b"q", &mut queued), Ok(1));
        shutdown_unix_stream(&second, SocketShutdown::Read).unwrap();
        assert_eq!(send_bytes(&first, b"x"), Err(SocketSendError::PeerClosed));
        let (mut received, bytes) = receive_rights(&second, 1, false).unwrap();
        assert_eq!(bytes, b"q");
        drop(received.take_rights());
        assert!(queued_identity.try_lease().is_none());

        let (outbound_bundle, outbound_identity) = OpenedDescriptionBundle::for_kunit(1);
        let mut outbound = Some(outbound_bundle);
        assert_eq!(send_rights(&second, b"p", &mut outbound), Ok(1));
        super::super::final_release_unix_endpoint(&second);
        assert!(outbound_identity.try_lease().is_some());
        let (mut preserved, bytes) = receive_rights(&first, 1, false).unwrap();
        assert_eq!(bytes, b"p");
        drop(preserved.take_rights());
        assert!(outbound_identity.try_lease().is_none());
        super::super::final_release_unix_endpoint(&first);

        let (first, second) = pair();
        let (discarded_bundle, discarded_identity) = OpenedDescriptionBundle::for_kunit(1);
        let mut discarded = Some(discarded_bundle);
        assert_eq!(send_rights(&first, b"d", &mut discarded), Ok(1));
        super::super::final_release_unix_endpoint(&second);
        assert!(discarded_identity.try_lease().is_none());
        super::super::final_release_unix_endpoint(&first);
    }
}
