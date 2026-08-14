mod support;

use anemone_net_api::{
    EthernetAddress, Instant, Ipv4Address, Ipv4Cidr, Ipv4EgressSelection,
    icmp_raw::IcmpRawNamespacePolicy,
    tcp::{
        TcpBindError, TcpBindRequest, TcpChildError, TcpConnectResult, TcpDiagnosticState,
        TcpEndpointFacts, TcpEndpointId, TcpListenBacklog, TcpListenError, TcpPeer,
        TcpPendingError, TcpReleaseReason,
    },
    udp::UdpNamespacePolicy,
};
use anemone_smoltcp_stack::{PumpBudget, Stack, StackPolicy, TcpPolicy};

use support::frame::BoundedProvider;

const SERVER_MAC: [u8; 6] = [0x02, 0, 0, 0, 21, 1];
const CLIENT_MAC: [u8; 6] = [0x02, 0, 0, 0, 21, 2];
const SERVER_IP: Ipv4Address = Ipv4Address::new([10, 0, 21, 1]);
const CLIENT_IP: Ipv4Address = Ipv4Address::new([10, 0, 21, 2]);
const LISTEN_PORT: u16 = 26001;

struct Topology {
    server: Stack,
    server_local: anemone_net_api::InterfaceId,
    server_external: anemone_net_api::InterfaceId,
    server_provider: BoundedProvider,
    client: Stack,
    client_external: anemone_net_api::InterfaceId,
    client_provider: BoundedProvider,
    server_frames: usize,
    client_frames: usize,
}

impl Topology {
    fn new(policy: TcpPolicy) -> Self {
        let mut server = Stack::with_policy(StackPolicy::new(
            UdpNamespacePolicy::new(4, 30000, 30003),
            IcmpRawNamespacePolicy::new(4),
            policy,
        ));
        let server_local = server
            .add_local_ipv4(
                Ipv4Cidr::new(Ipv4Address::LOOPBACK, 8).unwrap(),
                128,
                1500,
                Instant::ZERO,
            )
            .unwrap();
        let mut server_provider = BoundedProvider::with_capacities(SERVER_MAC, 16, 16);
        let server_external = server.add_interface(
            &mut server_provider,
            EthernetAddress::new(SERVER_MAC),
            Instant::ZERO,
        );
        server
            .configure_ipv4_for_host_validation(server_external, SERVER_IP.octets(), 24)
            .unwrap();
        server.add_local_delivery_ipv4(SERVER_IP).unwrap();

        let mut client = Stack::with_policy(StackPolicy::new(
            UdpNamespacePolicy::new(4, 30000, 30003),
            IcmpRawNamespacePolicy::new(4),
            policy,
        ));
        let mut client_provider = BoundedProvider::with_capacities(CLIENT_MAC, 16, 16);
        let client_external = client.add_interface(
            &mut client_provider,
            EthernetAddress::new(CLIENT_MAC),
            Instant::ZERO,
        );
        client
            .configure_ipv4_for_host_validation(client_external, CLIENT_IP.octets(), 24)
            .unwrap();

        Self {
            server,
            server_local,
            server_external,
            server_provider,
            client,
            client_external,
            client_provider,
            server_frames: 0,
            client_frames: 0,
        }
    }

    fn drive_local(&mut self, first_tick: i64) {
        for tick in first_tick..first_tick + 128 {
            self.server
                .pump_local(
                    self.server_local,
                    Instant::from_micros(tick * 1_000),
                    PumpBudget::new(32, 32),
                )
                .unwrap();
        }
    }

    fn drive_external(&mut self, first_tick: i64) {
        for tick in first_tick..first_tick + 256 {
            self.client
                .pump(
                    self.client_external,
                    &mut self.client_provider,
                    Instant::from_micros(tick * 1_000),
                    PumpBudget::new(32, 32),
                )
                .unwrap();
            transfer(
                &mut self.client_provider,
                &mut self.client_frames,
                &mut self.server_provider,
            );
            self.server
                .pump(
                    self.server_external,
                    &mut self.server_provider,
                    Instant::from_micros(tick * 1_000),
                    PumpBudget::new(32, 32),
                )
                .unwrap();
            transfer(
                &mut self.server_provider,
                &mut self.server_frames,
                &mut self.client_provider,
            );
        }
    }

    fn listen(&mut self, address: Ipv4Address, backlog: usize) -> TcpEndpointId {
        let listener = self.server.create_tcp_endpoint().unwrap();
        let _ = self
            .server
            .bind_tcp_endpoint(listener, TcpBindRequest::new(address, LISTEN_PORT))
            .unwrap();
        let _ = self
            .server
            .listen_tcp_endpoint_with_backlog(
                listener,
                Ipv4Address::UNSPECIFIED,
                TcpListenBacklog::new(backlog),
            )
            .unwrap();
        listener
    }

    fn start_local(&mut self) -> TcpEndpointId {
        let client = self.server.create_tcp_endpoint().unwrap();
        let _ = self
            .server
            .start_tcp_connect(
                client,
                Ipv4EgressSelection::new(self.server_local, SERVER_IP),
                TcpPeer::new(SERVER_IP, LISTEN_PORT),
            )
            .unwrap();
        client
    }

    fn start_external(&mut self, destination: Ipv4Address) -> TcpEndpointId {
        let client = self.client.create_tcp_endpoint().unwrap();
        let _ = self
            .client
            .start_tcp_connect(
                client,
                Ipv4EgressSelection::new(self.client_external, CLIENT_IP),
                TcpPeer::new(destination, LISTEN_PORT),
            )
            .unwrap();
        client
    }
}

fn policy(engine_capacity: usize, backlog_capacity: usize) -> TcpPolicy {
    TcpPolicy::new(
        32,
        engine_capacity,
        backlog_capacity,
        2,
        128,
        128,
        engine_capacity,
        60_000,
        60_000,
        40000,
        40031,
    )
}

fn transfer(from: &mut BoundedProvider, cursor: &mut usize, to: &mut BoundedProvider) {
    let frames = from.submitted_frames()[*cursor..].to_vec();
    *cursor += frames.len();
    from.complete_all();
    for frame in frames {
        to.inject(&frame);
    }
}

#[test]
fn external_specific_listener_aggregates_local_and_provider_ingress() {
    let mut topology = Topology::new(policy(32, 4));
    let listener = topology.listen(SERVER_IP, 2);
    let local_client = topology.start_local();
    topology.drive_local(0);
    let external_client = topology.start_external(SERVER_IP);
    topology.drive_external(128);

    assert!(matches!(
        topology.server.tcp_connect_result(local_client).unwrap(),
        TcpConnectResult::Connected { .. }
    ));
    assert!(matches!(
        topology.client.tcp_connect_result(external_client).unwrap(),
        TcpConnectResult::Connected { .. }
    ));
    let records = topology.server.tcp_diagnostic_records();
    let listener_record = records
        .iter()
        .filter(|record| record.state() == TcpDiagnosticState::Listen)
        .collect::<Vec<_>>();
    assert_eq!(listener_record.len(), 1);
    assert_eq!(listener_record[0].interface(), None);
    assert_eq!(listener_record[0].receive_queue(), 2);
    assert_eq!(listener_record[0].send_queue(), 2);
    assert!(records.iter().any(|record| {
        record.state() == TcpDiagnosticState::Established
            && record.interface() == Some(topology.server_local)
    }));
    assert!(records.iter().any(|record| {
        record.state() == TcpDiagnosticState::Established
            && record.interface() == Some(topology.server_external)
    }));

    let first = topology
        .server
        .claim_tcp_pending_child(listener)
        .unwrap()
        .unwrap();
    let second = topology
        .server
        .claim_tcp_pending_child(listener)
        .unwrap()
        .unwrap();
    let first = topology.server.take_tcp_child(first).unwrap();
    let second = topology.server.take_tcp_child(second).unwrap();
    assert!(topology.server.tcp_endpoint_peer(first).unwrap().is_some());
    assert!(topology.server.tcp_endpoint_peer(second).unwrap().is_some());
    assert_eq!(
        topology.server.tcp_endpoint_facts(listener),
        Ok(TcpEndpointFacts::Listener {
            has_pending_child: false
        })
    );
}

#[test]
fn wildcard_uses_both_paths_while_loopback_rejects_external_ingress() {
    let mut wildcard = Topology::new(policy(32, 2));
    let listener = wildcard.listen(Ipv4Address::UNSPECIFIED, 2);
    wildcard.start_local();
    wildcard.drive_local(0);
    let external = wildcard.start_external(SERVER_IP);
    wildcard.drive_external(128);
    assert!(matches!(
        wildcard.client.tcp_connect_result(external).unwrap(),
        TcpConnectResult::Connected { .. }
    ));
    assert_eq!(
        wildcard
            .server
            .tcp_diagnostic_records()
            .into_iter()
            .find(|record| record.state() == TcpDiagnosticState::Listen)
            .unwrap()
            .receive_queue(),
        2
    );
    assert!(
        wildcard
            .server
            .claim_tcp_pending_child(listener)
            .unwrap()
            .is_some()
    );

    let mut loopback = Topology::new(policy(16, 1));
    let listener = loopback.listen(Ipv4Address::LOOPBACK, 1);
    let external = loopback.start_external(SERVER_IP);
    loopback.drive_external(0);
    assert!(matches!(
        loopback.client.tcp_connect_result(external).unwrap(),
        TcpConnectResult::Failed(TcpPendingError::ConnectionRefused)
            | TcpConnectResult::Failed(TcpPendingError::ConnectionReset)
    ));
    assert!(
        loopback
            .server
            .claim_tcp_pending_child(listener)
            .unwrap()
            .is_none()
    );
}

#[test]
fn aggregate_full_backlog_rejects_new_projection_without_displacing_child() {
    let mut topology = Topology::new(policy(16, 1));
    let listener = topology.listen(SERVER_IP, 1);
    topology.start_local();
    topology.drive_local(0);
    let external = topology.start_external(SERVER_IP);
    topology.drive_external(128);

    let listener_record = topology
        .server
        .tcp_diagnostic_records()
        .into_iter()
        .find(|record| record.state() == TcpDiagnosticState::Listen)
        .unwrap();
    assert_eq!(listener_record.receive_queue(), 1);
    assert!(matches!(
        topology.client.tcp_connect_result(external).unwrap(),
        TcpConnectResult::Failed(TcpPendingError::ConnectionReset) | TcpConnectResult::Terminal
    ));
    assert!(
        topology
            .server
            .claim_tcp_pending_child(listener)
            .unwrap()
            .is_some()
    );
}

#[test]
fn publication_capacity_failure_precedes_binding_and_engine_publication() {
    let mut topology = Topology::new(policy(1, 1));
    let explicit = topology.server.create_tcp_endpoint().unwrap();
    topology
        .server
        .bind_tcp_endpoint(explicit, TcpBindRequest::new(SERVER_IP, LISTEN_PORT))
        .unwrap();
    assert_eq!(
        topology.server.listen_tcp_endpoint_with_backlog(
            explicit,
            Ipv4Address::UNSPECIFIED,
            TcpListenBacklog::new(1),
        ),
        Err(TcpListenError::EngineCapacity)
    );
    assert_eq!(
        topology
            .server
            .tcp_endpoint_binding(explicit)
            .unwrap()
            .unwrap()
            .port(),
        LISTEN_PORT
    );

    let implicit = topology.server.create_tcp_endpoint().unwrap();
    assert_eq!(
        topology.server.listen_tcp_endpoint_with_backlog(
            implicit,
            Ipv4Address::UNSPECIFIED,
            TcpListenBacklog::new(1),
        ),
        Err(TcpListenError::EngineCapacity)
    );
    assert_eq!(
        topology.server.tcp_endpoint_binding(implicit).unwrap(),
        None
    );
    assert!(
        topology
            .server
            .tcp_diagnostic_records()
            .iter()
            .all(|record| record.state() != TcpDiagnosticState::Listen)
    );
}

#[test]
fn wildcard_degrades_to_local_only_without_an_external_deployment() {
    let mut stack = Stack::with_policy(StackPolicy::new(
        UdpNamespacePolicy::new(4, 30000, 30003),
        IcmpRawNamespacePolicy::new(4),
        policy(8, 2),
    ));
    let local = stack
        .add_local_ipv4(
            Ipv4Cidr::new(Ipv4Address::LOOPBACK, 8).unwrap(),
            128,
            1500,
            Instant::ZERO,
        )
        .unwrap();
    let listener = stack.create_tcp_endpoint().unwrap();
    stack
        .bind_tcp_endpoint(
            listener,
            TcpBindRequest::new(Ipv4Address::UNSPECIFIED, LISTEN_PORT),
        )
        .unwrap();
    stack
        .listen_tcp_endpoint_with_backlog(
            listener,
            Ipv4Address::UNSPECIFIED,
            TcpListenBacklog::new(1),
        )
        .unwrap();
    let client = stack.create_tcp_endpoint().unwrap();
    let _ = stack
        .start_tcp_connect(
            client,
            Ipv4EgressSelection::new(local, Ipv4Address::LOOPBACK),
            TcpPeer::new(Ipv4Address::LOOPBACK, LISTEN_PORT),
        )
        .unwrap();
    for tick in 0..128 {
        stack
            .pump_local(
                local,
                Instant::from_micros(tick * 1_000),
                PumpBudget::new(32, 32),
            )
            .unwrap();
    }

    assert!(matches!(
        stack.tcp_connect_result(client).unwrap(),
        TcpConnectResult::Connected { .. }
    ));
    assert!(stack.claim_tcp_pending_child(listener).unwrap().is_some());
    assert_eq!(
        stack
            .tcp_diagnostic_records()
            .into_iter()
            .find(|record| record.state() == TcpDiagnosticState::Listen)
            .unwrap()
            .receive_queue(),
        1
    );
}

#[test]
fn relisten_shrink_and_grow_preserve_aggregate_admitted_children() {
    let mut topology = Topology::new(policy(32, 3));
    let listener = topology.listen(SERVER_IP, 2);
    let _ = topology.start_local();
    topology.drive_local(0);
    let _ = topology.start_external(SERVER_IP);
    topology.drive_external(128);

    topology
        .server
        .listen_tcp_endpoint_with_backlog(
            listener,
            Ipv4Address::UNSPECIFIED,
            TcpListenBacklog::new(1),
        )
        .unwrap();
    let record = topology
        .server
        .tcp_diagnostic_records()
        .into_iter()
        .find(|record| record.state() == TcpDiagnosticState::Listen)
        .unwrap();
    assert_eq!(record.receive_queue(), 2);
    assert_eq!(record.send_queue(), 1);

    let claimed = topology
        .server
        .claim_tcp_pending_child(listener)
        .unwrap()
        .unwrap();
    let record = topology
        .server
        .tcp_diagnostic_records()
        .into_iter()
        .find(|record| record.state() == TcpDiagnosticState::Listen)
        .unwrap();
    assert_eq!(record.receive_queue(), 2);
    assert_eq!(record.send_queue(), 1);
    let _ = topology.server.take_tcp_child(claimed).unwrap();

    topology
        .server
        .listen_tcp_endpoint_with_backlog(
            listener,
            Ipv4Address::UNSPECIFIED,
            TcpListenBacklog::new(3),
        )
        .unwrap();
    let _ = topology.start_local();
    topology.drive_local(512);
    let _ = topology.start_external(SERVER_IP);
    topology.drive_external(640);
    let record = topology
        .server
        .tcp_diagnostic_records()
        .into_iter()
        .find(|record| record.state() == TcpDiagnosticState::Listen)
        .unwrap();
    assert_eq!(record.receive_queue(), 3);
    assert_eq!(record.send_queue(), 3);
}

#[test]
fn external_child_cancel_invalidates_only_its_projection_generation() {
    let mut topology = Topology::new(policy(16, 1));
    let listener = topology.listen(SERVER_IP, 1);
    let _ = topology.start_external(SERVER_IP);
    topology.drive_external(0);
    let child = topology
        .server
        .claim_tcp_pending_child(listener)
        .unwrap()
        .unwrap();
    assert!(topology.server.cancel_tcp_child(child).unwrap().is_some());
    assert_eq!(
        topology.server.cancel_tcp_child(child),
        Err(TcpChildError::StaleChild)
    );
    assert_eq!(
        topology.server.take_tcp_child(child),
        Err(TcpChildError::StaleChild)
    );
    topology.drive_external(512);

    let _ = topology.start_external(SERVER_IP);
    topology.drive_external(1024);
    let replacement = topology
        .server
        .claim_tcp_pending_child(listener)
        .unwrap()
        .unwrap();
    let _ = topology.server.take_tcp_child(replacement).unwrap();
    assert!(
        topology
            .server
            .tcp_diagnostic_records()
            .iter()
            .any(|record| {
                record.state() == TcpDiagnosticState::Established
                    && record.interface() == Some(topology.server_external)
            })
    );
}

#[test]
fn listener_withdrawal_closes_pending_and_claimed_projections_before_reuse() {
    let mut topology = Topology::new(policy(32, 2));
    let listener = topology.listen(SERVER_IP, 2);
    let _ = topology.start_local();
    topology.drive_local(0);
    let _ = topology.start_external(SERVER_IP);
    topology.drive_external(128);
    let claimed = topology
        .server
        .claim_tcp_pending_child(listener)
        .unwrap()
        .unwrap();

    let progressions = topology
        .server
        .release_tcp_endpoint(listener, TcpReleaseReason::ListenerWithdrawal)
        .unwrap();
    assert_eq!(progressions.len(), 2);
    assert!(
        topology
            .server
            .tcp_diagnostic_records()
            .iter()
            .all(|record| record.state() != TcpDiagnosticState::Listen)
    );
    assert_eq!(
        topology.server.take_tcp_child(claimed),
        Err(TcpChildError::StaleChild)
    );

    let contender = topology.server.create_tcp_endpoint().unwrap();
    assert_eq!(
        topology
            .server
            .bind_tcp_endpoint(contender, TcpBindRequest::new(SERVER_IP, LISTEN_PORT)),
        Err(TcpBindError::PortInUse)
    );
    topology.drive_local(512);
    topology.drive_external(640);
    topology
        .server
        .bind_tcp_endpoint(contender, TcpBindRequest::new(SERVER_IP, LISTEN_PORT))
        .unwrap();
}

#[test]
fn idle_listener_withdrawal_removes_every_projection_synchronously() {
    let mut topology = Topology::new(policy(8, 1));
    let listener = topology.listen(SERVER_IP, 1);
    assert!(
        topology
            .server
            .release_tcp_endpoint(listener, TcpReleaseReason::ListenerWithdrawal)
            .unwrap()
            .is_empty()
    );

    let contender = topology.server.create_tcp_endpoint().unwrap();
    topology
        .server
        .bind_tcp_endpoint(contender, TcpBindRequest::new(SERVER_IP, LISTEN_PORT))
        .unwrap();
}

#[test]
fn half_open_external_projection_retains_binding_until_protocol_cleanup() {
    let mut topology = Topology::new(policy(8, 1));
    let listener = topology.listen(SERVER_IP, 1);
    let _ = topology.start_external(SERVER_IP);

    let mut half_open = false;
    for tick in 0..32 {
        topology
            .client
            .pump(
                topology.client_external,
                &mut topology.client_provider,
                Instant::from_micros(tick * 1_000),
                PumpBudget::new(32, 32),
            )
            .unwrap();
        transfer(
            &mut topology.client_provider,
            &mut topology.client_frames,
            &mut topology.server_provider,
        );
        topology
            .server
            .pump(
                topology.server_external,
                &mut topology.server_provider,
                Instant::from_micros(tick * 1_000),
                PumpBudget::new(32, 32),
            )
            .unwrap();
        half_open = topology
            .server
            .tcp_diagnostic_records()
            .iter()
            .any(|record| record.state() == TcpDiagnosticState::SynReceived);
        if half_open {
            break;
        }
        transfer(
            &mut topology.server_provider,
            &mut topology.server_frames,
            &mut topology.client_provider,
        );
    }
    assert!(half_open);

    let progressions = topology
        .server
        .release_tcp_endpoint(listener, TcpReleaseReason::ListenerWithdrawal)
        .unwrap();
    assert_eq!(progressions.len(), 1);
    let contender = topology.server.create_tcp_endpoint().unwrap();
    assert_eq!(
        topology
            .server
            .bind_tcp_endpoint(contender, TcpBindRequest::new(SERVER_IP, LISTEN_PORT)),
        Err(TcpBindError::PortInUse)
    );

    transfer(
        &mut topology.server_provider,
        &mut topology.server_frames,
        &mut topology.client_provider,
    );
    topology.drive_external(128);
    topology
        .server
        .bind_tcp_endpoint(contender, TcpBindRequest::new(SERVER_IP, LISTEN_PORT))
        .unwrap();
}
