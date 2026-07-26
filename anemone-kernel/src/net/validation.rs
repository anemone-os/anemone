use crate::{driver::net::stage1_probe_stats, prelude::*};

use super::ACTIVE_PATHS;

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
    control.wait_for_kunit_probe(Duration::from_secs(5));
    let stats =
        stage1_probe_stats().expect("completed network KUnit probe must expose driver diagnostics");
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
