//! Unix listener backlog, connection admission, and operation wait sources.

use crate::{
    fs::iomux::PollRoute, kconfig_defs::UNIX_LISTENER_MAX_BACKLOG, prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::{
    super::{
        SocketAcceptError, SocketAcceptItem, SocketAddress, SocketConnectError, SocketListenError,
        SocketWait,
    },
    endpoint::{
        BindingPublication, EndpointAssociation, EndpointSide, UnixConnection, UnixEndpointCore,
        UnixPollRoute, private_from_core, replacement_poll_routes,
    },
    namespace::{LiveBinding, resolve_live_binding},
};

static_assert!(
    UNIX_LISTENER_MAX_BACKLOG < usize::MAX,
    "unix_listener_max_backlog must leave room for Linux's backlog-plus-one queue"
);

/// Serializes only Unix admission commits and their lifecycle revalidation.
/// No allocation, VFS operation, user copy, or wait occurs while held.
static ADMISSION_COMMIT: SpinLock<()> = SpinLock::new(());

pub(super) fn with_admission_commit<T>(f: impl FnOnce() -> T) -> T {
    let _guard = ADMISSION_COMMIT.lock();
    f()
}

fn normalized_backlog(backlog: i32) -> usize {
    // Linux compares the signed argument as unsigned against somaxconn, so a
    // negative backlog is clamped to the configured maximum.
    (backlog as u32 as usize).min(UNIX_LISTENER_MAX_BACKLOG)
}

pub(super) fn notify_admission_routes(routes: &Arc<Vec<UnixPollRoute>>, reason: &'static str) {
    if routes.is_empty() {
        return;
    }
    for entry in routes.iter() {
        entry.route.notify();
    }
    kdebugln!(
        "unix socket: issued {} admission route hints reason={}",
        routes.len(),
        reason,
    );
}

#[derive(Debug)]
struct ListenerState {
    backlog: usize,
    pending: VecDeque<Arc<UnixEndpointCore>>,
    connect_routes: Arc<Vec<UnixPollRoute>>,
    accept_routes: Arc<Vec<UnixPollRoute>>,
    closed: bool,
}

#[derive(Debug)]
pub(super) struct UnixListener {
    /// Sole owner of backlog, pending children, and the two operation-specific
    /// predicates. Route lists only carry recheck capabilities, never facts.
    state: SpinLock<ListenerState>,
}

impl UnixListener {
    fn new(backlog: usize) -> Arc<Self> {
        Arc::new(Self {
            state: SpinLock::new(ListenerState {
                backlog,
                pending: VecDeque::with_capacity(UNIX_LISTENER_MAX_BACKLOG + 1),
                connect_routes: Arc::new(Vec::new()),
                accept_routes: Arc::new(Vec::new()),
                closed: false,
            }),
        })
    }

    fn has_connect_capacity(state: &ListenerState) -> bool {
        !state.closed && state.pending.len() <= state.backlog
    }

    fn update_backlog(&self, backlog: usize) -> Option<Arc<Vec<UnixPollRoute>>> {
        let mut state = self.state.lock();
        assert!(
            !state.closed,
            "closed Unix listener remained endpoint-visible"
        );
        let was_ready = Self::has_connect_capacity(&state);
        state.backlog = backlog;
        let is_ready = Self::has_connect_capacity(&state);
        (!was_ready && is_ready).then(|| state.connect_routes.clone())
    }

    fn try_push(&self, child: Arc<UnixEndpointCore>) -> Result<Arc<Vec<UnixPollRoute>>, ()> {
        let mut state = self.state.lock();
        if !Self::has_connect_capacity(&state) {
            return Err(());
        }
        assert!(
            state.pending.len() < state.pending.capacity(),
            "Unix listener queue outgrew its configured allocation"
        );
        state.pending.push_back(child);
        Ok(state.accept_routes.clone())
    }

    #[cfg(feature = "kunit")]
    pub(super) fn admit_for_validation(
        &self,
        child: Arc<UnixEndpointCore>,
    ) -> Result<Arc<Vec<UnixPollRoute>>, ()> {
        self.try_push(child)
    }

    fn pop(&self) -> Result<(Arc<UnixEndpointCore>, Arc<Vec<UnixPollRoute>>), ()> {
        let mut state = self.state.lock();
        let child = state.pending.pop_front().ok_or(())?;
        Ok((child, state.connect_routes.clone()))
    }

    pub(super) fn close(
        &self,
        empty_connect_routes: Arc<Vec<UnixPollRoute>>,
        empty_accept_routes: Arc<Vec<UnixPollRoute>>,
    ) -> ListenerClose {
        let mut state = self.state.lock();
        assert!(!state.closed, "Unix listener closed twice");
        state.closed = true;
        ListenerClose {
            pending: core::mem::take(&mut state.pending),
            connect_routes: core::mem::replace(&mut state.connect_routes, empty_connect_routes),
            accept_routes: core::mem::replace(&mut state.accept_routes, empty_accept_routes),
        }
    }

    fn install_accept_routes(&self, routes: Arc<Vec<UnixPollRoute>>) {
        let mut state = self.state.lock();
        assert!(
            state.accept_routes.is_empty(),
            "Unix listener routes installed twice"
        );
        state.accept_routes = routes;
    }
}

pub(super) struct ListenerClose {
    pub(super) pending: VecDeque<Arc<UnixEndpointCore>>,
    pub(super) connect_routes: Arc<Vec<UnixPollRoute>>,
    pub(super) accept_routes: Arc<Vec<UnixPollRoute>>,
}

#[derive(Debug, Opaque)]
struct ConnectWaitSource {
    client: Arc<UnixEndpointCore>,
    listener_endpoint: Arc<UnixEndpointCore>,
    listener: Arc<UnixListener>,
}

#[derive(Debug, Opaque)]
struct AcceptWaitSource {
    listener_endpoint: Arc<UnixEndpointCore>,
    listener: Arc<UnixListener>,
}

fn listener_is_current(endpoint: &UnixEndpointCore, expected: &Arc<UnixListener>) -> bool {
    matches!(
        &endpoint.state.lock().association,
        EndpointAssociation::Listening(current) if Arc::ptr_eq(current, expected)
    )
}

fn connect_wait_ready(source: &ConnectWaitSource) -> bool {
    if !matches!(
        source.client.state.lock().association,
        EndpointAssociation::Unconnected
    ) || !listener_is_current(&source.listener_endpoint, &source.listener)
    {
        return true;
    }
    UnixListener::has_connect_capacity(&source.listener.state.lock())
}

fn poll_connect_wait(
    private: &AnyOpaque,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    let source = private
        .cast::<ConnectWaitSource>()
        .expect("Unix connect wait used without its source");
    let Some(route) = request.route() else {
        return Ok(PollRegisterResult::Ready(
            connect_wait_ready(source)
                .then_some(PollEvent::WRITABLE)
                .unwrap_or_else(PollEvent::empty),
        ));
    };

    loop {
        let client_routes = source.client.state.lock().lifecycle_routes.clone();
        let listener_routes = source.listener.state.lock().connect_routes.clone();
        let client_replacement =
            replacement_poll_routes(&client_routes, route, request.interests());
        let listener_replacement =
            replacement_poll_routes(&listener_routes, route, request.interests());

        let (old_client, old_listener, result, retry) = with_admission_commit(|| {
            let client = source.client.state.lock();
            if !matches!(client.association, EndpointAssociation::Unconnected) {
                return (
                    None,
                    None,
                    Some(PollRegisterResult::Ready(PollEvent::WRITABLE)),
                    false,
                );
            }
            if !Arc::ptr_eq(&client.lifecycle_routes, &client_routes) {
                return (None, None, None, true);
            }
            drop(client);

            if !listener_is_current(&source.listener_endpoint, &source.listener) {
                return (
                    None,
                    None,
                    Some(PollRegisterResult::Ready(PollEvent::WRITABLE)),
                    false,
                );
            }
            let mut listener = source.listener.state.lock();
            if !Arc::ptr_eq(&listener.connect_routes, &listener_routes) {
                return (None, None, None, true);
            }
            let ready = UnixListener::has_connect_capacity(&listener);
            let old_listener =
                core::mem::replace(&mut listener.connect_routes, listener_replacement);
            drop(listener);

            let mut client = source.client.state.lock();
            assert!(matches!(
                client.association,
                EndpointAssociation::Unconnected
            ));
            assert!(Arc::ptr_eq(&client.lifecycle_routes, &client_routes));
            let old_client = core::mem::replace(&mut client.lifecycle_routes, client_replacement);
            (
                Some(old_client),
                Some(old_listener),
                Some(PollRegisterResult::Subscribed(
                    ready
                        .then_some(PollEvent::WRITABLE)
                        .unwrap_or_else(PollEvent::empty),
                )),
                false,
            )
        });
        drop(old_client);
        drop(old_listener);
        if retry {
            continue;
        }
        return Ok(result.expect("Unix connect registration produced no result"));
    }
}

fn poll_accept_predicate(
    listener_endpoint: &UnixEndpointCore,
    listener: &Arc<UnixListener>,
    request: &PollRequest<'_>,
    terminal: PollEvent,
) -> Result<PollRegisterResult, SysError> {
    let ready = PollEvent::READABLE & request.interests();
    let Some(route) = request.route() else {
        if !listener_is_current(listener_endpoint, listener) {
            return Ok(PollRegisterResult::Ready(terminal));
        }
        let state = listener.state.lock();
        let events = if state.closed {
            terminal
        } else if !state.pending.is_empty() {
            ready
        } else {
            PollEvent::empty()
        };
        return Ok(PollRegisterResult::Ready(events));
    };

    loop {
        let current = listener.state.lock().accept_routes.clone();
        let replacement = replacement_poll_routes(&current, route, request.interests());
        let (old, result, retry) = with_admission_commit(|| {
            if !listener_is_current(listener_endpoint, listener) {
                return (None, Some(PollRegisterResult::Ready(terminal)), false);
            }
            let mut state = listener.state.lock();
            if !Arc::ptr_eq(&state.accept_routes, &current) {
                return (None, None, true);
            }
            let events = if state.closed {
                terminal
            } else if !state.pending.is_empty() {
                ready
            } else {
                PollEvent::empty()
            };
            let old = core::mem::replace(&mut state.accept_routes, replacement);
            (
                Some(old),
                Some(PollRegisterResult::Subscribed(events)),
                false,
            )
        });
        drop(old);
        if retry {
            continue;
        }
        return Ok(result.expect("Unix accept registration produced no result"));
    }
}

fn poll_accept_wait(
    private: &AnyOpaque,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    let source = private
        .cast::<AcceptWaitSource>()
        .expect("Unix accept wait used without its source");
    // A terminal listener makes the next accept attempt complete with an
    // error, so the operation-local wait source reports READABLE. Public
    // listener poll uses the same predicate and routes but projects HANG_UP.
    poll_accept_predicate(
        &source.listener_endpoint,
        &source.listener,
        request,
        PollEvent::READABLE,
    )
}

pub(super) fn poll_unix_listener(
    listener_endpoint: &UnixEndpointCore,
    listener: &Arc<UnixListener>,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    poll_accept_predicate(listener_endpoint, listener, request, PollEvent::HANG_UP)
}

pub(super) fn listen(
    endpoint: &Arc<UnixEndpointCore>,
    backlog: i32,
) -> Result<(), SocketListenError> {
    let backlog = normalized_backlog(backlog);
    // Preallocate the maximum queue before the endpoint becomes a listener.
    // Repeated listen calls discard this candidate and update the existing owner.
    let candidate = UnixListener::new(backlog);
    let empty_lifecycle_routes = Arc::new(Vec::new());
    let notify = with_admission_commit(|| {
        let mut state = endpoint.state.lock();
        if matches!(&state.association, EndpointAssociation::Retired) {
            return Err(SocketListenError::Retired);
        }
        if !matches!(state.binding, BindingPublication::Live(_)) {
            return Err(SocketListenError::InvalidState);
        }
        match &state.association {
            EndpointAssociation::Unconnected => {
                let lifecycle_routes =
                    core::mem::replace(&mut state.lifecycle_routes, empty_lifecycle_routes);
                candidate.install_accept_routes(lifecycle_routes.clone());
                state.association = EndpointAssociation::Listening(candidate);
                Ok((None, Some(lifecycle_routes)))
            },
            EndpointAssociation::Listening(listener) => {
                Ok((listener.update_backlog(backlog), None))
            },
            EndpointAssociation::Connected { .. } => Err(SocketListenError::InvalidState),
            EndpointAssociation::Retired => Err(SocketListenError::Retired),
        }
    })?;
    if let Some(routes) = notify.0 {
        notify_admission_routes(&routes, "listen backlog growth");
    }
    if let Some(routes) = notify.1 {
        notify_admission_routes(&routes, "endpoint became listener");
    }
    Ok(())
}

fn current_listener(
    endpoint: &UnixEndpointCore,
    binding: &LiveBinding,
) -> Result<Arc<UnixListener>, SocketConnectError> {
    let state = endpoint.state.lock();
    let binding_matches = matches!(
        &state.binding,
        BindingPublication::Live(registration) if binding.matches_registration(registration)
    );
    if !binding_matches {
        return Err(SocketConnectError::ConnectionRefused);
    }
    match &state.association {
        EndpointAssociation::Listening(listener) => Ok(listener.clone()),
        EndpointAssociation::Retired => Err(SocketConnectError::ConnectionRefused),
        EndpointAssociation::Unconnected | EndpointAssociation::Connected { .. } => {
            Err(SocketConnectError::ConnectionRefused)
        },
    }
}

pub(super) fn connect(
    client: &Arc<UnixEndpointCore>,
    address: SocketAddress,
) -> Result<(), SocketConnectError> {
    let SocketAddress::UnixPathname(pathname) = address else {
        return Err(SocketConnectError::Unsupported);
    };
    let binding = resolve_live_binding(&pathname).map_err(|error| match error {
        SysError::ConnectionRefused => SocketConnectError::ConnectionRefused,
        error => SocketConnectError::Operation(error),
    })?;
    let listener_endpoint = binding
        .endpoint()
        .ok_or(SocketConnectError::ConnectionRefused)?;

    // Prepare every object before the commit gate. The accepted endpoint
    // shares only the listener's immutable name capability, never registration.
    let accepted = UnixEndpointCore::new_with_name(listener_endpoint.name.clone());
    let connection = UnixConnection::new([client.name.clone(), accepted.name.clone()]);
    let empty_client_routes = Arc::new(Vec::new());

    let result = with_admission_commit(|| {
        let listener = current_listener(&listener_endpoint, &binding)?;
        {
            let state = client.state.lock();
            match state.association {
                EndpointAssociation::Unconnected => {},
                EndpointAssociation::Connected { .. } => {
                    return Err(SocketConnectError::AlreadyConnected);
                },
                EndpointAssociation::Listening(_) => {
                    return Err(SocketConnectError::InvalidState);
                },
                EndpointAssociation::Retired => return Err(SocketConnectError::Retired),
            }
        }

        let accept_routes = match listener.try_push(accepted.clone()) {
            Ok(routes) => routes,
            Err(()) => return Ok(ConnectCommit::Full(listener)),
        };

        // Queue capacity was committed first and every remaining transition is
        // infallible. The client association is the externally observable
        // linearization point; the queued child already owns the peer side.
        accepted.install_connection(connection.clone(), EndpointSide::Second);
        let client_routes =
            client.commit_connection(connection, EndpointSide::First, empty_client_routes);
        Ok(ConnectCommit::Admitted {
            accept_routes,
            client_routes,
        })
    });

    match result {
        Ok(ConnectCommit::Admitted {
            accept_routes,
            client_routes,
        }) => {
            notify_admission_routes(&accept_routes, "connection admission");
            notify_admission_routes(&client_routes, "client connection commit");
            Ok(())
        },
        Ok(ConnectCommit::Full(listener)) => Err(SocketConnectError::WouldBlock(SocketWait::new(
            AnyOpaque::new(ConnectWaitSource {
                client: client.clone(),
                listener_endpoint,
                listener,
            }),
            poll_connect_wait,
        ))),
        Err(error) => Err(error),
    }
}

enum ConnectCommit {
    Admitted {
        accept_routes: Arc<Vec<UnixPollRoute>>,
        client_routes: Arc<Vec<UnixPollRoute>>,
    },
    Full(Arc<UnixListener>),
}

pub(super) fn accept(
    endpoint: &Arc<UnixEndpointCore>,
) -> Result<SocketAcceptItem, SocketAcceptError> {
    let result = with_admission_commit(|| {
        let listener = match &endpoint.state.lock().association {
            EndpointAssociation::Listening(listener) => listener.clone(),
            EndpointAssociation::Retired => return Err(SocketAcceptError::Retired),
            EndpointAssociation::Unconnected | EndpointAssociation::Connected { .. } => {
                return Err(SocketAcceptError::InvalidState);
            },
        };
        Ok(match listener.pop() {
            Ok((child, connect_routes)) => AcceptCommit::Consumed(child, connect_routes),
            Err(()) => AcceptCommit::Empty(listener),
        })
    })?;
    match result {
        AcceptCommit::Consumed(child, routes) => {
            notify_admission_routes(&routes, "accept consume");
            Ok(SocketAcceptItem {
                peer_address: child.peer_address_snapshot(),
                private: private_from_core(child),
            })
        },
        AcceptCommit::Empty(listener) => Err(SocketAcceptError::WouldBlock(SocketWait::new(
            AnyOpaque::new(AcceptWaitSource {
                listener_endpoint: endpoint.clone(),
                listener,
            }),
            poll_accept_wait,
        ))),
    }
}

enum AcceptCommit {
    Consumed(Arc<UnixEndpointCore>, Arc<Vec<UnixPollRoute>>),
    Empty(Arc<UnixListener>),
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::fs::iomux::PollObserver;

    #[derive(Default)]
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

    fn listening_endpoint(listener: &Arc<UnixListener>) -> Arc<UnixEndpointCore> {
        let endpoint = UnixEndpointCore::new_unconnected();
        endpoint.state.lock().association = EndpointAssociation::Listening(listener.clone());
        endpoint
    }

    fn register_connect_wait(
        client: &Arc<UnixEndpointCore>,
        listener_endpoint: &Arc<UnixEndpointCore>,
        listener: &Arc<UnixListener>,
        route: &PollRoute,
    ) -> (AnyOpaque, PollRegisterResult) {
        let source = AnyOpaque::new(ConnectWaitSource {
            client: client.clone(),
            listener_endpoint: listener_endpoint.clone(),
            listener: listener.clone(),
        });
        let result = poll_connect_wait(
            &source,
            &PollRequest::register_with_route(PollEvent::WRITABLE, route),
        )
        .unwrap();
        (source, result)
    }

    fn connect_wait_snapshot(source: &AnyOpaque) -> PollRegisterResult {
        poll_connect_wait(source, &PollRequest::snapshot(PollEvent::WRITABLE)).unwrap()
    }

    fn register_accept_wait(
        listener_endpoint: &Arc<UnixEndpointCore>,
        listener: &Arc<UnixListener>,
        route: &PollRoute,
    ) -> (AnyOpaque, PollRegisterResult) {
        let source = AnyOpaque::new(AcceptWaitSource {
            listener_endpoint: listener_endpoint.clone(),
            listener: listener.clone(),
        });
        let result = poll_accept_wait(
            &source,
            &PollRequest::register_with_route(PollEvent::READABLE, route),
        )
        .unwrap();
        (source, result)
    }

    fn accept_wait_snapshot(source: &AnyOpaque) -> PollRegisterResult {
        poll_accept_wait(source, &PollRequest::snapshot(PollEvent::READABLE)).unwrap()
    }

    #[kunit]
    fn backlog_normalization_matches_linux_unsigned_clamp() {
        assert_eq!(normalized_backlog(0), 0);
        assert_eq!(normalized_backlog(1), 1);
        assert_eq!(normalized_backlog(i32::MAX), UNIX_LISTENER_MAX_BACKLOG);
        assert_eq!(normalized_backlog(-1), UNIX_LISTENER_MAX_BACKLOG);
    }

    #[kunit]
    fn backlog_zero_admits_one_child_and_pop_restores_capacity() {
        let listener = UnixListener::new(0);
        let first = UnixEndpointCore::new_unconnected();
        let second = UnixEndpointCore::new_unconnected();
        assert!(listener.try_push(first.clone()).is_ok());
        assert!(listener.try_push(second.clone()).is_err());
        assert!(Arc::ptr_eq(&listener.pop().unwrap().0, &first));
        assert!(listener.try_push(second).is_ok());
    }

    #[kunit]
    fn backlog_growth_and_shrink_recheck_the_single_queue_owner() {
        let listener = UnixListener::new(1);
        let first = UnixEndpointCore::new_unconnected();
        let second = UnixEndpointCore::new_unconnected();
        let third = UnixEndpointCore::new_unconnected();

        assert!(listener.try_push(first).is_ok());
        assert!(listener.try_push(second).is_ok());
        assert!(listener.try_push(third.clone()).is_err());

        assert!(listener.update_backlog(0).is_none());
        listener.pop().unwrap();
        assert!(listener.try_push(third.clone()).is_err());

        let routes = listener
            .update_backlog(1)
            .expect("backlog growth must expose the full-to-ready transition");
        assert!(routes.is_empty());
        assert!(listener.try_push(third).is_ok());
    }

    #[kunit]
    fn close_withdraws_both_predicates_and_drains_children_once() {
        let listener = UnixListener::new(1);
        let first = UnixEndpointCore::new_unconnected();
        let second = UnixEndpointCore::new_unconnected();
        assert!(listener.try_push(first.clone()).is_ok());
        assert!(listener.try_push(second.clone()).is_ok());

        let closed = listener.close(Arc::new(Vec::new()), Arc::new(Vec::new()));
        assert_eq!(closed.pending.len(), 2);
        assert!(Arc::ptr_eq(&closed.pending[0], &first));
        assert!(Arc::ptr_eq(&closed.pending[1], &second));
        assert!(
            listener
                .try_push(UnixEndpointCore::new_unconnected())
                .is_err()
        );
        assert!(listener.pop().is_err());
    }

    #[kunit]
    fn listen_after_endpoint_retirement_reports_lifecycle_outcome() {
        let endpoint = UnixEndpointCore::new_unconnected();
        super::super::endpoint::retire_endpoint_core(&endpoint);
        assert_eq!(listen(&endpoint, 0), Err(SocketListenError::Retired));
    }

    #[kunit]
    fn subscribed_connect_wait_observes_client_connection_commit() {
        let listener = UnixListener::new(0);
        listener
            .try_push(UnixEndpointCore::new_unconnected())
            .unwrap();
        let listener_endpoint = listening_endpoint(&listener);
        let client = UnixEndpointCore::new_unconnected();
        let observer = Arc::new(CountingObserver::default());
        let route = route(&observer);
        let (source, registered) =
            register_connect_wait(&client, &listener_endpoint, &listener, &route);
        assert_eq!(
            registered,
            PollRegisterResult::Subscribed(PollEvent::empty())
        );

        let peer = UnixEndpointCore::new_unconnected();
        let connection = UnixConnection::new([client.name.clone(), peer.name.clone()]);
        let routes =
            client.commit_connection(connection, EndpointSide::First, Arc::new(Vec::new()));
        notify_admission_routes(&routes, "KUnit client connection commit");

        assert_eq!(observer.notifications(), 1);
        assert_eq!(
            connect_wait_snapshot(&source),
            PollRegisterResult::Ready(PollEvent::WRITABLE)
        );
    }

    #[kunit]
    fn subscribed_connect_wait_observes_capacity_and_listener_close() {
        let listener = UnixListener::new(0);
        listener
            .try_push(UnixEndpointCore::new_unconnected())
            .unwrap();
        let listener_endpoint = listening_endpoint(&listener);
        let client = UnixEndpointCore::new_unconnected();
        let capacity_observer = Arc::new(CountingObserver::default());
        let capacity_route = route(&capacity_observer);
        let (capacity_source, registered) =
            register_connect_wait(&client, &listener_endpoint, &listener, &capacity_route);
        assert_eq!(
            registered,
            PollRegisterResult::Subscribed(PollEvent::empty())
        );

        let (_, routes) = listener.pop().unwrap();
        notify_admission_routes(&routes, "KUnit accept consume");
        assert_eq!(capacity_observer.notifications(), 1);
        assert_eq!(
            connect_wait_snapshot(&capacity_source),
            PollRegisterResult::Ready(PollEvent::WRITABLE)
        );

        listener
            .try_push(UnixEndpointCore::new_unconnected())
            .unwrap();
        let close_observer = Arc::new(CountingObserver::default());
        let close_route = route(&close_observer);
        let (close_source, registered) =
            register_connect_wait(&client, &listener_endpoint, &listener, &close_route);
        assert_eq!(
            registered,
            PollRegisterResult::Subscribed(PollEvent::empty())
        );

        super::super::endpoint::retire_endpoint_core(&listener_endpoint);
        assert_eq!(close_observer.notifications(), 1);
        assert_eq!(
            connect_wait_snapshot(&close_source),
            PollRegisterResult::Ready(PollEvent::WRITABLE)
        );
    }

    #[kunit]
    fn subscribed_accept_wait_observes_admission_and_listener_close() {
        let listener = UnixListener::new(0);
        let listener_endpoint = listening_endpoint(&listener);
        let observer = Arc::new(CountingObserver::default());
        let admission_route = route(&observer);
        let (source, registered) =
            register_accept_wait(&listener_endpoint, &listener, &admission_route);
        assert_eq!(
            registered,
            PollRegisterResult::Subscribed(PollEvent::empty())
        );

        let routes = listener
            .try_push(UnixEndpointCore::new_unconnected())
            .unwrap();
        notify_admission_routes(&routes, "KUnit connection admission");
        assert_eq!(observer.notifications(), 1);
        assert_eq!(
            accept_wait_snapshot(&source),
            PollRegisterResult::Ready(PollEvent::READABLE)
        );

        let close_listener = UnixListener::new(0);
        let close_endpoint = listening_endpoint(&close_listener);
        let close_observer = Arc::new(CountingObserver::default());
        let close_route = route(&close_observer);
        let (close_source, registered) =
            register_accept_wait(&close_endpoint, &close_listener, &close_route);
        assert_eq!(
            registered,
            PollRegisterResult::Subscribed(PollEvent::empty())
        );

        super::super::endpoint::retire_endpoint_core(&close_endpoint);
        assert_eq!(close_observer.notifications(), 1);
        assert_eq!(
            accept_wait_snapshot(&close_source),
            PollRegisterResult::Ready(PollEvent::READABLE)
        );
    }

    #[kunit]
    fn public_listener_poll_reuses_accept_predicate_and_routes() {
        let listener = UnixListener::new(0);
        let listener_endpoint = listening_endpoint(&listener);
        assert_eq!(
            poll_unix_listener(
                &listener_endpoint,
                &listener,
                &PollRequest::snapshot(PollEvent::READABLE | PollEvent::WRITABLE),
            )
            .unwrap(),
            PollRegisterResult::Ready(PollEvent::empty())
        );

        let observer = Arc::new(CountingObserver::default());
        let route = route(&observer);
        assert_eq!(
            poll_unix_listener(
                &listener_endpoint,
                &listener,
                &PollRequest::register_with_route(PollEvent::READABLE, &route),
            )
            .unwrap(),
            PollRegisterResult::Subscribed(PollEvent::empty())
        );

        let routes = listener
            .try_push(UnixEndpointCore::new_unconnected())
            .unwrap();
        notify_admission_routes(&routes, "KUnit public listener admission");
        assert_eq!(observer.notifications(), 1);
        assert_eq!(
            poll_unix_listener(
                &listener_endpoint,
                &listener,
                &PollRequest::snapshot(PollEvent::READABLE | PollEvent::WRITABLE),
            )
            .unwrap(),
            PollRegisterResult::Ready(PollEvent::READABLE)
        );

        super::super::endpoint::retire_endpoint_core(&listener_endpoint);
        assert_eq!(observer.notifications(), 2);
        assert_eq!(
            poll_unix_listener(
                &listener_endpoint,
                &listener,
                &PollRequest::snapshot(PollEvent::empty()),
            )
            .unwrap(),
            PollRegisterResult::Ready(PollEvent::HANG_UP)
        );
    }

    #[kunit]
    fn terminal_register_returns_ready_without_claiming_subscription() {
        let listener = UnixListener::new(0);
        let listener_endpoint = listening_endpoint(&listener);
        let client = UnixEndpointCore::new_unconnected();
        let peer = UnixEndpointCore::new_unconnected();
        let connection = UnixConnection::new([client.name.clone(), peer.name.clone()]);
        client.commit_connection(connection, EndpointSide::First, Arc::new(Vec::new()));
        let connect_observer = Arc::new(CountingObserver::default());
        let connect_route = route(&connect_observer);
        let (_, connect_result) =
            register_connect_wait(&client, &listener_endpoint, &listener, &connect_route);
        assert_eq!(
            connect_result,
            PollRegisterResult::Ready(PollEvent::WRITABLE)
        );
        assert_eq!(connect_observer.notifications(), 0);

        let accept_listener = UnixListener::new(0);
        let accept_endpoint = listening_endpoint(&accept_listener);
        super::super::endpoint::retire_endpoint_core(&accept_endpoint);
        let accept_observer = Arc::new(CountingObserver::default());
        let accept_route = route(&accept_observer);
        let (_, accept_result) =
            register_accept_wait(&accept_endpoint, &accept_listener, &accept_route);
        assert_eq!(
            accept_result,
            PollRegisterResult::Ready(PollEvent::READABLE)
        );
        assert_eq!(accept_observer.notifications(), 0);
    }
}
