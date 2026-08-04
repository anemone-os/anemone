//! Socket-owned ICMP raw readiness projection and Endpoint invalidation bridge.

use anemone_net_api::icmp_raw::{IcmpRawEndpointFacts, IcmpRawQueryError, IcmpRawRetireError};

use crate::{
    net::icmp_raw::{
        EventRegistrationError, IcmpRawEndpointEventRegistration,
        IcmpRawEndpointInvalidationObserver, IcmpRawEndpointPort,
    },
    prelude::*,
};

use super::super::source::SocketPollSource;

struct IcmpRawAssociation {
    endpoint: IcmpRawEndpointPort,
    event_registration: IcmpRawEndpointEventRegistration,
}

pub(super) struct IcmpRawSocketSource {
    source: SocketPollSource<IcmpRawAssociation>,
}

impl IcmpRawSocketSource {
    pub(super) fn try_new(endpoint: IcmpRawEndpointPort) -> Result<Arc<Self>, SysError> {
        let source = Arc::try_new(Self {
            source: SocketPollSource::try_new()?,
        })
        .map_err(|_| SysError::OutOfMemory)?;
        let observer: Arc<dyn IcmpRawEndpointInvalidationObserver> = source.clone();
        let event_registration =
            endpoint
                .register_invalidation_observer(&observer)
                .map_err(|error| match error {
                    EventRegistrationError::OutOfMemory => SysError::OutOfMemory,
                })?;
        drop(observer);

        source.source.publish(IcmpRawAssociation {
            endpoint,
            event_registration,
        });
        Ok(source)
    }

    pub(super) fn endpoint(&self) -> Option<IcmpRawEndpointPort> {
        self.source
            .with_live(|association| association.endpoint.clone())
    }

    pub(super) fn poll(&self, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
        self.source.poll(request, |association, interests| {
            Ok(project_facts(current_facts(association)?, interests))
        })
    }

    pub(super) fn retire(&self) -> Result<(), IcmpRawRetireError> {
        let endpoint = self
            .source
            .retire(|association| {
                association.event_registration.unregister();
                association.endpoint
            })
            .ok_or(IcmpRawRetireError::UnknownEndpoint)?;
        endpoint.retire()
    }
}

impl IcmpRawEndpointInvalidationObserver for IcmpRawSocketSource {
    fn invalidate(&self) {
        self.source.invalidate();
    }
}

fn current_facts(association: &IcmpRawAssociation) -> Result<IcmpRawEndpointFacts, SysError> {
    association.endpoint.facts().map_err(|error| match error {
        IcmpRawQueryError::UnknownEndpoint => {
            assert!(false, "published ICMP raw source lost its Endpoint");
            SysError::IdentifierRemoved
        },
    })
}

fn project_facts(facts: IcmpRawEndpointFacts, interests: PollEvent) -> PollEvent {
    if !facts.is_live() {
        return PollEvent::empty();
    }
    let mut events = PollEvent::empty();
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

    use anemone_net_api::{Ipv4Address, icmp_raw::IcmpRawEgressPolicy};

    use crate::{
        fs::iomux::{PollObserver, PollRoute},
        kconfig_defs::NET_ICMP_RAW_DEFAULT_TTL,
        net::icmp_raw::create_endpoint,
    };

    struct CountingObserver(AtomicUsize);

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
    fn register_rechecks_owner_facts_and_retire_isolates_late_hints() {
        let endpoint = create_endpoint().expect("KUnit ICMP raw endpoint must fit");
        let source =
            IcmpRawSocketSource::try_new(endpoint.clone()).expect("KUnit raw source must fit");
        let observer = Arc::new(CountingObserver(AtomicUsize::new(0)));
        let route = route(&observer);
        assert_eq!(
            source
                .poll(&PollRequest::register_with_route(
                    PollEvent::WRITABLE,
                    &route,
                ))
                .unwrap(),
            PollRegisterResult::Subscribed(PollEvent::WRITABLE)
        );
        let destination = Ipv4Address::LOOPBACK;
        let selection = endpoint
            .prepare_send(destination)
            .expect("KUnit loopback selection must exist");
        endpoint
            .send_prepared(
                &selection,
                destination,
                IcmpRawEgressPolicy::new(NET_ICMP_RAW_DEFAULT_TTL, 0).unwrap(),
                &[],
            )
            .expect("KUnit raw queue must admit one packet");
        let before_retire = observer.0.load(Ordering::Acquire);
        assert!(
            before_retire > 0,
            "DomainStack transition did not reach the Socket source route"
        );

        source
            .retire()
            .expect("KUnit raw source must retain its Endpoint");
        let after_retire = observer.0.load(Ordering::Acquire);
        assert!(after_retire > before_retire);
        IcmpRawEndpointInvalidationObserver::invalidate(source.as_ref());
        assert_eq!(observer.0.load(Ordering::Acquire), after_retire);
        assert_eq!(
            source.poll(&PollRequest::snapshot(PollEvent::WRITABLE)),
            Err(SysError::IdentifierRemoved)
        );
    }
}
