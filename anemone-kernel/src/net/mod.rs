//! Kernel-side network attach authority.

mod domain;
mod worker;

use crate::{
    device::net::{
        NetdevFrameProvider, NetdevSnapshot, PendingAttachProvider, PublishedNetdev,
        retain_pending, take_pending,
    },
    prelude::*,
};

use domain::{
    ControlPlaneActivationError, ExternalControlInput, InitialDomain, LogicalInterfaceReservation,
    LogicalInterfaceSnapshot,
};
use worker::{AttachFailure, PumpControl};

struct ActivePath {
    /// Immutable device-publication snapshot for diagnosis only. Current
    /// provider truth remains in the worker-owned provider.
    netdev: NetdevSnapshot,
    /// Immutable domain-membership snapshot for diagnosis and shutdown logs
    /// only. It never drives provider or Stack admission.
    logical: LogicalInterfaceSnapshot,
    /// Immutable boot-lifetime logical-to-protocol association handed to the
    /// control-plane owner. DomainStack remains the mapping truth; R0 has no
    /// detach or ID reuse, so this protocol state cannot become stale.
    interface: anemone_net_api::InterfaceId,
    control: Arc<PumpControl>,
}

struct AttachAuthority {
    domain: InitialDomain,
    active_paths: Vec<ActivePath>,
    /// Sole admission truth for terminal network shutdown. Per-path `active`
    /// bits are capability-local projections and cannot reopen this gate.
    shutdown_started: bool,
}

impl AttachAuthority {
    fn new() -> Self {
        Self {
            domain: InitialDomain::new(),
            active_paths: Vec::new(),
            shutdown_started: false,
        }
    }
}

/// Initial-domain composition, active publication, and terminal shutdown
/// admission owner. Entries contain no provider backing, queue truth, smoltcp
/// handle, task, timer, or IRQ object.
static ACTIVE_PATHS: Lazy<SpinLock<AttachAuthority>> =
    Lazy::new(|| SpinLock::new(AttachAuthority::new()));

impl<P: NetdevFrameProvider> PendingAttachProvider for P {
    fn attach(published: PublishedNetdev<Self>) -> Result<(), PublishedNetdev<Self>> {
        attach_one(published)
    }
}

fn attach_one<P: NetdevFrameProvider>(
    published: PublishedNetdev<P>,
) -> Result<(), PublishedNetdev<P>> {
    let Some(ethernet_address) = published.snapshot().facts().ethernet_address else {
        kerrln!("published network device has no Ethernet address; leaving it unattached");
        return Err(published);
    };

    let (reservation, stack) = {
        let mut authority = ACTIVE_PATHS.lock();
        if authority.shutdown_started {
            kerrln!(
                "network shutdown already started; retaining netdev {} ({}) as published/unattached",
                published.snapshot().id().index(),
                published.snapshot().origin(),
            );
            return Err(published);
        }
        let reservation = authority
            .domain
            .logical_mut()
            .reserve_external(published.snapshot().id());
        let stack = authority.domain.stack();
        (reservation, stack)
    };

    let worker_name = reservation.snapshot().name().to_string();
    match worker::prepare(published, stack, ethernet_address, &worker_name) {
        Ok(prepared) => {
            publish_prepared(reservation, prepared);
            Ok(())
        },
        Err(AttachFailure::WorkerSpawn { error, published }) => {
            let mut authority = ACTIVE_PATHS.lock();
            authority.domain.logical_mut().abort(reservation);
            drop(authority);
            kerrln!(
                "failed to spawn network pump worker: {:?}; leaving netdev published/unattached",
                error
            );
            Err(published)
        },
    }
}

fn publish_prepared(reservation: LogicalInterfaceReservation, mut prepared: worker::PreparedPath) {
    let netdev = prepared.snapshot().clone();
    let interface = prepared.interface();
    let mut authority = ACTIVE_PATHS.lock();
    if authority.shutdown_started {
        // Mapping withdrawal precedes reservation abort. The inactive worker
        // is stopped only after both unpublished domain facts are gone.
        drop(authority);
        prepared.rollback_mapping();
        let mut authority = ACTIVE_PATHS.lock();
        assert!(
            authority.shutdown_started,
            "terminal network admission cannot reopen"
        );
        authority.domain.logical_mut().abort(reservation);
        drop(authority);
        prepared.stop_and_retain();
        kerrln!(
            "network shutdown closed admission; netdev {} ({}) retained without a logical or Stack publication",
            netdev.id().index(),
            netdev.origin(),
        );
        return;
    }

    let logical = authority.domain.logical_mut().commit(reservation);
    authority.active_paths.push(ActivePath {
        netdev: netdev.clone(),
        logical: logical.clone(),
        interface,
        control: prepared.control(),
    });
    // Readers cannot observe the logical or active record until mapping,
    // worker, IRQ-wake, and time wiring are ready. Activation is the final
    // step in this authority critical section.
    prepared.activate();
    drop(authority);
    kinfoln!(
        "network path {} (ifindex {}, {:?}) from netdev {} ({}) active as protocol interface {:?}",
        logical.name(),
        logical.ifindex(),
        logical.kind(),
        netdev.id().index(),
        netdev.origin(),
        interface,
    );
}

/// Attach every capability that was pending when the boot-time drain began.
/// Failed entries are returned to registry ownership but are not retried by
/// this drain; runtime retry is outside the boot-only lifecycle.
pub(crate) fn attach_published_netdevs() {
    // Force initial-domain/lo/global-Stack construction even when no external
    // publication is pending in this boot drain.
    drop(ACTIVE_PATHS.lock());
    for pending in take_pending() {
        if let Err(pending) = pending.attach() {
            retain_pending(pending);
        }
    }
    activate_initial_control_plane();
}

fn activate_initial_control_plane() {
    let mut authority = ACTIVE_PATHS.lock();
    assert!(
        !authority.shutdown_started,
        "boot control-plane activation raced terminal shutdown"
    );
    let external = authority
        .active_paths
        .iter()
        .map(|path| {
            ExternalControlInput::new(
                path.logical.clone(),
                path.interface,
                path.control.pump_wake(),
            )
        })
        .collect::<Vec<_>>();
    let deployment = crate::network_defs::STATIC_IPV4_DEPLOYMENT;
    if let Err(error) = authority
        .domain
        .activate_control_plane(deployment, &external)
    {
        let actual = external
            .iter()
            .map(ExternalControlInput::name)
            .collect::<Vec<_>>();
        let expected = deployment.map(|deployment| deployment.interface);
        kerrln!(
            "static IPv4 control-plane activation failed: {:?}; expected interface {:?}, published external interfaces {:?}",
            error,
            expected,
            actual,
        );
        match error {
            ControlPlaneActivationError::AlreadyPublished
            | ControlPlaneActivationError::MissingInterface
            | ControlPlaneActivationError::DuplicateInterface => {
                panic!("static IPv4 SystemTarget does not match published network interfaces")
            },
        }
    }

    if let Some(deployment) = deployment {
        let logical = authority
            .domain
            .control_plane()
            .and_then(|control| control.external_logical())
            .expect("configured control plane must retain its external association");
        kinfoln!(
            "IPv4 control plane active: lo ready, {} (ifindex {}) = {}.{}.{}.{}/{}; default route {}",
            logical.name(),
            logical.ifindex(),
            deployment.address[0],
            deployment.address[1],
            deployment.address[2],
            deployment.address[3],
            deployment.prefix,
            if deployment.default_gateway.is_some() {
                "configured"
            } else {
                "absent"
            },
        );
    } else {
        kinfoln!("IPv4 control plane active: loopback-only local path ready");
    }
}

/// Close network attach admission and request one non-waiting stop attempt for
/// every active path. System Power owns the surrounding global order.
pub(crate) unsafe fn shutdown() {
    let (local, paths) = {
        let mut authority = ACTIVE_PATHS.lock();
        assert!(
            !authority.shutdown_started,
            "network shutdown admission closed more than once"
        );
        authority.shutdown_started = true;
        let local = authority.domain.withdraw_control_plane();
        let paths = authority
            .active_paths
            .iter()
            .map(|path| {
                (
                    path.netdev.clone(),
                    path.logical.clone(),
                    path.control.clone(),
                )
            })
            .collect::<Vec<_>>();
        (local, paths)
    };

    if let Some(local) = local {
        local.request_shutdown();
        kemergln!("initial-domain local network worker shutdown: admission closed, stop requested");
    }

    for (netdev, logical, control) in paths {
        control.request_shutdown();
        kemergln!(
            "network path {} (ifindex {}) from netdev {} ({}) shutdown: admission closed, stop requested; terminal resources retained",
            logical.name(),
            logical.ifindex(),
            netdev.id().index(),
            netdev.origin(),
        );
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use anemone_net_api::Ipv4Address;

    use super::*;

    /// Exercises the boot-published control plane, DomainStack and sleeping
    /// local worker. Stage 3 must replace the temporary UDP probe calls with
    /// its real Socket-to-Endpoint capability or delete redundant coverage.
    fn deliver_over_production_local_path(
        destination: Ipv4Address,
        sender_port: u16,
        receiver_port: u16,
        payload: &[u8],
    ) {
        let (stack, selection) = {
            let authority = ACTIVE_PATHS.lock();
            let selection = authority
                .domain
                .control_plane()
                .expect("KUnit runs after boot control-plane publication")
                .select(destination, None)
                .expect("KUnit destination must select the production local path");
            (authority.domain.stack(), selection)
        };

        let sender = stack
            .create_udp_probe(sender_port)
            .expect("KUnit sender Endpoint must bind");
        let receiver = stack
            .create_udp_probe(receiver_port)
            .expect("KUnit receiver Endpoint must bind");
        stack
            .send_udp_probe(
                sender,
                selection.interface(),
                selection.source(),
                destination,
                receiver_port,
                payload,
            )
            .expect("KUnit local datagram must queue");
        selection.request_pump();

        let mut received = None;
        for _ in 0..10_000 {
            if let Some(datagram) = stack.receive_udp_probe(receiver) {
                received = Some(datagram);
                break;
            }
            yield_now();
        }
        let received = received.expect("bounded local worker did not deliver KUnit datagram");
        assert_eq!(received.payload.as_slice(), payload);
        assert_eq!(received.source_address, selection.source());
        assert_eq!(received.source_port, sender_port);

        stack
            .retire_udp_probe(sender)
            .expect("KUnit sender Endpoint must retire");
        stack
            .retire_udp_probe(receiver)
            .expect("KUnit receiver Endpoint must retire");
    }

    #[kunit]
    fn udp_loopback_worker_127_0_0_1() {
        deliver_over_production_local_path(Ipv4Address::LOOPBACK, 39001, 39002, b"loopback");
    }

    #[kunit]
    fn udp_loopback_worker_other_127_8() {
        deliver_over_production_local_path(
            Ipv4Address::new([127, 17, 23, 42]),
            39003,
            39004,
            b"loopback-cidr",
        );
    }

    #[kunit]
    fn udp_self_external_uses_local_handoff() {
        let Some(destination) = crate::network_defs::STATIC_IPV4_DEPLOYMENT
            .map(|deployment| Ipv4Address::new(deployment.address))
        else {
            kinfoln!("self-external local-handoff KUnit skipped for loopback-only SystemTarget");
            return;
        };
        deliver_over_production_local_path(destination, 39005, 39006, b"self-external");
    }
}
