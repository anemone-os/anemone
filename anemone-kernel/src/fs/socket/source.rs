//! Socket-owned poll source publication and route handoff.

use crate::{
    fs::iomux::{PollRegisterResult, PollRequest, PollRoute},
    prelude::*,
};

#[derive(Clone, Debug)]
struct SocketPollRoute {
    route: PollRoute,
    interests: PollEvent,
}

impl SocketPollRoute {
    fn new(route: &PollRoute, interests: PollEvent) -> Self {
        Self {
            route: route.clone(),
            interests,
        }
    }
}

enum SocketSourcePublication<A> {
    Unpublished {
        routes: Arc<Vec<SocketPollRoute>>,
    },
    Live {
        /// Opaque family association published through this Socket source.
        /// Family Endpoint lifecycle and readiness remain owner-defined.
        association: A,
        routes: Arc<Vec<SocketPollRoute>>,
    },
    Retired,
}

/// Shared Socket-side source protocol for family-defined readiness.
///
/// The association stays opaque so this type cannot derive or cache protocol
/// facts. The family callback reads its owner-defined predicate while the
/// source lock protects route publication from a lost wakeup.
pub(super) struct SocketPollSource<A> {
    /// Registration builds a replacement before publication. Invalidation
    /// clones this snapshot while locked; notification and final drop happen
    /// only after the source lock is released.
    publication: SpinLock<SocketSourcePublication<A>>,
}

impl<A> SocketPollSource<A> {
    pub(super) fn try_new() -> Result<Self, SysError> {
        let routes = Arc::try_new(Vec::new()).map_err(|_| SysError::OutOfMemory)?;
        Ok(Self {
            publication: SpinLock::new(SocketSourcePublication::Unpublished { routes }),
        })
    }

    pub(super) fn publish(&self, association: A) {
        let mut publication = self.publication.lock();
        assert!(
            matches!(&*publication, SocketSourcePublication::Unpublished { .. }),
            "Socket poll source published outside its unpublished phase"
        );
        let previous = core::mem::replace(&mut *publication, SocketSourcePublication::Retired);
        let SocketSourcePublication::Unpublished { routes } = previous else {
            unreachable!();
        };
        *publication = SocketSourcePublication::Live {
            association,
            routes,
        };
    }

    pub(super) fn with_live<R>(&self, operation: impl FnOnce(&A) -> R) -> Option<R> {
        let publication = self.publication.lock();
        let SocketSourcePublication::Live { association, .. } = &*publication else {
            return None;
        };
        Some(operation(association))
    }

    pub(super) fn poll(
        &self,
        request: &PollRequest<'_>,
        current: impl Fn(&A, PollEvent) -> Result<PollEvent, SysError>,
    ) -> Result<PollRegisterResult, SysError> {
        let Some(route) = request.route() else {
            let publication = self.publication.lock();
            let SocketSourcePublication::Live { association, .. } = &*publication else {
                return Err(SysError::IdentifierRemoved);
            };
            return Ok(PollRegisterResult::Ready(current(
                association,
                request.interests(),
            )?));
        };

        loop {
            let expected = {
                let publication = self.publication.lock();
                let SocketSourcePublication::Live { routes, .. } = &*publication else {
                    return Err(SysError::IdentifierRemoved);
                };
                routes.clone()
            };
            let replacement = prepare_route_replacement(&expected, route, request.interests())?;

            let (previous, readiness) = {
                let mut publication = self.publication.lock();
                let SocketSourcePublication::Live {
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
                // The callback may take the family owner's non-sleeping facts
                // lock. Producers must update those facts and select their
                // invalidation snapshot before notifying outside that lock.
                let readiness = current(association, request.interests());
                (previous, readiness)
            };
            drop(previous);
            return Ok(PollRegisterResult::Subscribed(readiness?));
        }
    }

    pub(super) fn invalidate(&self) {
        let publication = self.publication.lock();
        let SocketSourcePublication::Live { routes, .. } = &*publication else {
            return;
        };
        let routes = routes.clone();
        drop(publication);
        notify_interested_routes(&routes);
    }

    /// Withdraws source publication, then consumes the family's reverse
    /// registration before waking every detached route. The callback result
    /// lets the family complete its owner-local Endpoint retire.
    pub(super) fn retire<R>(&self, unregister: impl FnOnce(A) -> R) -> Option<R> {
        let (association, routes) = {
            let mut publication = self.publication.lock();
            let previous = core::mem::replace(&mut *publication, SocketSourcePublication::Retired);
            match previous {
                SocketSourcePublication::Live {
                    association,
                    routes,
                } => (association, routes),
                SocketSourcePublication::Unpublished { .. } | SocketSourcePublication::Retired => {
                    return None;
                },
            }
        };

        let result = unregister(association);
        notify_all_routes(&routes);
        drop(routes);
        Some(result)
    }
}

fn prepare_route_replacement(
    routes: &Arc<Vec<SocketPollRoute>>,
    route: &PollRoute,
    interests: PollEvent,
) -> Result<Arc<Vec<SocketPollRoute>>, SysError> {
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
    replacement.push(SocketPollRoute::new(route, interests));
    Arc::try_new(replacement).map_err(|_| SysError::OutOfMemory)
}

fn notify_interested_routes(routes: &Arc<Vec<SocketPollRoute>>) {
    for entry in routes.iter() {
        if entry
            .interests
            .intersects(PollEvent::READABLE | PollEvent::WRITABLE)
        {
            entry.route.notify();
        }
    }
}

fn notify_all_routes(routes: &Arc<Vec<SocketPollRoute>>) {
    for entry in routes.iter() {
        entry.route.notify();
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    use crate::fs::iomux::PollObserver;

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

    fn no_readiness(_: &(), _: PollEvent) -> Result<PollEvent, SysError> {
        Ok(PollEvent::empty())
    }

    #[kunit]
    fn invalidation_filters_interests_and_prunes_retired_routes() {
        let source = SocketPollSource::try_new().expect("KUnit source routes must fit");
        source.publish(());
        let first = Arc::new(CountingObserver::new());
        let second = Arc::new(CountingObserver::new());
        let first_route = route(&first);
        let second_route = route(&second);

        source
            .poll(
                &PollRequest::register_with_route(PollEvent::READABLE, &first_route),
                no_readiness,
            )
            .unwrap();
        source
            .poll(
                &PollRequest::register_with_route(PollEvent::WRITABLE, &second_route),
                no_readiness,
            )
            .unwrap();
        source.invalidate();
        source.invalidate();
        assert_eq!(first.notifications(), 2);
        assert_eq!(second.notifications(), 2);

        drop(first);
        let third = Arc::new(CountingObserver::new());
        let third_route = route(&third);
        source
            .poll(
                &PollRequest::register_with_route(PollEvent::READABLE, &third_route),
                no_readiness,
            )
            .unwrap();
        let publication = source.publication.lock();
        let SocketSourcePublication::Live { routes, .. } = &*publication else {
            panic!("KUnit source lost live publication");
        };
        assert_eq!(routes.len(), 2);
        drop(publication);

        source.invalidate();
        assert_eq!(second.notifications(), 3);
        assert_eq!(third.notifications(), 1);
    }

    #[kunit]
    fn retire_withdraws_before_unregister_and_wakes_every_route() {
        let source = SocketPollSource::try_new().expect("KUnit source routes must fit");
        source.publish(());
        let readable = Arc::new(CountingObserver::new());
        let empty = Arc::new(CountingObserver::new());
        let hang_up = Arc::new(CountingObserver::new());

        for (interests, observer) in [
            (PollEvent::READABLE, &readable),
            (PollEvent::empty(), &empty),
            (PollEvent::HANG_UP, &hang_up),
        ] {
            source
                .poll(
                    &PollRequest::register_with_route(interests, &route(observer)),
                    no_readiness,
                )
                .unwrap();
        }

        source
            .retire(|_| {
                assert_eq!(
                    source.poll(&PollRequest::snapshot(PollEvent::READABLE), no_readiness),
                    Err(SysError::IdentifierRemoved)
                );
                assert_eq!(readable.notifications(), 0);
                assert_eq!(empty.notifications(), 0);
                assert_eq!(hang_up.notifications(), 0);
            })
            .expect("KUnit source must retain its association");
        assert_eq!(readable.notifications(), 1);
        assert_eq!(empty.notifications(), 1);
        assert_eq!(hang_up.notifications(), 1);

        source.invalidate();
        assert_eq!(readable.notifications(), 1);
        assert_eq!(empty.notifications(), 1);
        assert_eq!(hang_up.notifications(), 1);
    }
}
