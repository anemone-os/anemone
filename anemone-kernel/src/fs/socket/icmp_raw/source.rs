//! Socket-owned ICMP raw readiness source and Endpoint invalidation handoff.

use anemone_net_api::icmp_raw::{IcmpRawEndpointFacts, IcmpRawQueryError, IcmpRawRetireError};

use crate::{
    fs::iomux::PollRoute,
    net::icmp_raw::{
        EventRegistrationError, IcmpRawEndpointEventRegistration,
        IcmpRawEndpointInvalidationObserver, IcmpRawEndpointPort,
    },
    prelude::*,
};

#[derive(Clone, Debug)]
struct IcmpRawPollRoute {
    route: PollRoute,
    interests: PollEvent,
}

impl IcmpRawPollRoute {
    fn new(route: &PollRoute, interests: PollEvent) -> Self {
        Self {
            route: route.clone(),
            interests,
        }
    }
}

struct IcmpRawAssociation {
    endpoint: IcmpRawEndpointPort,
    event_registration: IcmpRawEndpointEventRegistration,
}

enum IcmpRawSourcePublication {
    Unpublished,
    Live {
        /// Sole Socket publication of the Endpoint capability and reverse
        /// route. Endpoint association and readiness remain Stack-owned.
        association: IcmpRawAssociation,
        routes: Arc<Vec<IcmpRawPollRoute>>,
    },
    Retired,
}

pub(super) struct IcmpRawSocketSource {
    /// Registration builds a replacement before publication. Invalidation
    /// clones this snapshot while locked; notification and final drop happen
    /// only after the source lock is released.
    publication: SpinLock<IcmpRawSourcePublication>,
}

impl IcmpRawSocketSource {
    pub(super) fn try_new(endpoint: IcmpRawEndpointPort) -> Result<Arc<Self>, SysError> {
        let source = Arc::try_new(Self {
            publication: SpinLock::new(IcmpRawSourcePublication::Unpublished),
        })
        .map_err(|_| SysError::OutOfMemory)?;
        let routes = Arc::try_new(Vec::new()).map_err(|_| SysError::OutOfMemory)?;
        let observer: Arc<dyn IcmpRawEndpointInvalidationObserver> = source.clone();
        let event_registration =
            endpoint
                .register_invalidation_observer(&observer)
                .map_err(|error| match error {
                    EventRegistrationError::OutOfMemory => SysError::OutOfMemory,
                })?;
        drop(observer);

        let previous = core::mem::replace(
            &mut *source.publication.lock(),
            IcmpRawSourcePublication::Live {
                association: IcmpRawAssociation {
                    endpoint,
                    event_registration,
                },
                routes,
            },
        );
        assert!(
            matches!(previous, IcmpRawSourcePublication::Unpublished),
            "fresh ICMP raw source did not begin unpublished"
        );
        Ok(source)
    }

    pub(super) fn endpoint(&self) -> Option<IcmpRawEndpointPort> {
        let publication = self.publication.lock();
        let IcmpRawSourcePublication::Live { association, .. } = &*publication else {
            return None;
        };
        Some(association.endpoint.clone())
    }

    pub(super) fn poll(&self, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
        let Some(route) = request.route() else {
            let publication = self.publication.lock();
            let IcmpRawSourcePublication::Live { association, .. } = &*publication else {
                return Err(SysError::IdentifierRemoved);
            };
            let facts = current_facts(association)?;
            return Ok(PollRegisterResult::Ready(project_facts(
                facts,
                request.interests(),
            )));
        };

        loop {
            let expected = {
                let publication = self.publication.lock();
                let IcmpRawSourcePublication::Live { routes, .. } = &*publication else {
                    return Err(SysError::IdentifierRemoved);
                };
                routes.clone()
            };
            let replacement = prepare_route_replacement(&expected, route, request.interests())?;

            let (previous, facts) = {
                let mut publication = self.publication.lock();
                let IcmpRawSourcePublication::Live {
                    association,
                    routes,
                } = &mut *publication
                else {
                    return Err(SysError::IdentifierRemoved);
                };
                if !Arc::ptr_eq(routes, &expected) {
                    continue;
                }
                let previous = core::mem::replace(routes, replacement);
                // This is the only permitted nested order: source publication
                // to a short Stack facts snapshot. Stack routes invalidations
                // only after releasing its owner lock.
                let facts = current_facts(association)?;
                (previous, facts)
            };
            drop(previous);
            return Ok(PollRegisterResult::Subscribed(project_facts(
                facts,
                request.interests(),
            )));
        }
    }

    pub(super) fn retire(&self) -> Result<(), IcmpRawRetireError> {
        let (association, routes) = {
            let mut publication = self.publication.lock();
            let previous = core::mem::replace(&mut *publication, IcmpRawSourcePublication::Retired);
            match previous {
                IcmpRawSourcePublication::Live {
                    association,
                    routes,
                } => (association, routes),
                IcmpRawSourcePublication::Unpublished | IcmpRawSourcePublication::Retired => {
                    return Err(IcmpRawRetireError::UnknownEndpoint);
                },
            }
        };

        // Withdraw publication and reverse lookup before wake delivery. Every
        // consumer then observes retirement in its final predicate scan.
        association.event_registration.unregister();
        notify_all_routes(&routes);
        drop(routes);
        association.endpoint.retire()
    }
}

impl IcmpRawEndpointInvalidationObserver for IcmpRawSocketSource {
    fn invalidate(&self) {
        let publication = self.publication.lock();
        let IcmpRawSourcePublication::Live { routes, .. } = &*publication else {
            return;
        };
        let routes = routes.clone();
        drop(publication);
        notify_interested_routes(&routes);
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

fn prepare_route_replacement(
    routes: &Arc<Vec<IcmpRawPollRoute>>,
    route: &PollRoute,
    interests: PollEvent,
) -> Result<Arc<Vec<IcmpRawPollRoute>>, SysError> {
    let retained = routes
        .iter()
        .filter(|entry| !entry.route.is_prunable())
        .count();
    let capacity = retained.checked_add(1).ok_or(SysError::OutOfMemory)?;
    let mut replacement = Vec::new();
    replacement
        .try_reserve(capacity)
        .map_err(|_| SysError::OutOfMemory)?;
    replacement.extend(
        routes
            .iter()
            .filter(|entry| !entry.route.is_prunable())
            .cloned(),
    );
    replacement.push(IcmpRawPollRoute::new(route, interests));
    Arc::try_new(replacement).map_err(|_| SysError::OutOfMemory)
}

fn notify_interested_routes(routes: &Arc<Vec<IcmpRawPollRoute>>) {
    for entry in routes.iter() {
        if entry
            .interests
            .intersects(PollEvent::READABLE | PollEvent::WRITABLE)
        {
            entry.route.notify();
        }
    }
}

fn notify_all_routes(routes: &Arc<Vec<IcmpRawPollRoute>>) {
    for entry in routes.iter() {
        entry.route.notify();
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    use anemone_net_api::{Ipv4Address, icmp_raw::IcmpRawEgressPolicy};

    use crate::{
        fs::iomux::PollObserver, kconfig_defs::NET_ICMP_RAW_DEFAULT_TTL,
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
