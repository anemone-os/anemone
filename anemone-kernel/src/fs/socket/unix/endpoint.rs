//! Unix stream endpoint, paired connection, and directional byte owners.

use crate::{
    fs::iomux::PollRoute,
    prelude::*,
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use super::{
    super::{
        SocketAcceptError, SocketAcceptItem, SocketAddress, SocketAddressSink, SocketBindError,
        SocketConnectError, SocketCreation, SocketListenError, SocketOps, SocketPairPreparation,
        SocketPreparation, SocketQueryError, SocketReceiveError, SocketReceiveRequest,
        SocketSendError, SocketSendRequest, SocketStreamReadSink, SocketStreamWriteSource,
        SocketType,
    },
    admission::{
        UnixListener, accept, connect, listen, notify_admission_routes, with_admission_commit,
    },
    namespace::{BindingRegistration, create_socket_pathname, publish_binding, withdraw_binding},
};

static_assert!(
    UNIX_STREAM_DIRECTION_CAPACITY_BYTES > 0,
    "unix_stream_direction_capacity_bytes must be non-zero"
);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum EndpointSide {
    First,
    Second,
}

impl EndpointSide {
    const fn index(self) -> usize {
        match self {
            Self::First => 0,
            Self::Second => 1,
        }
    }

    const fn peer(self) -> Self {
        match self {
            Self::First => Self::Second,
            Self::Second => Self::First,
        }
    }
}

#[derive(Clone, Debug)]
struct UnixPollRoute {
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
struct ConnectionState {
    /// Peer-visible full-close facts. Endpoint operation admission remains in
    /// the corresponding `UnixEndpointCore`; these entries only project the
    /// terminal handoff into connection/readiness semantics.
    endpoint_terminal: [bool; 2],
    /// Entry N owns bytes written by endpoint N and read by its peer.
    directions: [StreamDirection; 2],
    /// Each endpoint owns the routes used to recheck its combined predicates.
    routes: [Arc<Vec<UnixPollRoute>>; 2],
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
pub(super) struct UnixConnection {
    /// One lock linearizes endpoint retirement, both directional facts, and
    /// route publication. The two direction entries remain the unique owners
    /// of their byte and terminal facts; this lock is not another truth source.
    state: SpinLock<ConnectionState>,
    /// A read gate keeps one selected prefix stable across user copyout.
    read_operations: [Mutex<()>; 2],
    /// A write gate keeps capacity stable while one user prefix is staged.
    write_operations: [Mutex<()>; 2],
    /// Read-only peer views of the endpoint-owned local-name facts. Keeping
    /// these narrow capabilities alive preserves Linux peer-name observation
    /// after the peer's final close without extending binding admission.
    names: [Arc<EndpointName>; 2],
}

impl UnixConnection {
    pub(super) fn new(names: [Arc<EndpointName>; 2]) -> Arc<Self> {
        Arc::new(Self {
            state: SpinLock::new(ConnectionState::new()),
            read_operations: [Mutex::new(()), Mutex::new(())],
            write_operations: [Mutex::new(()), Mutex::new(())],
            names,
        })
    }
}

#[derive(Debug)]
pub(super) struct EndpointName(SpinLock<Option<Arc<str>>>);

impl EndpointName {
    fn new() -> Arc<Self> {
        Arc::new(Self(SpinLock::new(None)))
    }

    pub(super) fn snapshot(&self) -> Option<Arc<str>> {
        self.0.lock().clone()
    }

    fn publish(&self, pathname: Arc<str>) {
        let mut name = self.0.lock();
        assert!(
            name.is_none(),
            "Unix endpoint local name was published twice"
        );
        *name = Some(pathname);
    }
}

#[derive(Debug)]
pub(super) enum EndpointAssociation {
    Unconnected,
    Listening(Arc<UnixListener>),
    Connected {
        connection: Arc<UnixConnection>,
        side: EndpointSide,
    },
    Retired,
}

#[derive(Debug)]
pub(super) enum BindingState {
    Unnamed,
    Preparing(Arc<str>),
    Bound(BindingRegistration),
}

#[derive(Debug)]
pub(super) struct EndpointState {
    pub(super) association: EndpointAssociation,
    pub(super) binding: BindingState,
    /// Routes interested only in endpoint admission/lifecycle changes. They
    /// carry no role, readiness, or errno truth and are pruned
    /// opportunistically.
    pub(super) lifecycle_routes: Arc<Vec<PollRoute>>,
}

#[derive(Debug)]
pub(super) struct UnixEndpointCore {
    /// Sole owner of endpoint role/association and bind publication phase.
    /// Connection/directional locks are always acquired after this lock when
    /// a commit must validate both owners.
    pub(super) state: SpinLock<EndpointState>,
    pub(super) name: Arc<EndpointName>,
}

impl UnixEndpointCore {
    pub(super) fn new_unconnected() -> Arc<Self> {
        Arc::new(Self {
            state: SpinLock::new(EndpointState {
                association: EndpointAssociation::Unconnected,
                binding: BindingState::Unnamed,
                lifecycle_routes: Arc::new(Vec::new()),
            }),
            name: EndpointName::new(),
        })
    }

    pub(super) fn new_with_name(name: Arc<EndpointName>) -> Arc<Self> {
        Arc::new(Self {
            state: SpinLock::new(EndpointState {
                association: EndpointAssociation::Unconnected,
                binding: BindingState::Unnamed,
                lifecycle_routes: Arc::new(Vec::new()),
            }),
            name,
        })
    }

    pub(super) fn peer_address_snapshot(&self) -> Option<SocketAddress> {
        let (connection, side) = self
            .connected()
            .expect("queued Unix child lost its connected association");
        connection.names[side.peer().index()]
            .snapshot()
            .map(SocketAddress::UnixPathname)
    }

    pub(super) fn install_connection(&self, connection: Arc<UnixConnection>, side: EndpointSide) {
        let mut state = self.state.lock();
        assert!(matches!(
            state.association,
            EndpointAssociation::Unconnected
        ));
        assert!(
            state.lifecycle_routes.is_empty(),
            "unpublished Unix endpoint unexpectedly carried lifecycle routes"
        );
        state.association = EndpointAssociation::Connected { connection, side };
    }

    pub(super) fn commit_connection(
        &self,
        connection: Arc<UnixConnection>,
        side: EndpointSide,
        empty_lifecycle_routes: Arc<Vec<PollRoute>>,
    ) -> Arc<Vec<PollRoute>> {
        let mut state = self.state.lock();
        assert!(matches!(
            state.association,
            EndpointAssociation::Unconnected
        ));
        state.association = EndpointAssociation::Connected { connection, side };
        core::mem::replace(&mut state.lifecycle_routes, empty_lifecycle_routes)
    }

    fn connected(&self) -> Result<(Arc<UnixConnection>, EndpointSide), EndpointAccessError> {
        match &self.state.lock().association {
            EndpointAssociation::Unconnected => Err(EndpointAccessError::Unconnected),
            EndpointAssociation::Listening(_) => Err(EndpointAccessError::InvalidState),
            EndpointAssociation::Connected { connection, side } => Ok((connection.clone(), *side)),
            EndpointAssociation::Retired => Err(EndpointAccessError::Retired),
        }
    }

    fn begin_bind(&self, pathname: Arc<str>) -> Result<(), SocketBindError> {
        let mut state = self.state.lock();
        if matches!(state.association, EndpointAssociation::Retired) {
            return Err(SocketBindError::Retired);
        }
        if !matches!(state.binding, BindingState::Unnamed) {
            return Err(SocketBindError::AlreadyBound);
        }
        state.binding = BindingState::Preparing(pathname);
        Ok(())
    }

    fn abort_bind(&self, pathname: &Arc<str>) {
        let mut state = self.state.lock();
        if matches!(
            &state.binding,
            BindingState::Preparing(current) if Arc::ptr_eq(current, pathname)
        ) {
            state.binding = BindingState::Unnamed;
        }
    }

    fn commit_bind(
        self: &Arc<Self>,
        pathname: Arc<str>,
        inode: InodeRef,
    ) -> Result<(), SocketBindError> {
        let mut state = self.state.lock();
        if matches!(state.association, EndpointAssociation::Retired) {
            return Err(SocketBindError::Retired);
        }
        assert!(
            matches!(
                &state.binding,
                BindingState::Preparing(current) if Arc::ptr_eq(current, &pathname)
            ),
            "Unix bind commit lost its endpoint-local preparation"
        );

        // Endpoint -> registry -> name is the sole publication order. Registry
        // lookup drops its lock before using the returned capability, so no
        // reverse acquisition exists. Heap allocation here is kernel-fatal,
        // not a returnable post-VFS failure.
        let registration = publish_binding(inode, self);
        self.name.publish(pathname);
        state.binding = BindingState::Bound(registration);
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EndpointAccessError {
    Unconnected,
    InvalidState,
    Retired,
}

#[derive(Debug, Opaque)]
struct UnixEndpoint {
    core: Arc<UnixEndpointCore>,
}

pub(super) fn private_from_core(core: Arc<UnixEndpointCore>) -> AnyOpaque {
    AnyOpaque::new(UnixEndpoint { core })
}

fn endpoint(private: &AnyOpaque) -> &UnixEndpoint {
    private
        .cast::<UnixEndpoint>()
        .expect("Unix SocketOps used without Unix endpoint private state")
}

fn prepare_unix_pair() -> Result<SocketPairPreparation, SysError> {
    let first = UnixEndpointCore::new_unconnected();
    let second = UnixEndpointCore::new_unconnected();
    let connection = UnixConnection::new([first.name.clone(), second.name.clone()]);
    first.install_connection(connection.clone(), EndpointSide::First);
    second.install_connection(connection, EndpointSide::Second);
    Ok(SocketPairPreparation {
        first_private: private_from_core(first),
        second_private: private_from_core(second),
    })
}

fn commit_unix_socket(_creation: &mut AnyOpaque) {}

fn prepare_unix_socket() -> Result<SocketPreparation, SysError> {
    Ok(SocketPreparation {
        private: private_from_core(UnixEndpointCore::new_unconnected()),
        creation: SocketCreation {
            commit: commit_unix_socket,
            authority: NilOpaque::new(),
        },
    })
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

fn bind_unix_stream(private: &AnyOpaque, address: SocketAddress) -> Result<(), SocketBindError> {
    let SocketAddress::UnixPathname(pathname) = address else {
        return Err(SocketBindError::Unsupported);
    };
    let endpoint = &endpoint(private).core;
    endpoint.begin_bind(pathname.clone())?;
    let inode = match create_socket_pathname(&pathname) {
        Ok(inode) => inode,
        Err(error) => {
            endpoint.abort_bind(&pathname);
            return Err(match error {
                SysError::AddressInUse | SysError::AlreadyExists => SocketBindError::AddressInUse,
                error => SocketBindError::Operation(error),
            });
        },
    };
    if let Err(error) = endpoint.commit_bind(pathname.clone(), inode.clone()) {
        // Final close may retire the endpoint while the VFS owner is creating
        // the node. We fail closed instead of holding a Unix lock across VFS or
        // unlinking by pathname: the inode is inert, has no registry/name
        // publication, and requires explicit unlink before reuse.
        knoticeln!(
            "unix bind: endpoint retired after pathname creation; inert inode remains path={} ino={} errno=EBADF",
            pathname,
            inode.ino(),
        );
        return Err(error);
    }
    Ok(())
}

fn listen_unix_stream(private: &AnyOpaque, backlog: i32) -> Result<(), SocketListenError> {
    listen(&endpoint(private).core, backlog)
}

fn connect_unix_stream(
    private: &AnyOpaque,
    address: SocketAddress,
) -> Result<(), SocketConnectError> {
    connect(&endpoint(private).core, address)
}

fn accept_unix_stream(private: &AnyOpaque) -> Result<SocketAcceptItem, SocketAcceptError> {
    accept(&endpoint(private).core)
}

fn query_unix_local_address(
    private: &AnyOpaque,
    sink: &mut dyn SocketAddressSink,
) -> Result<(), SocketQueryError> {
    let endpoint = &endpoint(private).core;
    if matches!(
        endpoint.state.lock().association,
        EndpointAssociation::Retired
    ) {
        return Err(SocketQueryError::Retired);
    }
    sink.copy_address(endpoint.name.snapshot().map(SocketAddress::UnixPathname))
        .map_err(SocketQueryError::Copy)
}

fn query_unix_peer_address(
    private: &AnyOpaque,
    sink: &mut dyn SocketAddressSink,
) -> Result<(), SocketQueryError> {
    let endpoint = &endpoint(private).core;
    let (connection, side) = endpoint.connected().map_err(|error| match error {
        EndpointAccessError::Unconnected | EndpointAccessError::InvalidState => {
            SocketQueryError::NotConnected
        },
        EndpointAccessError::Retired => SocketQueryError::Retired,
    })?;
    sink.copy_address(
        connection.names[side.peer().index()]
            .snapshot()
            .map(SocketAddress::UnixPathname),
    )
    .map_err(SocketQueryError::Copy)
}

fn receive_unix_stream(
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

fn send_unix_stream(
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

fn poll_unix_stream(
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

pub(super) fn retire_endpoint_core(endpoint: &Arc<UnixEndpointCore>) {
    let empty_lifecycle_routes = Arc::new(Vec::new());
    let empty_connect_routes = Arc::new(Vec::new());
    let empty_accept_routes = Arc::new(Vec::new());
    let (association, binding, lifecycle_routes, listener_close) = with_admission_commit(|| {
        let mut state = endpoint.state.lock();
        let association = core::mem::replace(&mut state.association, EndpointAssociation::Retired);
        assert!(
            !matches!(association, EndpointAssociation::Retired),
            "Unix endpoint final release ran more than once"
        );
        let listener_close = match &association {
            EndpointAssociation::Listening(listener) => {
                Some(listener.close(empty_connect_routes, empty_accept_routes))
            },
            _ => None,
        };
        let binding = core::mem::replace(&mut state.binding, BindingState::Unnamed);
        let lifecycle_routes =
            core::mem::replace(&mut state.lifecycle_routes, empty_lifecycle_routes);
        (association, binding, lifecycle_routes, listener_close)
    });
    if let BindingState::Bound(registration) = binding {
        withdraw_binding(registration);
    }
    notify_admission_routes(&lifecycle_routes, "endpoint retirement");

    if let Some(closed) = listener_close {
        notify_admission_routes(&closed.connect_routes, "listener close");
        notify_admission_routes(&closed.accept_routes, "listener close");
        for child in closed.pending {
            retire_endpoint_core(&child);
        }
        return;
    }

    let EndpointAssociation::Connected { connection, side } = association else {
        return;
    };
    let index = side.index();
    let peer_index = side.peer().index();
    let empty_routes = Arc::new(Vec::new());
    let (own_routes, peer_routes) = {
        let mut state = connection.state.lock();
        assert!(
            !state.endpoint_terminal[index],
            "Unix connection observed duplicate endpoint terminal handoff"
        );

        // Withdraw operation/source publication before exposing directional
        // terminal facts. New attempts fail closed; already staged attempts
        // recheck this admission under the same lock before commit.
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

fn final_release_unix_stream(private: &AnyOpaque) {
    retire_endpoint_core(&endpoint(private).core);
}

pub(in crate::fs::socket) static UNIX_STREAM_SOCKET_OPS: SocketOps = SocketOps {
    socket_type: SocketType::UnixStream,
    create: Some(prepare_unix_socket),
    create_pair: Some(prepare_unix_pair),
    bind: Some(bind_unix_stream),
    listen: Some(listen_unix_stream),
    connect: Some(connect_unix_stream),
    accept: Some(accept_unix_stream),
    local_address: Some(query_unix_local_address),
    peer_address: Some(query_unix_peer_address),
    send: Some(send_unix_stream),
    receive: Some(receive_unix_stream),
    poll: poll_unix_stream,
    final_release: final_release_unix_stream,
};

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::fs::{iomux::PollObserver, socket::prepare_socket};

    #[derive(Default)]
    struct AddressCapture(Option<SocketAddress>);

    impl SocketAddressSink for AddressCapture {
        fn copy_address(&mut self, address: Option<SocketAddress>) -> Result<(), SysError> {
            self.0 = address;
            Ok(())
        }
    }

    fn anonymous_socket_inode() -> (File, InodeRef) {
        let (file, creation) = prepare_socket(&UNIX_STREAM_SOCKET_OPS).unwrap();
        creation.commit();
        let inode = file.inode().clone();
        (file, inode)
    }

    fn receive_stream_for_test(
        private: &AnyOpaque,
        sink: &mut dyn SocketStreamReadSink,
    ) -> Result<usize, SocketReceiveError> {
        receive_unix_stream(private, SocketReceiveRequest::Stream(sink))
    }

    fn send_stream_for_test(
        private: &AnyOpaque,
        source: &mut dyn SocketStreamWriteSource,
    ) -> Result<usize, SocketSendError> {
        send_unix_stream(private, SocketSendRequest::Stream(source))
    }

    struct ReadCapture(Vec<u8>);

    impl SocketStreamReadSink for ReadCapture {
        fn remaining(&self) -> usize {
            usize::MAX
        }

        fn copy_bytes(&mut self, bytes: &[u8]) -> Result<usize, SysError> {
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
    }

    struct WriteBytes<'a>(&'a [u8]);

    impl SocketStreamWriteSource for WriteBytes<'_> {
        fn remaining(&self) -> usize {
            self.0.len()
        }

        fn copy_bytes(&mut self, bytes: &mut [u8]) -> Result<usize, SysError> {
            let copied = bytes.len().min(self.0.len());
            bytes[..copied].copy_from_slice(&self.0[..copied]);
            Ok(copied)
        }
    }

    struct PartialReadCapture {
        bytes: Vec<u8>,
        limit: usize,
    }

    impl SocketStreamReadSink for PartialReadCapture {
        fn remaining(&self) -> usize {
            usize::MAX
        }

        fn copy_bytes(&mut self, bytes: &[u8]) -> Result<usize, SysError> {
            let copied = bytes.len().min(self.limit);
            self.bytes.extend_from_slice(&bytes[..copied]);
            Ok(copied)
        }
    }

    struct PartialWriteBytes<'a> {
        bytes: &'a [u8],
        limit: usize,
    }

    impl SocketStreamWriteSource for PartialWriteBytes<'_> {
        fn remaining(&self) -> usize {
            self.bytes.len()
        }

        fn copy_bytes(&mut self, dst: &mut [u8]) -> Result<usize, SysError> {
            let copied = dst.len().min(self.bytes.len()).min(self.limit);
            dst[..copied].copy_from_slice(&self.bytes[..copied]);
            Ok(copied)
        }
    }

    struct FaultReadSink;

    impl SocketStreamReadSink for FaultReadSink {
        fn remaining(&self) -> usize {
            usize::MAX
        }

        fn copy_bytes(&mut self, _bytes: &[u8]) -> Result<usize, SysError> {
            Err(SysError::BadAddress)
        }
    }

    struct FaultWriteSource;

    impl SocketStreamWriteSource for FaultWriteSource {
        fn remaining(&self) -> usize {
            1
        }

        fn copy_bytes(&mut self, _bytes: &mut [u8]) -> Result<usize, SysError> {
            Err(SysError::BadAddress)
        }
    }

    struct CountingObserver(AtomicUsize);

    impl CountingObserver {
        fn notifications(&self) -> usize {
            self.0.load(Ordering::Acquire)
        }
    }

    impl PollObserver for CountingObserver {
        fn notify(&self) {
            self.0.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn route(observer: &Arc<CountingObserver>) -> PollRoute {
        let erased: Arc<dyn PollObserver> = observer.clone();
        let route = PollRoute::new(&erased);
        drop(erased);
        route
    }

    #[kunit]
    fn single_preparation_abort_and_unconnected_operations_are_typed() {
        let prepared = prepare_unix_socket().unwrap();
        let core = Arc::downgrade(&endpoint(&prepared.private).core);
        let mut local = AddressCapture::default();
        assert_eq!(
            query_unix_local_address(&prepared.private, &mut local),
            Ok(())
        );
        assert_eq!(local.0, None);
        assert_eq!(
            query_unix_peer_address(&prepared.private, &mut AddressCapture::default()),
            Err(SocketQueryError::NotConnected)
        );
        assert_eq!(
            send_stream_for_test(&prepared.private, &mut WriteBytes(b"x")),
            Err(SocketSendError::NotConnected)
        );
        assert_eq!(
            receive_stream_for_test(&prepared.private, &mut ReadCapture(Vec::new())),
            Err(SocketReceiveError::InvalidState)
        );

        drop(prepared);
        assert!(core.upgrade().is_none());
    }

    #[kunit]
    fn retirement_during_bind_preparation_publishes_no_name_or_registration() {
        let prepared = prepare_unix_socket().unwrap();
        let core = endpoint(&prepared.private).core.clone();
        let pathname: Arc<str> = Arc::from("/kunit/unix-retired-bind");
        core.begin_bind(pathname.clone()).unwrap();
        final_release_unix_stream(&prepared.private);

        let (_file, inode) = anonymous_socket_inode();
        assert_eq!(
            core.commit_bind(pathname, inode.clone()),
            Err(SocketBindError::Retired)
        );
        assert_eq!(core.name.snapshot(), None);
        assert!(super::super::namespace::lookup_binding(&inode).is_none());
    }

    #[kunit]
    fn connected_later_bind_is_one_time_and_peer_name_survives_close() {
        let pair = prepare_unix_pair().unwrap();
        let first = endpoint(&pair.first_private).core.clone();
        let pathname: Arc<str> = Arc::from("/kunit/unix-connected-name");
        first.begin_bind(pathname.clone()).unwrap();
        assert_eq!(
            first.begin_bind(Arc::from("/kunit/other")),
            Err(SocketBindError::AlreadyBound)
        );

        let (_file, inode) = anonymous_socket_inode();
        first.commit_bind(pathname.clone(), inode.clone()).unwrap();
        assert_eq!(
            first.begin_bind(Arc::from("/kunit/other")),
            Err(SocketBindError::AlreadyBound)
        );
        assert!(super::super::namespace::lookup_binding(&inode).is_some());

        let mut peer = AddressCapture::default();
        query_unix_peer_address(&pair.second_private, &mut peer).unwrap();
        assert_eq!(peer.0, Some(SocketAddress::UnixPathname(pathname.clone())));

        final_release_unix_stream(&pair.first_private);
        assert!(super::super::namespace::lookup_binding(&inode).is_none());
        let mut peer_after_close = AddressCapture::default();
        query_unix_peer_address(&pair.second_private, &mut peer_after_close).unwrap();
        assert_eq!(
            peer_after_close.0,
            Some(SocketAddress::UnixPathname(pathname))
        );
        final_release_unix_stream(&pair.second_private);
    }

    #[kunit]
    fn paired_directions_do_not_cross_and_preserve_order() {
        let pair = prepare_unix_pair().unwrap();
        assert_eq!(
            send_stream_for_test(&pair.first_private, &mut WriteBytes(b"first")),
            Ok(5)
        );
        assert_eq!(
            send_stream_for_test(&pair.second_private, &mut WriteBytes(b"second")),
            Ok(6)
        );

        let mut first_read = ReadCapture(Vec::new());
        let mut second_read = ReadCapture(Vec::new());
        assert_eq!(
            receive_stream_for_test(&pair.first_private, &mut first_read),
            Ok(6)
        );
        assert_eq!(
            receive_stream_for_test(&pair.second_private, &mut second_read),
            Ok(5)
        );
        assert_eq!(first_read.0, b"second");
        assert_eq!(second_read.0, b"first");
    }

    #[kunit]
    fn capacity_partial_progress_and_final_close_share_direction_truth() {
        let pair = prepare_unix_pair().unwrap();
        let bytes = vec![0x5a; UNIX_STREAM_DIRECTION_CAPACITY_BYTES + 1];
        assert_eq!(
            send_stream_for_test(&pair.first_private, &mut WriteBytes(&bytes)),
            Ok(UNIX_STREAM_DIRECTION_CAPACITY_BYTES)
        );
        assert_eq!(
            send_stream_for_test(&pair.first_private, &mut WriteBytes(b"x")),
            Err(SocketSendError::WouldBlock)
        );

        final_release_unix_stream(&pair.second_private);
        assert_eq!(
            send_stream_for_test(&pair.first_private, &mut WriteBytes(b"x")),
            Err(SocketSendError::PeerClosed)
        );
        let events = poll_unix_stream(
            &pair.first_private,
            &PollRequest::snapshot(PollEvent::READABLE | PollEvent::WRITABLE),
        )
        .unwrap()
        .expect_ready("Unix close KUnit");
        assert!(events.contains(PollEvent::HANG_UP));

        let mut drained = ReadCapture(Vec::new());
        assert_eq!(
            receive_stream_for_test(&pair.first_private, &mut drained),
            Ok(0)
        );
    }

    #[kunit]
    fn copy_fault_and_short_copy_commit_only_the_reported_prefix() {
        let pair = prepare_unix_pair().unwrap();

        assert_eq!(
            send_stream_for_test(&pair.first_private, &mut FaultWriteSource),
            Err(SocketSendError::Copy(SysError::BadAddress))
        );
        let mut empty = ReadCapture(Vec::new());
        assert_eq!(
            receive_stream_for_test(&pair.second_private, &mut empty),
            Err(SocketReceiveError::WouldBlock)
        );

        assert_eq!(
            send_stream_for_test(
                &pair.first_private,
                &mut PartialWriteBytes {
                    bytes: b"write",
                    limit: 2,
                },
            ),
            Ok(2)
        );
        let mut written = ReadCapture(Vec::new());
        assert_eq!(
            receive_stream_for_test(&pair.second_private, &mut written),
            Ok(2)
        );
        assert_eq!(written.0, b"wr");

        assert_eq!(
            send_stream_for_test(&pair.second_private, &mut WriteBytes(b"read")),
            Ok(4)
        );
        assert_eq!(
            receive_stream_for_test(&pair.first_private, &mut FaultReadSink),
            Err(SocketReceiveError::Copy(SysError::BadAddress))
        );
        let mut prefix = PartialReadCapture {
            bytes: Vec::new(),
            limit: 2,
        };
        assert_eq!(
            receive_stream_for_test(&pair.first_private, &mut prefix),
            Ok(2)
        );
        assert_eq!(prefix.bytes, b"re");
        let mut suffix = ReadCapture(Vec::new());
        assert_eq!(
            receive_stream_for_test(&pair.first_private, &mut suffix),
            Ok(2)
        );
        assert_eq!(suffix.0, b"ad");
    }

    #[kunit]
    fn register_snapshot_and_recheck_follow_direction_predicates() {
        let pair = prepare_unix_pair().unwrap();
        let readable = Arc::new(CountingObserver(AtomicUsize::new(0)));
        let writable = Arc::new(CountingObserver(AtomicUsize::new(0)));
        let readable_route = route(&readable);
        let writable_route = route(&writable);

        assert_eq!(
            poll_unix_stream(
                &pair.first_private,
                &PollRequest::register_with_route(PollEvent::READABLE, &readable_route),
            )
            .unwrap(),
            PollRegisterResult::Subscribed(PollEvent::empty())
        );
        assert_eq!(
            poll_unix_stream(
                &pair.second_private,
                &PollRequest::register_with_route(PollEvent::WRITABLE, &writable_route),
            )
            .unwrap(),
            PollRegisterResult::Subscribed(PollEvent::WRITABLE)
        );

        assert_eq!(
            send_stream_for_test(&pair.second_private, &mut WriteBytes(b"x")),
            Ok(1)
        );
        assert_eq!(readable.notifications(), 1);
        assert_eq!(writable.notifications(), 1);
        let events = poll_unix_stream(
            &pair.first_private,
            &PollRequest::snapshot(PollEvent::READABLE),
        )
        .unwrap()
        .expect_ready("Unix readable recheck");
        assert_eq!(events, PollEvent::READABLE);

        let mut byte = ReadCapture(Vec::new());
        assert_eq!(
            receive_stream_for_test(&pair.first_private, &mut byte),
            Ok(1)
        );
        assert_eq!(readable.notifications(), 2);
        let events = poll_unix_stream(
            &pair.first_private,
            &PollRequest::snapshot(PollEvent::READABLE),
        )
        .unwrap()
        .expect_ready("Unix empty recheck");
        assert!(events.is_empty());
    }

    #[kunit]
    fn retired_endpoint_poll_does_not_publish_route() {
        let pair = prepare_unix_pair().unwrap();
        let (connection, _) = endpoint(&pair.first_private).core.connected().unwrap();
        final_release_unix_stream(&pair.first_private);
        let observer = Arc::new(CountingObserver(AtomicUsize::new(0)));
        let poll_route = route(&observer);

        let events = poll_unix_stream(
            &pair.first_private,
            &PollRequest::register_with_route(PollEvent::READABLE, &poll_route),
        )
        .unwrap()
        .expect_ready("retired Unix endpoint poll");
        assert_eq!(events, PollEvent::HANG_UP);
        let error = endpoint(&pair.first_private)
            .core
            .connected()
            .expect_err("retired endpoint must reject connection access");
        assert_eq!(error, EndpointAccessError::Retired);
        assert!(connection.state.lock().routes[EndpointSide::First.index()].is_empty());
    }

    #[kunit]
    fn dropping_unpublished_pair_aborts_both_endpoints() {
        let pair = prepare_unix_pair().unwrap();
        let (strong, _) = endpoint(&pair.first_private).core.connected().unwrap();
        let connection = Arc::downgrade(&strong);
        assert!(connection.upgrade().is_some());
        drop(strong);
        drop(pair);
        assert!(connection.upgrade().is_none());
    }
}
