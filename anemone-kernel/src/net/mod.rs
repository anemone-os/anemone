//! Kernel-side network attach authority.

mod worker;

use anemone_net_api::InterfaceId;

use crate::{device::net::NetdevSnapshot, driver::net::virtio::take_published_netdevs, prelude::*};

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

#[cfg(all(feature = "kunit", target_arch = "riscv64"))]
#[kunit]
fn rv64_virtio_net_vertical_slice() {
    let (snapshot, interface, control) = {
        let paths = ACTIVE_PATHS.lock();
        assert_eq!(
            paths.len(),
            1,
            "RV64 Stage 1 validation requires exactly one active network path"
        );
        let path = &paths[0];
        (path.snapshot.clone(), path.interface, path.control.clone())
    };

    control.request_kunit_probe();
    let stats = control
        .wait_for_kunit_probe(Duration::from_secs(5))
        .expect("completed network KUnit probe must publish diagnostic counters");
    assert!(
        stats.tx_submissions() > 0,
        "ICMP path submitted no VirtIO TX"
    );
    assert!(
        stats.tx_completions() > 0,
        "ICMP path observed no VirtIO TX completion"
    );
    assert!(
        stats.irq_rechecks() > 0,
        "ICMP path observed no IRQ recheck"
    );
    assert!(
        stats.rx_completions() > 0,
        "ICMP path observed no VirtIO RX completion"
    );
    assert!(
        stats.mapping_high_water() >= VIRTIO_NET_QUEUE_SIZE / 2,
        "initial VirtIO RX mappings were not reflected in the high-water mark"
    );
    kprintln!(
        "net-frame probe complete: active path {} (ifindex {}, {:?}); RX completion {}, TX submit/completion {}/{}, IRQ recheck {}, queue-full {}, live/high-water mappings {}/{}",
        snapshot.name(),
        snapshot.ifindex(),
        interface,
        stats.rx_completions(),
        stats.tx_submissions(),
        stats.tx_completions(),
        stats.irq_rechecks(),
        stats.queue_full(),
        stats.live_mappings(),
        stats.mapping_high_water(),
    );
    kinfoln!(
        "RV64 net-frame vertical slice passed for {} (ifindex {}, {:?})",
        snapshot.name(),
        snapshot.ifindex(),
        interface,
    );
}
