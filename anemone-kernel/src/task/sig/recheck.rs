use core::fmt::Debug;

use crate::{prelude::*, task::ThreadGroup};

/// Source-neutral wake capability for signalfd predicate rechecks.
///
/// Implementations must not own a task, thread group, opened description, poll
/// observer, or active wait lifecycle. A blocking trigger may retain retired
/// wait-token backing storage until the next stale prune; that token cannot
/// reactivate the round or drive readiness.
pub(crate) trait SignalFdRecheckObserver: Debug + Send + Sync {
    fn notify(&self);
    fn is_prunable(&self) -> bool;
}

#[derive(Debug, Clone)]
pub(crate) struct SignalFdRecheckRoute {
    observer: Arc<dyn SignalFdRecheckObserver>,
    /// Poll-observer identity only. Blocking read rounds are never
    /// deduplicated: their wait id is diagnostic and cannot participate in
    /// protocol state.
    poll_hygiene_key: Option<usize>,
}

impl SignalFdRecheckRoute {
    pub(crate) fn for_read(observer: Arc<dyn SignalFdRecheckObserver>) -> Self {
        Self {
            observer,
            poll_hygiene_key: None,
        }
    }

    pub(crate) fn for_poll(
        observer: Arc<dyn SignalFdRecheckObserver>,
        poll_hygiene_key: usize,
    ) -> Self {
        Self {
            observer,
            poll_hygiene_key: Some(poll_hygiene_key),
        }
    }

    fn is_prunable(&self) -> bool {
        self.observer.is_prunable()
    }

    fn notify(&self) {
        self.observer.notify();
    }
}

#[derive(Debug)]
pub(crate) struct SignalFdRecheckRoutes {
    routes: Arc<Vec<SignalFdRecheckRoute>>,
}

impl SignalFdRecheckRoutes {
    pub(crate) fn new() -> Self {
        Self {
            routes: Arc::new(Vec::new()),
        }
    }

    pub(crate) fn replace_with(
        &mut self,
        route: SignalFdRecheckRoute,
    ) -> Result<Arc<Vec<SignalFdRecheckRoute>>, SysError> {
        let replacement_key = route.poll_hygiene_key;
        let retained = self
            .routes
            .iter()
            .filter(|entry| {
                !entry.is_prunable()
                    && match replacement_key {
                        Some(key) => entry.poll_hygiene_key != Some(key),
                        None => true,
                    }
            })
            .count();
        let capacity = retained.checked_add(1).ok_or(SysError::OutOfMemory)?;
        let mut replacement = Vec::new();
        replacement
            .try_reserve(capacity)
            .map_err(|_| SysError::OutOfMemory)?;
        replacement.extend(
            self.routes
                .iter()
                .filter(|entry| {
                    !entry.is_prunable()
                        && match replacement_key {
                            Some(key) => entry.poll_hygiene_key != Some(key),
                            None => true,
                        }
                })
                .cloned(),
        );
        replacement.push(route);
        let replacement = Arc::try_new(replacement).map_err(|_| SysError::OutOfMemory)?;
        Ok(core::mem::replace(&mut self.routes, replacement))
    }

    pub(crate) fn snapshot(&self) -> Arc<Vec<SignalFdRecheckRoute>> {
        self.routes.clone()
    }
}

impl ThreadGroup {
    /// Registers a weak predicate recheck route. The displaced snapshot is
    /// dropped after releasing the IRQ-off registry guard.
    pub(crate) fn register_signalfd_recheck(
        &self,
        route: SignalFdRecheckRoute,
    ) -> Result<(), SysError> {
        let previous = self.signalfd_rechecks.lock().replace_with(route)?;
        drop(previous);
        Ok(())
    }

    pub(crate) fn snapshot_signalfd_rechecks(&self) -> Arc<Vec<SignalFdRecheckRoute>> {
        self.signalfd_rechecks.lock().snapshot()
    }
}

/// Notify only after the producer has published pending state and released all
/// pending/topology guards. Routes carry no readiness or signal identity.
pub(crate) fn notify_signalfd_rechecks(routes: Arc<Vec<SignalFdRecheckRoute>>) {
    for route in routes.iter() {
        route.notify();
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;

    #[derive(Debug)]
    struct TestObserver {
        notifications: AtomicUsize,
        prunable: AtomicBool,
    }

    impl TestObserver {
        fn new(prunable: bool) -> Self {
            Self {
                notifications: AtomicUsize::new(0),
                prunable: AtomicBool::new(prunable),
            }
        }
    }

    impl SignalFdRecheckObserver for TestObserver {
        fn notify(&self) {
            self.notifications.fetch_add(1, Ordering::SeqCst);
        }

        fn is_prunable(&self) -> bool {
            self.prunable.load(Ordering::SeqCst)
        }
    }

    fn read_route(observer: &Arc<TestObserver>) -> SignalFdRecheckRoute {
        let observer: Arc<dyn SignalFdRecheckObserver> = observer.clone();
        SignalFdRecheckRoute::for_read(observer)
    }

    fn poll_route(observer: &Arc<TestObserver>, key: usize) -> SignalFdRecheckRoute {
        let observer: Arc<dyn SignalFdRecheckObserver> = observer.clone();
        SignalFdRecheckRoute::for_poll(observer, key)
    }

    #[kunit]
    fn signalfd_recheck_registration_publishes_before_notification() {
        let observer = Arc::new(TestObserver::new(false));
        let mut routes = SignalFdRecheckRoutes::new();
        drop(routes.replace_with(read_route(&observer)).unwrap());

        let snapshot = routes.snapshot();
        assert_eq!(snapshot.len(), 1);
        notify_signalfd_rechecks(snapshot);
        assert_eq!(observer.notifications.load(Ordering::SeqCst), 1);
    }

    #[kunit]
    fn signalfd_recheck_registration_replaces_duplicate_and_prunes_stale() {
        let duplicate_old = Arc::new(TestObserver::new(false));
        let duplicate_new = Arc::new(TestObserver::new(false));
        let stale = Arc::new(TestObserver::new(true));
        let retained = Arc::new(TestObserver::new(false));
        let mut routes = SignalFdRecheckRoutes::new();

        drop(routes.replace_with(poll_route(&duplicate_old, 7)).unwrap());
        drop(routes.replace_with(poll_route(&stale, 8)).unwrap());
        drop(routes.replace_with(poll_route(&retained, 9)).unwrap());
        drop(routes.replace_with(poll_route(&duplicate_new, 7)).unwrap());

        let snapshot = routes.snapshot();
        assert_eq!(snapshot.len(), 2);
        notify_signalfd_rechecks(snapshot);
        assert_eq!(duplicate_old.notifications.load(Ordering::SeqCst), 0);
        assert_eq!(stale.notifications.load(Ordering::SeqCst), 0);
        assert_eq!(duplicate_new.notifications.load(Ordering::SeqCst), 1);
        assert_eq!(retained.notifications.load(Ordering::SeqCst), 1);
    }

    #[kunit]
    fn signalfd_blocking_read_routes_do_not_use_diagnostic_identity() {
        let first = Arc::new(TestObserver::new(false));
        let second = Arc::new(TestObserver::new(false));
        let mut routes = SignalFdRecheckRoutes::new();

        drop(routes.replace_with(read_route(&first)).unwrap());
        drop(routes.replace_with(read_route(&second)).unwrap());

        let snapshot = routes.snapshot();
        assert_eq!(snapshot.len(), 2);
        notify_signalfd_rechecks(snapshot);
        assert_eq!(first.notifications.load(Ordering::SeqCst), 1);
        assert_eq!(second.notifications.load(Ordering::SeqCst), 1);
    }
}
