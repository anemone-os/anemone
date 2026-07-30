//! Socket-owned UDP readiness source and Endpoint invalidation handoff.

use anemone_net_api::udp::{UdpEndpointFacts, UdpQueryError, UdpRetireError};

use crate::{
    fs::iomux::{PollObserver, PollRoute},
    net::udp::{
        EventRegistrationError, UdpEndpointEventRegistration, UdpEndpointInvalidationObserver,
        UdpEndpointPort,
    },
    prelude::*,
};

#[derive(Clone, Debug)]
struct UdpPollRoute {
    route: PollRoute,
    interests: PollEvent,
}

impl UdpPollRoute {
    fn new(route: &PollRoute, interests: PollEvent) -> Self {
        Self {
            route: route.clone(),
            interests,
        }
    }
}

struct UdpAssociation {
    endpoint: UdpEndpointPort,
    event_registration: UdpEndpointEventRegistration,
}

enum UdpSourcePublication {
    Unpublished,
    Live {
        /// Sole Socket publication of the Endpoint capability and reverse
        /// route. Endpoint liveness/readiness remain Stack-owned.
        association: UdpAssociation,
        routes: Arc<Vec<UdpPollRoute>>,
    },
    Retired,
}

pub(super) struct UdpSocketSource {
    /// Registration builds a replacement before publication. Invalidation
    /// clones this snapshot while locked; notification and final drop happen
    /// only after the source lock is released.
    publication: SpinLock<UdpSourcePublication>,
}

impl UdpSocketSource {
    pub(super) fn try_new(endpoint: UdpEndpointPort) -> Result<Arc<Self>, SysError> {
        let source = Arc::try_new(Self {
            publication: SpinLock::new(UdpSourcePublication::Unpublished),
        })
        .map_err(|_| SysError::OutOfMemory)?;
        let routes = Arc::try_new(Vec::new()).map_err(|_| SysError::OutOfMemory)?;
        let observer: Arc<dyn UdpEndpointInvalidationObserver> = source.clone();
        let event_registration =
            endpoint
                .register_invalidation_observer(&observer)
                .map_err(|error| match error {
                    EventRegistrationError::OutOfMemory => SysError::OutOfMemory,
                })?;
        drop(observer);

        let previous = core::mem::replace(
            &mut *source.publication.lock(),
            UdpSourcePublication::Live {
                association: UdpAssociation {
                    endpoint,
                    event_registration,
                },
                routes,
            },
        );
        assert!(
            matches!(previous, UdpSourcePublication::Unpublished),
            "fresh UDP source did not begin unpublished"
        );
        Ok(source)
    }

    pub(super) fn endpoint(&self) -> Option<UdpEndpointPort> {
        let publication = self.publication.lock();
        let UdpSourcePublication::Live { association, .. } = &*publication else {
            return None;
        };
        Some(association.endpoint.clone())
    }

    pub(super) fn poll(&self, request: &PollRequest<'_>) -> Result<PollRegisterResult, SysError> {
        let Some(route) = request.route() else {
            let publication = self.publication.lock();
            let UdpSourcePublication::Live { association, .. } = &*publication else {
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
                let UdpSourcePublication::Live { routes, .. } = &*publication else {
                    return Err(SysError::IdentifierRemoved);
                };
                routes.clone()
            };
            let replacement = prepare_route_replacement(&expected, route, request.interests())?;

            let (previous, facts) = {
                let mut publication = self.publication.lock();
                let UdpSourcePublication::Live {
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
                // to a short Stack facts snapshot. Stack transitions route
                // invalidations only after releasing the Stack lock.
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

    pub(super) fn retire(&self) -> Result<(), UdpRetireError> {
        let (association, routes) = {
            let mut publication = self.publication.lock();
            let previous = core::mem::replace(&mut *publication, UdpSourcePublication::Retired);
            match previous {
                UdpSourcePublication::Live {
                    association,
                    routes,
                } => (association, routes),
                UdpSourcePublication::Unpublished | UdpSourcePublication::Retired => {
                    return Err(UdpRetireError::UnknownEndpoint);
                },
            }
        };

        // Publication is already withdrawn. Remove the reverse lookup before
        // waking consumers, then retire the Endpoint without an operation lock.
        association.event_registration.unregister();
        notify_routes(&routes);
        drop(routes);
        association.endpoint.retire()
    }
}

impl UdpEndpointInvalidationObserver for UdpSocketSource {
    fn invalidate(&self) {
        let publication = self.publication.lock();
        let UdpSourcePublication::Live { routes, .. } = &*publication else {
            return;
        };
        let routes = routes.clone();
        drop(publication);
        notify_routes(&routes);
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
    if interests.contains(PollEvent::READABLE) && facts.is_readable() {
        events |= PollEvent::READABLE;
    }
    if interests.contains(PollEvent::WRITABLE) && facts.is_writable() {
        events |= PollEvent::WRITABLE;
    }
    events
}

fn prepare_route_replacement(
    routes: &Arc<Vec<UdpPollRoute>>,
    route: &PollRoute,
    interests: PollEvent,
) -> Result<Arc<Vec<UdpPollRoute>>, SysError> {
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
    replacement.push(UdpPollRoute::new(route, interests));
    Arc::try_new(replacement).map_err(|_| SysError::OutOfMemory)
}

fn notify_routes(routes: &Arc<Vec<UdpPollRoute>>) {
    for entry in routes.iter() {
        if entry
            .interests
            .intersects(PollEvent::READABLE | PollEvent::WRITABLE)
        {
            entry.route.notify();
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    use crate::net::udp::create_endpoint;

    struct CountingObserver {
        notifications: AtomicUsize,
    }

    impl CountingObserver {
        fn new() -> Self {
            Self {
                notifications: AtomicUsize::new(0),
            }
        }

        fn notifications(&self) -> usize {
            self.notifications.load(Ordering::Acquire)
        }
    }

    impl PollObserver for CountingObserver {
        fn notify(&self) {
            self.notifications.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn route(observer: &Arc<CountingObserver>) -> PollRoute {
        let erased: Arc<dyn PollObserver> = observer.clone();
        let route = PollRoute::new(&erased);
        drop(erased);
        route
    }

    #[kunit]
    fn plural_routes_survive_peer_retirement_and_duplicate_hints() {
        let first = Arc::new(CountingObserver::new());
        let second = Arc::new(CountingObserver::new());
        let first_route = route(&first);
        let second_route = route(&second);
        let routes = Arc::new(Vec::new());
        let routes = prepare_route_replacement(&routes, &first_route, PollEvent::READABLE)
            .expect("first route must fit");
        let routes = prepare_route_replacement(&routes, &second_route, PollEvent::WRITABLE)
            .expect("second route must fit");

        notify_routes(&routes);
        notify_routes(&routes);
        assert_eq!(first.notifications(), 2);
        assert_eq!(second.notifications(), 2);

        drop(first);
        let third = Arc::new(CountingObserver::new());
        let third_route = route(&third);
        let replacement = prepare_route_replacement(&routes, &third_route, PollEvent::READABLE)
            .expect("replacement route must fit");
        assert_eq!(replacement.len(), 2);
        notify_routes(&replacement);
        assert_eq!(second.notifications(), 3);
        assert_eq!(third.notifications(), 1);
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
}
