//! Kernel-side network attach authority.

#[cfg(all(feature = "kunit", target_arch = "riscv64"))]
mod validation;
mod worker;

use anemone_net_api::InterfaceId;

use crate::{device::net::NetdevSnapshot, driver::net::take_published_netdevs, prelude::*};

use worker::{AttachFailure, PumpControl};

struct ActivePath {
    snapshot: NetdevSnapshot,
    /// Diagnostic-only projection of the stack-owned mapping identity. It does
    /// not authorize protocol access or drive attach/worker decisions.
    interface: InterfaceId,
    control: Arc<PumpControl>,
}

/// Active publication owner. Entries contain no provider backing, queue truth,
/// smoltcp handle, task, timer, or IRQ object.
static ACTIVE_PATHS: Lazy<SpinLock<Vec<ActivePath>>> = Lazy::new(|| SpinLock::new(Vec::new()));

#[initcall(late)]
fn attach_published_netdevs() {
    for published in take_published_netdevs() {
        match worker::prepare(published) {
            Ok(prepared) => {
                let snapshot = prepared.snapshot().clone();
                let interface = prepared.interface();
                let mut paths = ACTIVE_PATHS.lock();
                paths.push(ActivePath {
                    snapshot: snapshot.clone(),
                    interface,
                    control: prepared.control(),
                });
                // Readers cannot observe the registry entry until its worker
                // predicate is active; preparation has already completed all
                // mapping, worker, IRQ-wake, and time wiring.
                prepared.activate();
                drop(paths);
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
