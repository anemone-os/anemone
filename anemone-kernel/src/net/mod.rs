//! Kernel-side network attach authority.

mod worker;

use crate::{device::net::NetdevSnapshot, driver::net::take_published_netdevs, prelude::*};

use worker::{AttachFailure, PumpControl};

struct ActivePath {
    snapshot: NetdevSnapshot,
    control: Arc<PumpControl>,
}

struct AttachAuthority {
    active_paths: Vec<ActivePath>,
    /// Sole admission truth for terminal network shutdown. Per-path `active`
    /// bits are capability-local projections and cannot reopen this gate.
    shutdown_started: bool,
}

impl AttachAuthority {
    const fn new() -> Self {
        Self {
            active_paths: Vec::new(),
            shutdown_started: false,
        }
    }
}

/// Active publication and terminal shutdown-admission owner. Entries contain
/// no provider backing, queue truth, smoltcp handle, task, timer, or IRQ
/// object.
static ACTIVE_PATHS: Lazy<SpinLock<AttachAuthority>> =
    Lazy::new(|| SpinLock::new(AttachAuthority::new()));

#[initcall(late)]
fn attach_published_netdevs() {
    for published in take_published_netdevs() {
        match worker::prepare(published) {
            Ok(prepared) => {
                let snapshot = prepared.snapshot().clone();
                let interface = prepared.interface();
                let mut authority = ACTIVE_PATHS.lock();
                if authority.shutdown_started {
                    drop(authority);
                    prepared.stop_and_retain();
                    kerrln!(
                        "network shutdown already started; leaving {} (ifindex {}) published/unattached",
                        snapshot.name(),
                        snapshot.ifindex(),
                    );
                    continue;
                }
                authority.active_paths.push(ActivePath {
                    snapshot: snapshot.clone(),
                    control: prepared.control(),
                });
                // Readers cannot observe the registry entry until its worker
                // predicate is active; preparation has already completed all
                // mapping, worker, IRQ-wake, and time wiring.
                prepared.activate();
                drop(authority);
                kinfoln!(
                    "network path {} (ifindex {}) active as {:?}",
                    snapshot.name(),
                    snapshot.ifindex(),
                    interface,
                );
            },
            Err(AttachFailure::MissingEthernetAddress) => {
                kerrln!("published network device has no Ethernet address; leaving it unattached");
            },
            Err(AttachFailure::WorkerSpawn(error)) => {
                kerrln!(
                    "failed to spawn network pump worker: {:?}; leaving netdev published/unattached",
                    error
                );
            },
        }
    }
}

/// Close network attach admission and request one non-waiting stop attempt for
/// every active path. System Power owns the surrounding global order.
pub(crate) unsafe fn shutdown() {
    let paths = {
        let mut authority = ACTIVE_PATHS.lock();
        assert!(
            !authority.shutdown_started,
            "network shutdown admission closed more than once"
        );
        authority.shutdown_started = true;
        authority
            .active_paths
            .iter()
            .map(|path| (path.snapshot.clone(), path.control.clone()))
            .collect::<Vec<_>>()
    };

    for (snapshot, control) in paths {
        control.request_shutdown();
        kemergln!(
            "network path {} (ifindex {}) shutdown: admission closed, stop requested; terminal resources retained",
            snapshot.name(),
            snapshot.ifindex(),
        );
    }
}
