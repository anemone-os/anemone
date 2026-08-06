//! Socket-owned UDP readiness projection and Endpoint invalidation bridge.

use anemone_net_api::udp::{UdpEndpointFacts, UdpQueryError, UdpRetireError};

use crate::{
    net::udp::{
        EventRegistrationError, UdpEndpointEventRegistration, UdpEndpointInvalidationObserver,
        UdpEndpointPort,
    },
    prelude::*,
};

use super::super::source::SocketPollSource;

struct UdpAssociation {
    endpoint: UdpEndpointPort,
    event_registration: UdpEndpointEventRegistration,
}

pub(super) struct UdpSocketSource {
    source: SocketPollSource<UdpAssociation>,
}

impl UdpSocketSource {
    pub(super) fn try_new(endpoint: UdpEndpointPort) -> Result<Arc<Self>, SysError> {
        let source = Arc::try_new(Self {
            source: SocketPollSource::try_new()?,
        })
        .map_err(|_| SysError::OutOfMemory)?;
        let observer: Arc<dyn UdpEndpointInvalidationObserver> = source.clone();
        let event_registration =
            endpoint
                .register_invalidation_observer(&observer)
                .map_err(|error| match error {
                    EventRegistrationError::OutOfMemory => SysError::OutOfMemory,
                })?;
        drop(observer);

        source.source.publish(UdpAssociation {
            endpoint,
            event_registration,
        });
        Ok(source)
    }

    pub(super) fn endpoint(&self) -> Option<UdpEndpointPort> {
        self.source
            .with_live(|association| association.endpoint.clone())
    }

    pub(super) fn poll(&self, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
        self.source.poll(request, |association, interests| {
            Ok(project_facts(current_facts(association)?, interests))
        })
    }

    pub(super) fn retire(&self) -> Result<(), UdpRetireError> {
        let endpoint = self
            .source
            .retire(|association| {
                association.event_registration.unregister();
                association.endpoint
            })
            .ok_or(UdpRetireError::UnknownEndpoint)?;
        endpoint.retire()
    }
}

impl UdpEndpointInvalidationObserver for UdpSocketSource {
    fn invalidate(&self) {
        self.source.invalidate();
    }
}

fn current_facts(association: &UdpAssociation) -> Result<UdpEndpointFacts, SysError> {
    association.endpoint.facts().map_err(|error| match error {
        UdpQueryError::UnknownEndpoint => {
            assert!(false, "published UDP source lost its Endpoint");
            SysError::IdentifierRemoved
        },
    })
}

fn project_facts(facts: UdpEndpointFacts, interests: PollEvent) -> PollEvent {
    if !facts.is_live() {
        return PollEvent::empty();
    }
    let mut events = PollEvent::empty();
    // ERROR is a mandatory Linux poll result. It projects the current
    // Endpoint-owned pending/FIFO fact even when the caller did not request
    // it; invalidation remains only a hint to rerun this snapshot.
    if facts.has_error() {
        events |= PollEvent::ERROR;
    }
    if interests.contains(PollEvent::READABLE) && facts.is_readable() {
        events |= PollEvent::READABLE;
    }
    if interests.contains(PollEvent::WRITABLE) && facts.is_writable() {
        events |= PollEvent::WRITABLE;
    }
    events
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    use crate::{
        fs::iomux::{PollObserver, PollRoute},
        net::udp::create_endpoint,
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
    fn ready_at_register_keeps_route_for_later_invalidation() {
        let endpoint = create_endpoint().expect("KUnit UDP endpoint must fit");
        let source = UdpSocketSource::try_new(endpoint).expect("KUnit UDP source must fit");
        let observer = Arc::new(CountingObserver::new());
        let route = route(&observer);
        let request = PollRequest::register_with_route(PollEvent::WRITABLE, &route);

        assert_eq!(
            source.poll(&request).unwrap(),
            PollRegisterResult::Subscribed(PollEvent::WRITABLE)
        );
        UdpEndpointInvalidationObserver::invalidate(source.as_ref());
        assert_eq!(observer.notifications(), 1);

        source
            .retire()
            .expect("KUnit UDP source must retain its Endpoint");
    }

    #[kunit]
    fn empty_interest_registers_for_later_mandatory_error() {
        let endpoint = create_endpoint().expect("KUnit UDP endpoint must fit");
        let source = UdpSocketSource::try_new(endpoint).expect("KUnit UDP source must fit");
        let observer = Arc::new(CountingObserver::new());
        let route = route(&observer);
        let request = PollRequest::register_with_route(PollEvent::empty(), &route);

        assert_eq!(
            source.poll(&request).unwrap(),
            PollRegisterResult::Subscribed(PollEvent::empty())
        );
        UdpEndpointInvalidationObserver::invalidate(source.as_ref());
        assert_eq!(observer.notifications(), 1);

        source
            .retire()
            .expect("KUnit UDP source must retain its Endpoint");
    }

    #[kunit]
    fn error_projection_is_mandatory_and_clears_with_owner_fact() {
        let error = UdpEndpointFacts::from_owner_snapshot(false, true, true);
        assert_eq!(project_facts(error, PollEvent::empty()), PollEvent::ERROR);
        let clear = UdpEndpointFacts::from_owner_snapshot(false, true, false);
        assert_eq!(project_facts(clear, PollEvent::empty()), PollEvent::empty());
    }
}
