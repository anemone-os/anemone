//! TCP owner-fact projection and weak operation-wait bridge.

use anemone_net_api::tcp::{TcpConnectFact, TcpEndpointFacts, TcpQueryError, TcpReleaseReason};

use crate::{
    net::tcp::{
        EventRegistrationError, TcpEndpointAccessPort, TcpEndpointEventRegistration,
        TcpEndpointInvalidationObserver, TcpEndpointPort,
    },
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

use super::super::{SocketWait, source::SocketPollSource};

struct TcpAssociation {
    endpoint: TcpEndpointPort,
    access: TcpEndpointAccessPort,
    event_registration: TcpEndpointEventRegistration,
}

pub(super) struct TcpSocketSource {
    source: SocketPollSource<TcpAssociation>,
}

#[derive(Clone, Copy)]
enum TcpWaitPredicate {
    Connect,
    Accept,
}

#[derive(Opaque)]
struct TcpWaitSource {
    /// Operation waiters must not retain the Endpoint or opened description.
    source: Weak<TcpSocketSource>,
    predicate: TcpWaitPredicate,
}

impl TcpSocketSource {
    pub(super) fn try_new(
        endpoint: TcpEndpointPort,
    ) -> Result<Arc<Self>, (SysError, TcpEndpointPort)> {
        let poll_source = match SocketPollSource::try_new() {
            Ok(source) => source,
            Err(error) => return Err((error, endpoint)),
        };
        let source = match Arc::try_new(Self {
            source: poll_source,
        }) {
            Ok(source) => source,
            Err(_) => return Err((SysError::OutOfMemory, endpoint)),
        };
        let observer: Arc<dyn TcpEndpointInvalidationObserver> = source.clone();
        let access = endpoint.access();
        let event_registration = match access.register_invalidation_observer(&observer) {
            Ok(registration) => registration,
            Err(EventRegistrationError::OutOfMemory) => {
                drop(observer);
                return Err((SysError::OutOfMemory, endpoint));
            },
        };
        drop(observer);
        source.source.publish(TcpAssociation {
            endpoint,
            access,
            event_registration,
        });
        Ok(source)
    }

    pub(super) fn endpoint(&self) -> Option<TcpEndpointAccessPort> {
        self.source
            .with_live(|association| association.access.clone())
    }

    pub(super) fn poll(&self, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
        self.source.poll(request, |association, interests| {
            Ok(project_public(current_facts(association)?, interests))
        })
    }

    pub(super) fn connect_wait(source: &Arc<Self>) -> SocketWait {
        Self::wait(source, TcpWaitPredicate::Connect)
    }

    pub(super) fn accept_wait(source: &Arc<Self>) -> SocketWait {
        Self::wait(source, TcpWaitPredicate::Accept)
    }

    fn wait(source: &Arc<Self>, predicate: TcpWaitPredicate) -> SocketWait {
        SocketWait::new(
            AnyOpaque::new(TcpWaitSource {
                source: Arc::downgrade(source),
                predicate,
            }),
            poll_tcp_wait,
        )
    }

    pub(super) fn release(&self, reason: TcpReleaseReason) -> bool {
        self.source
            .retire(|association| {
                association.event_registration.unregister();
                association.endpoint.release(reason);
            })
            .is_some()
    }
}

impl TcpEndpointInvalidationObserver for TcpSocketSource {
    fn invalidate(&self) {
        self.source.invalidate();
    }
}

impl Drop for TcpSocketSource {
    fn drop(&mut self) {
        let retired = self.source.retire(|association| {
            association.event_registration.unregister();
            association.endpoint.release(TcpReleaseReason::FinalRelease);
        });
        if retired.is_some() {
            panic!("TCP Socket source dropped before lifecycle-owned retirement");
        }
    }
}

fn poll_tcp_wait(
    private: &AnyOpaque,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    let wait = private
        .cast::<TcpWaitSource>()
        .expect("TCP SocketWait used without its weak source");
    let source = wait.source.upgrade().ok_or(SysError::IdentifierRemoved)?;
    source.source.poll(request, |association, _| {
        let facts = current_facts(association)?;
        Ok(match wait.predicate {
            TcpWaitPredicate::Connect => project_connect_wait(facts),
            TcpWaitPredicate::Accept => project_accept_wait(facts),
        })
    })
}

fn current_facts(association: &TcpAssociation) -> Result<TcpEndpointFacts, SysError> {
    association.access.facts().map_err(|error| match error {
        TcpQueryError::UnknownEndpoint | TcpQueryError::WrongRole => {
            assert!(false, "published TCP source lost its Endpoint facts");
            SysError::IdentifierRemoved
        },
    })
}

fn project_public(facts: TcpEndpointFacts, interests: PollEvent) -> PollEvent {
    match facts {
        TcpEndpointFacts::Idle | TcpEndpointFacts::Bound => {
            PollEvent::HANG_UP | (PollEvent::WRITABLE & interests)
        },
        TcpEndpointFacts::Listener { has_pending_child } => {
            if has_pending_child && interests.contains(PollEvent::READABLE) {
                PollEvent::READABLE
            } else {
                PollEvent::empty()
            }
        },
        TcpEndpointFacts::Connection(connection) => {
            let mut events = PollEvent::empty();
            let pending_error = connection.has_pending_error();
            if pending_error {
                events |= PollEvent::ERROR;
            }
            if connection.is_terminal() {
                events |= PollEvent::HANG_UP;
            }
            if interests.contains(PollEvent::READABLE)
                && (connection.received_bytes() != 0
                    || connection.is_local_read_shutdown()
                    || connection.is_peer_receive_closed()
                    || pending_error
                    || connection.is_terminal())
            {
                events |= PollEvent::READABLE;
            }
            if interests.contains(PollEvent::WRITABLE)
                && (connection.is_local_write_shutdown()
                    || pending_error
                    || connection.is_terminal()
                    || (connection.connect() == TcpConnectFact::Connected
                        && connection.send_capacity() != 0))
            {
                events |= PollEvent::WRITABLE;
            }
            if interests.contains(PollEvent::READ_HANG_UP)
                && (connection.is_local_read_shutdown()
                    || connection.is_peer_receive_closed()
                    || connection.is_terminal())
            {
                events |= PollEvent::READ_HANG_UP;
            }
            events
        },
    }
}

fn project_connect_wait(facts: TcpEndpointFacts) -> PollEvent {
    match facts {
        TcpEndpointFacts::Connection(connection)
            if connection.connect() == TcpConnectFact::Connecting =>
        {
            PollEvent::empty()
        },
        TcpEndpointFacts::Connection(_) => PollEvent::WRITABLE,
        TcpEndpointFacts::Idle | TcpEndpointFacts::Bound | TcpEndpointFacts::Listener { .. } => {
            PollEvent::HANG_UP
        },
    }
}

fn project_accept_wait(facts: TcpEndpointFacts) -> PollEvent {
    match facts {
        TcpEndpointFacts::Listener {
            has_pending_child: true,
        } => PollEvent::READABLE,
        TcpEndpointFacts::Listener {
            has_pending_child: false,
        } => PollEvent::empty(),
        TcpEndpointFacts::Idle | TcpEndpointFacts::Bound | TcpEndpointFacts::Connection(_) => {
            PollEvent::HANG_UP
        },
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    use anemone_net_api::Ipv4Address;

    use crate::{
        fs::iomux::{PollObserver, PollRoute},
        net::tcp::create_endpoint,
    };

    struct CountingObserver(AtomicUsize);

    impl CountingObserver {
        fn new() -> Self {
            Self(AtomicUsize::new(0))
        }

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
    fn projection_matches_unconnected_and_failed_connect_linux_categories() {
        assert_eq!(
            project_public(TcpEndpointFacts::Idle, PollEvent::WRITABLE),
            PollEvent::WRITABLE | PollEvent::HANG_UP
        );
        let failed = anemone_net_api::tcp::TcpConnectionFacts::from_owner_snapshot(
            TcpConnectFact::Failed,
            0,
            0,
            true,
            false,
            false,
            false,
            true,
        );
        assert_eq!(
            project_public(
                TcpEndpointFacts::Connection(failed),
                PollEvent::READABLE | PollEvent::WRITABLE | PollEvent::READ_HANG_UP
            ),
            PollEvent::READABLE
                | PollEvent::WRITABLE
                | PollEvent::ERROR
                | PollEvent::HANG_UP
                | PollEvent::READ_HANG_UP
        );
    }

    #[kunit]
    fn consumed_terminal_error_still_ends_both_io_predicates_without_error() {
        let failed = anemone_net_api::tcp::TcpConnectionFacts::from_owner_snapshot(
            TcpConnectFact::Failed,
            0,
            0,
            false,
            false,
            false,
            false,
            true,
        );
        assert_eq!(
            project_public(
                TcpEndpointFacts::Connection(failed),
                PollEvent::READABLE | PollEvent::WRITABLE | PollEvent::READ_HANG_UP
            ),
            PollEvent::READABLE
                | PollEvent::WRITABLE
                | PollEvent::HANG_UP
                | PollEvent::READ_HANG_UP
        );
    }

    #[kunit]
    fn peer_fin_is_readable_and_requested_read_hang_up_without_full_hang_up() {
        let peer_fin = anemone_net_api::tcp::TcpConnectionFacts::from_owner_snapshot(
            TcpConnectFact::Connected,
            1,
            0,
            false,
            false,
            false,
            true,
            false,
        );
        assert_eq!(
            project_public(
                TcpEndpointFacts::Connection(peer_fin),
                PollEvent::READABLE | PollEvent::READ_HANG_UP
            ),
            PollEvent::READABLE | PollEvent::READ_HANG_UP
        );
    }

    #[kunit]
    fn production_source_rechecks_multiwaiters_and_weak_wait_does_not_retain_lifecycle() {
        let endpoint = create_endpoint().expect("KUnit TCP endpoint must fit");
        let source = TcpSocketSource::try_new(endpoint).expect("KUnit TCP source must fit");
        let first = Arc::new(CountingObserver::new());
        let second = Arc::new(CountingObserver::new());
        for observer in [&first, &second] {
            assert_eq!(
                source
                    .poll(&PollRequest::register_with_route(
                        PollEvent::READABLE,
                        &route(observer),
                    ))
                    .unwrap(),
                PollRegisterResult::Subscribed(PollEvent::HANG_UP)
            );
        }

        source
            .endpoint()
            .expect("KUnit source must be live")
            .bind(Ipv4Address::UNSPECIFIED, 0)
            .expect("KUnit TCP bind must fit");
        assert_eq!(first.notifications(), 1);
        assert_eq!(second.notifications(), 1);

        let wait = TcpSocketSource::accept_wait(&source);
        assert!(source.release(TcpReleaseReason::CreationRollback));
        TcpEndpointInvalidationObserver::invalidate(source.as_ref());
        assert_eq!(first.notifications(), 2);
        assert_eq!(second.notifications(), 2);
        drop(source);
        assert_eq!(
            wait.poll(&PollRequest::snapshot(PollEvent::READABLE)),
            Err(SysError::IdentifierRemoved)
        );
    }
}
