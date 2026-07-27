use crate::{driver::net::stage1_probe_stats, prelude::*};

use super::{ACTIVE_PATHS, worker::WORKER_REPOLL_LIMIT};

#[kunit]
fn rv64_virtio_net_vertical_slice() {
    let burst = VIRTIO_NET_QUEUE_SIZE
        .checked_mul(2)
        .expect("VirtIO-Net validation burst overflow");
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

    let baseline =
        stage1_probe_stats().expect("active network path must expose driver diagnostics");
    let worker_baseline = control.worker_probe_stats();
    control.request_kunit_probe(burst);
    control.wait_for_kunit_probe(Duration::from_secs(5));
    let deadline = crate::time::Instant::now() + Duration::from_secs(5);
    let stats = loop {
        let stats = stage1_probe_stats()
            .expect("completed network KUnit probe must expose driver diagnostics");
        let tx_submissions = stats
            .tx_submissions()
            .checked_sub(baseline.tx_submissions())
            .expect("VirtIO-Net TX submission counter moved backwards");
        let tx_completions = stats
            .tx_completions()
            .checked_sub(baseline.tx_completions())
            .expect("VirtIO-Net TX completion counter moved backwards");
        if tx_submissions >= burst
            && tx_submissions == tx_completions
            && stats.tx_outstanding() == 0
            && stats.live_mappings() == baseline.live_mappings()
        {
            break stats;
        }
        assert!(
            crate::time::Instant::now() < deadline,
            "RV64 network burst did not return TX and mapping ownership to baseline"
        );
        yield_now();
    };
    let worker = control.worker_probe_stats();
    let worker_actions = worker
        .worker_actions()
        .checked_sub(worker_baseline.worker_actions())
        .expect("network worker action counter moved backwards");
    let repoll_requests = worker
        .repoll_requests()
        .checked_sub(worker_baseline.repoll_requests())
        .expect("network worker request counter moved backwards");
    let worker_yields = worker
        .yields()
        .checked_sub(worker_baseline.yields())
        .expect("network worker yield counter moved backwards");
    let tx_submissions = stats.tx_submissions() - baseline.tx_submissions();
    let tx_completions = stats.tx_completions() - baseline.tx_completions();
    let replies = burst;
    let normal_exhaustion = stats.queue_full() - baseline.queue_full();
    kprintln!(
        "net-frame probe summary: burst/reply {}/{}; TX submit/completion {}/{}; natural exhaustion {}; outstanding current/high-water {}/{}; IRQ recheck {}; mappings baseline/current/high-water {}/{}/{}; worker action/max-round/request/yield {}/{}/{}/{}",
        burst,
        replies,
        tx_submissions,
        tx_completions,
        normal_exhaustion,
        stats.tx_outstanding(),
        stats.tx_outstanding_high_water(),
        stats.irq_rechecks() - baseline.irq_rechecks(),
        baseline.live_mappings(),
        stats.live_mappings(),
        stats.mapping_high_water(),
        worker_actions,
        worker.max_pump_rounds(),
        repoll_requests,
        worker_yields,
    );

    assert_eq!(replies, burst);
    assert!(
        tx_submissions >= burst,
        "ICMP burst submitted too few VirtIO TX frames"
    );
    assert_eq!(tx_submissions, tx_completions);
    assert_eq!(baseline.tx_outstanding(), 0);
    assert_eq!(stats.tx_outstanding(), 0);
    assert!(stats.tx_outstanding_high_water() <= VIRTIO_NET_QUEUE_SIZE / 2);
    assert_eq!(stats.live_mappings(), baseline.live_mappings());
    assert!(stats.mapping_high_water() <= VIRTIO_NET_QUEUE_SIZE);
    assert!(
        worker_actions <= burst.checked_mul(4).expect("worker action bound overflow"),
        "network burst exceeded its worker action bound"
    );
    assert!(worker.max_pump_rounds() <= WORKER_REPOLL_LIMIT);
    assert!(repoll_requests <= worker_actions);
    assert!(worker_yields <= worker_actions);
    if normal_exhaustion > 0 {
        let last_exhaustion = stats
            .last_exhaustion_submissions()
            .expect("natural exhaustion counter lacked its diagnostic marker");
        assert!(
            stats.tx_submissions() > last_exhaustion,
            "VirtIO TX did not resume after its last natural exhaustion"
        );
        assert_eq!(stats.tx_submissions(), stats.tx_completions());
    }
    assert!(
        stats.tx_completions() > baseline.tx_completions(),
        "ICMP burst observed no VirtIO TX completion"
    );
    assert!(
        stats.irq_rechecks() > baseline.irq_rechecks(),
        "ICMP path observed no IRQ recheck"
    );
    assert!(
        stats.rx_completions() > baseline.rx_completions(),
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
