//! Unix endpoint role, local name, binding publication, and family composition.

mod stream;

use crate::{
    fs::iomux::PollRoute,
    prelude::*,
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use super::{
    super::{
        SocketAcceptError, SocketAcceptItem, SocketAddress, SocketAddressSink, SocketBindError,
        SocketConnectError, SocketCreation, SocketListenError, SocketOps, SocketPairPreparation,
        SocketPreparation, SocketQueryError, SocketReadSink, SocketReceiveError,
        SocketReceiveFlags, SocketReceiveRequest, SocketSendError, SocketSendRequest,
        SocketShutdown, SocketShutdownError, SocketStreamDestination, SocketType,
        SocketWriteSource,
    },
    admission::{
        UnixListener, accept, connect, listen, notify_admission_routes, poll_unix_listener,
        with_admission_commit,
    },
    namespace::{BindingRegistration, create_socket_pathname, publish_binding, withdraw_binding},
};

pub(super) use stream::{EndpointSide, UnixConnection};
use stream::{
    poll_connected_unix_stream, receive_unix_stream, retire_connection_endpoint, send_unix_stream,
    shutdown_unix_stream,
};

#[derive(Clone, Debug)]
pub(super) struct UnixPollRoute {
    /// Non-owning consumer capability; it carries no readiness payload.
    pub(super) route: PollRoute,
    /// Notification filter only. Exact readiness is always re-derived from the
    /// endpoint's current role owner after a hint or role handoff.
    pub(super) interests: PollEvent,
}

impl UnixPollRoute {
    pub(super) fn new(route: &PollRoute, interests: PollEvent) -> Self {
        Self {
            route: route.clone(),
            interests,
        }
    }
}

pub(super) fn replacement_poll_routes(
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
pub(super) enum BindingPublication {
    /// No live namespace registration. The endpoint may still carry an
    /// inherited immutable local name, as accepted sockets do.
    Absent,
    Preparing(Arc<str>),
    Live(BindingRegistration),
}

#[derive(Debug)]
pub(super) struct EndpointState {
    pub(super) association: EndpointAssociation,
    pub(super) binding: BindingPublication,
    /// Routes interested in endpoint role/lifecycle changes. A role commit
    /// hands them to the new listener/connection predicate owner; they carry
    /// no role, readiness, or errno truth and are pruned opportunistically.
    pub(super) lifecycle_routes: Arc<Vec<UnixPollRoute>>,
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
                binding: BindingPublication::Absent,
                lifecycle_routes: Arc::new(Vec::new()),
            }),
            name: EndpointName::new(),
        })
    }

    pub(super) fn new_with_name(name: Arc<EndpointName>) -> Arc<Self> {
        Arc::new(Self {
            state: SpinLock::new(EndpointState {
                association: EndpointAssociation::Unconnected,
                binding: BindingPublication::Absent,
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
        empty_lifecycle_routes: Arc<Vec<UnixPollRoute>>,
    ) -> Arc<Vec<UnixPollRoute>> {
        let mut state = self.state.lock();
        assert!(matches!(
            state.association,
            EndpointAssociation::Unconnected
        ));
        let routes = core::mem::replace(&mut state.lifecycle_routes, empty_lifecycle_routes);
        connection.install_routes(side, routes.clone());
        state.association = EndpointAssociation::Connected { connection, side };
        routes
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
        // Accepted endpoints inherit the listener's immutable local name but
        // never its live namespace registration. Reject them before VFS node
        // creation instead of treating registration absence as an unnamed
        // endpoint and reaching the duplicate-name commit assertion.
        if self.name.snapshot().is_some() || !matches!(state.binding, BindingPublication::Absent) {
            return Err(SocketBindError::AlreadyBound);
        }
        state.binding = BindingPublication::Preparing(pathname);
        Ok(())
    }

    fn abort_bind(&self, pathname: &Arc<str>) {
        let mut state = self.state.lock();
        if matches!(
            &state.binding,
            BindingPublication::Preparing(current) if Arc::ptr_eq(current, pathname)
        ) {
            state.binding = BindingPublication::Absent;
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
                BindingPublication::Preparing(current) if Arc::ptr_eq(current, &pathname)
            ),
            "Unix bind commit lost its endpoint-local preparation"
        );

        // Endpoint -> registry -> name is the sole publication order. Registry
        // lookup drops its lock before using the returned capability, so no
        // reverse acquisition exists. Heap allocation here is kernel-fatal,
        // not a returnable post-VFS failure.
        let registration = publish_binding(inode, self);
        self.name.publish(pathname);
        state.binding = BindingPublication::Live(registration);
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

fn query_unix_accepting(private: &AnyOpaque) -> Result<bool, SocketQueryError> {
    match &endpoint(private).core.state.lock().association {
        EndpointAssociation::Listening(_) => Ok(true),
        EndpointAssociation::Retired => Err(SocketQueryError::Retired),
        EndpointAssociation::Unconnected | EndpointAssociation::Connected { .. } => Ok(false),
    }
}

fn unconnected_poll_events(request: &PollRequest<'_>) -> PollEvent {
    PollEvent::HANG_UP | (PollEvent::WRITABLE & request.interests())
}

fn poll_unconnected_endpoint(
    endpoint: &UnixEndpointCore,
    request: &PollRequest<'_>,
) -> Result<Option<PollRegisterResult>, SysError> {
    let Some(route) = request.route() else {
        let state = endpoint.state.lock();
        return Ok(
            matches!(state.association, EndpointAssociation::Unconnected)
                .then(|| PollRegisterResult::Ready(unconnected_poll_events(request))),
        );
    };

    loop {
        let current = {
            let state = endpoint.state.lock();
            if !matches!(state.association, EndpointAssociation::Unconnected) {
                return Ok(None);
            }
            state.lifecycle_routes.clone()
        };
        let replacement = replacement_poll_routes(&current, route, request.interests());
        let (previous, result, retry) = with_admission_commit(|| {
            let mut state = endpoint.state.lock();
            if !matches!(state.association, EndpointAssociation::Unconnected) {
                return (None, None, false);
            }
            if !Arc::ptr_eq(&state.lifecycle_routes, &current) {
                return (None, None, true);
            }
            let previous = core::mem::replace(&mut state.lifecycle_routes, replacement);
            (
                Some(previous),
                Some(PollRegisterResult::Subscribed(unconnected_poll_events(
                    request,
                ))),
                false,
            )
        });
        drop(previous);
        if retry {
            continue;
        }
        return Ok(result);
    }
}

fn poll_unix_stream(
    private: &AnyOpaque,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    let endpoint = &endpoint(private).core;
    loop {
        enum Dispatch {
            Unconnected,
            Listening(Arc<UnixListener>),
            Connected,
            Retired,
        }

        let dispatch = match &endpoint.state.lock().association {
            EndpointAssociation::Unconnected => Dispatch::Unconnected,
            EndpointAssociation::Listening(listener) => Dispatch::Listening(listener.clone()),
            EndpointAssociation::Connected { .. } => Dispatch::Connected,
            EndpointAssociation::Retired => Dispatch::Retired,
        };
        match dispatch {
            Dispatch::Unconnected => {
                // Linux AF_UNIX keeps an unconnected/bound stream in its
                // terminal pre-connection role: HUP is mandatory and a send
                // attempt can complete immediately with its role error. A
                // persistent route is still required so listen/connect can
                // hand the watch into the new predicate owner.
                if let Some(result) = poll_unconnected_endpoint(endpoint, request)? {
                    return Ok(result);
                }
            },
            Dispatch::Listening(listener) => {
                return poll_unix_listener(endpoint, &listener, request);
            },
            Dispatch::Connected => return poll_connected_unix_stream(private, request),
            Dispatch::Retired => {
                return Ok(PollRegisterResult::Ready(PollEvent::HANG_UP));
            },
        }
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
        let binding = core::mem::replace(&mut state.binding, BindingPublication::Absent);
        let lifecycle_routes =
            core::mem::replace(&mut state.lifecycle_routes, empty_lifecycle_routes);
        (association, binding, lifecycle_routes, listener_close)
    });
    if let BindingPublication::Live(registration) = binding {
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
    retire_connection_endpoint(&connection, side);
}

fn final_release_unix_stream(private: &AnyOpaque) {
    retire_endpoint_core(&endpoint(private).core);
}

pub(in crate::fs::socket) static UNIX_STREAM_SOCKET_OPS: SocketOps = SocketOps {
    socket_type: SocketType::UnixStream,
    file_io: super::super::SocketFileIo::ByteStream,
    create: Some(prepare_unix_socket),
    create_pair: Some(prepare_unix_pair),
    bind: Some(bind_unix_stream),
    listen: Some(listen_unix_stream),
    connect: Some(connect_unix_stream),
    accept: Some(accept_unix_stream),
    shutdown: Some(shutdown_unix_stream),
    local_address: Some(query_unix_local_address),
    peer_address: Some(query_unix_peer_address),
    accepting: query_unix_accepting,
    send: Some(send_unix_stream),
    receive: Some(receive_unix_stream),
    query_option: None,
    mutate_option: None,
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
        sink: &mut dyn SocketReadSink,
    ) -> Result<usize, SocketReceiveError> {
        receive_stream_with_flags_for_test(private, sink, false)
    }

    fn receive_stream_with_flags_for_test(
        private: &AnyOpaque,
        sink: &mut dyn SocketReadSink,
        peek: bool,
    ) -> Result<usize, SocketReceiveError> {
        receive_unix_stream(
            private,
            SocketReceiveRequest::Stream {
                sink,
                flags: SocketReceiveFlags { peek },
            },
        )
        .map(|outcome| outcome.copied())
    }

    fn send_stream_for_test(
        private: &AnyOpaque,
        source: &mut dyn SocketWriteSource,
    ) -> Result<usize, SocketSendError> {
        send_stream_with_destination_for_test(private, source, SocketStreamDestination::Absent)
    }

    fn send_stream_with_destination_for_test(
        private: &AnyOpaque,
        source: &mut dyn SocketWriteSource,
        destination: SocketStreamDestination,
    ) -> Result<usize, SocketSendError> {
        send_unix_stream(
            private,
            SocketSendRequest::Stream {
                source,
                destination,
            },
        )
    }

    struct ReadCapture(Vec<u8>);

    impl SocketReadSink for ReadCapture {
        fn remaining(&self) -> usize {
            usize::MAX
        }

        fn copy_bytes(&mut self, bytes: &[u8]) -> Result<usize, SysError> {
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
    }

    struct WriteBytes<'a>(&'a [u8]);

    impl SocketWriteSource for WriteBytes<'_> {
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

    impl SocketReadSink for PartialReadCapture {
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

    impl SocketWriteSource for PartialWriteBytes<'_> {
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

    impl SocketReadSink for FaultReadSink {
        fn remaining(&self) -> usize {
            usize::MAX
        }

        fn copy_bytes(&mut self, _bytes: &[u8]) -> Result<usize, SysError> {
            Err(SysError::BadAddress)
        }
    }

    struct FaultWriteSource;

    impl SocketWriteSource for FaultWriteSource {
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
    fn accepted_name_rejects_bind_before_namespace_preparation() {
        let listener = UnixEndpointCore::new_unconnected();
        let listener_name: Arc<str> = Arc::from("/kunit/unix-listener-name");
        listener.name.publish(listener_name.clone());
        let accepted = UnixEndpointCore::new_with_name(listener.name.clone());

        assert_eq!(
            accepted.begin_bind(Arc::from("/kunit/unix-accepted-rebind")),
            Err(SocketBindError::AlreadyBound)
        );
        assert!(matches!(
            accepted.state.lock().binding,
            BindingPublication::Absent
        ));
        assert_eq!(accepted.name.snapshot(), Some(listener_name));
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
    fn peek_copies_without_consuming_or_releasing_capacity() {
        let pair = prepare_unix_pair().unwrap();
        assert_eq!(
            send_stream_for_test(&pair.first_private, &mut WriteBytes(b"peek")),
            Ok(4)
        );

        let mut peek = PartialReadCapture {
            bytes: Vec::new(),
            limit: 2,
        };
        assert_eq!(
            receive_stream_with_flags_for_test(&pair.second_private, &mut peek, true),
            Ok(2)
        );
        assert_eq!(peek.bytes, b"pe");

        let mut whole = ReadCapture(Vec::new());
        assert_eq!(
            receive_stream_for_test(&pair.second_private, &mut whole),
            Ok(4)
        );
        assert_eq!(whole.0, b"peek");
    }

    #[kunit]
    fn shutdown_is_idempotent_and_direction_owned() {
        let pair = prepare_unix_pair().unwrap();
        assert_eq!(
            send_stream_for_test(&pair.first_private, &mut WriteBytes(b"queued")),
            Ok(6)
        );
        assert_eq!(
            shutdown_unix_stream(&pair.first_private, SocketShutdown::Write),
            Ok(())
        );
        assert_eq!(
            shutdown_unix_stream(&pair.first_private, SocketShutdown::Write),
            Ok(())
        );
        assert_eq!(
            send_stream_for_test(&pair.first_private, &mut WriteBytes(b"x")),
            Err(SocketSendError::PeerClosed)
        );

        let mut queued = ReadCapture(Vec::new());
        assert_eq!(
            receive_stream_for_test(&pair.second_private, &mut queued),
            Ok(6)
        );
        assert_eq!(queued.0, b"queued");
        assert_eq!(
            receive_stream_for_test(&pair.second_private, &mut ReadCapture(Vec::new())),
            Ok(0)
        );
        assert_eq!(
            send_stream_for_test(&pair.second_private, &mut WriteBytes(b"reply")),
            Ok(5)
        );
        let mut reply = ReadCapture(Vec::new());
        assert_eq!(
            receive_stream_for_test(&pair.first_private, &mut reply),
            Ok(5)
        );
        assert_eq!(reply.0, b"reply");

        let pair = prepare_unix_pair().unwrap();
        assert_eq!(
            send_stream_for_test(&pair.second_private, &mut WriteBytes(b"buffered")),
            Ok(8)
        );
        assert_eq!(
            shutdown_unix_stream(&pair.first_private, SocketShutdown::Read),
            Ok(())
        );
        let mut buffered = ReadCapture(Vec::new());
        assert_eq!(
            receive_stream_for_test(&pair.first_private, &mut buffered),
            Ok(8)
        );
        assert_eq!(buffered.0, b"buffered");
        assert_eq!(
            receive_stream_for_test(&pair.first_private, &mut ReadCapture(Vec::new())),
            Ok(0)
        );
        assert_eq!(
            send_stream_for_test(&pair.second_private, &mut WriteBytes(b"x")),
            Err(SocketSendError::PeerClosed)
        );

        assert_eq!(
            shutdown_unix_stream(&pair.first_private, SocketShutdown::ReadWrite),
            Ok(())
        );
        final_release_unix_stream(&pair.first_private);
        final_release_unix_stream(&pair.second_private);
    }

    #[kunit]
    fn role_and_receive_half_close_readiness_are_owner_projections() {
        let single = prepare_unix_socket().unwrap();
        let events = poll_unix_stream(
            &single.private,
            &PollRequest::snapshot(
                PollEvent::READABLE | PollEvent::WRITABLE | PollEvent::READ_HANG_UP,
            ),
        )
        .unwrap()
        .expect_ready("unconnected Unix readiness");
        assert_eq!(events, PollEvent::WRITABLE | PollEvent::HANG_UP);

        let pair = prepare_unix_pair().unwrap();
        assert_eq!(
            send_stream_for_test(&pair.second_private, &mut WriteBytes(b"buffered")),
            Ok(8)
        );
        assert_eq!(
            shutdown_unix_stream(&pair.second_private, SocketShutdown::Write),
            Ok(())
        );
        let interests = PollEvent::READABLE | PollEvent::WRITABLE | PollEvent::READ_HANG_UP;
        let events = poll_unix_stream(&pair.first_private, &PollRequest::snapshot(interests))
            .unwrap()
            .expect_ready("peer write shutdown readiness");
        assert!(events.contains(PollEvent::READABLE));
        assert!(events.contains(PollEvent::WRITABLE));
        assert!(events.contains(PollEvent::READ_HANG_UP));
        assert!(!events.contains(PollEvent::HANG_UP));

        let mut buffered = ReadCapture(Vec::new());
        assert_eq!(
            receive_stream_for_test(&pair.first_private, &mut buffered),
            Ok(8)
        );
        assert_eq!(buffered.0, b"buffered");
        let events = poll_unix_stream(&pair.first_private, &PollRequest::snapshot(interests))
            .unwrap()
            .expect_ready("drained EOF readiness");
        assert!(events.contains(PollEvent::READABLE | PollEvent::READ_HANG_UP));
        assert!(!events.contains(PollEvent::HANG_UP));

        assert_eq!(
            shutdown_unix_stream(&pair.first_private, SocketShutdown::Write),
            Ok(())
        );
        let events = poll_unix_stream(&pair.first_private, &PollRequest::snapshot(interests))
            .unwrap()
            .expect_ready("full terminal readiness");
        assert!(events.contains(PollEvent::HANG_UP));
    }

    #[kunit]
    fn preconnection_poll_route_moves_to_connection_owner() {
        let prepared = prepare_unix_socket().unwrap();
        let client = endpoint(&prepared.private).core.clone();
        let observer = Arc::new(CountingObserver(AtomicUsize::new(0)));
        let poll_route = route(&observer);
        assert_eq!(
            poll_unix_stream(
                &prepared.private,
                &PollRequest::register_with_route(PollEvent::READABLE, &poll_route),
            )
            .unwrap(),
            PollRegisterResult::Subscribed(PollEvent::HANG_UP)
        );

        let peer = UnixEndpointCore::new_unconnected();
        let connection = UnixConnection::new([client.name.clone(), peer.name.clone()]);
        peer.install_connection(connection.clone(), EndpointSide::Second);
        let routes =
            client.commit_connection(connection, EndpointSide::First, Arc::new(Vec::new()));
        notify_admission_routes(&routes, "KUnit public poll connection commit");
        assert_eq!(observer.notifications(), 1);

        let peer_private = private_from_core(peer);
        assert_eq!(
            send_stream_for_test(&peer_private, &mut WriteBytes(b"x")),
            Ok(1)
        );
        assert_eq!(observer.notifications(), 2);
        final_release_unix_stream(&peer_private);
        final_release_unix_stream(&prepared.private);
    }

    #[kunit]
    fn prelisten_poll_route_moves_to_listener_owner() {
        let prepared = prepare_unix_socket().unwrap();
        let listener_endpoint = endpoint(&prepared.private).core.clone();
        let pathname: Arc<str> = Arc::from("/kunit/unix-prelisten-poll");
        listener_endpoint.begin_bind(pathname.clone()).unwrap();
        let (_file, inode) = anonymous_socket_inode();
        listener_endpoint.commit_bind(pathname, inode).unwrap();

        let observer = Arc::new(CountingObserver(AtomicUsize::new(0)));
        let poll_route = route(&observer);
        assert_eq!(
            poll_unix_stream(
                &prepared.private,
                &PollRequest::register_with_route(PollEvent::READABLE, &poll_route),
            )
            .unwrap(),
            PollRegisterResult::Subscribed(PollEvent::HANG_UP)
        );
        listen(&listener_endpoint, 0).unwrap();
        assert_eq!(observer.notifications(), 1);

        let listener = match &listener_endpoint.state.lock().association {
            EndpointAssociation::Listening(listener) => listener.clone(),
            association => panic!("listen committed unexpected association {association:?}"),
        };
        let routes = listener
            .admit_for_validation(UnixEndpointCore::new_unconnected())
            .unwrap();
        notify_admission_routes(&routes, "KUnit public poll listener admission");
        assert_eq!(observer.notifications(), 2);
        assert_eq!(
            poll_unix_stream(
                &prepared.private,
                &PollRequest::snapshot(PollEvent::READABLE),
            )
            .unwrap(),
            PollRegisterResult::Ready(PollEvent::READABLE)
        );
        final_release_unix_stream(&prepared.private);
    }

    #[kunit]
    fn stream_destination_and_r1_unconnected_shutdown_return_typed_outcomes() {
        let pair = prepare_unix_pair().unwrap();
        assert_eq!(
            send_stream_with_destination_for_test(
                &pair.first_private,
                &mut WriteBytes(b"x"),
                SocketStreamDestination::Present,
            ),
            Err(SocketSendError::AlreadyConnected)
        );

        let single = prepare_unix_socket().unwrap();
        for how in [
            SocketShutdown::Read,
            SocketShutdown::Write,
            SocketShutdown::ReadWrite,
        ] {
            assert_eq!(
                shutdown_unix_stream(&single.private, how),
                Err(SocketShutdownError::NotConnected)
            );
        }
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
