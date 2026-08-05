//! The sole TCP namespace, endpoint, stream, and reclaim owner.

mod listener;
mod namespace;
mod reclaim;
mod stream;

use alloc::{vec, vec::Vec};

use anemone_net_api::{
    InterfaceId,
    tcp::{TcpDisconnectCause, TcpEndpointId, TcpLocalBinding, TcpPeer, TcpReceiveReservationId},
};
use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::{self, tcp},
    wire::IpListenEndpoint,
};

/// Immutable TCP resource policy supplied by the kernel at Stack construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TcpPolicy {
    endpoint_capacity: usize,
    engine_timer_capacity: usize,
    listener_completed_capacity: usize,
    rx_buffer_bytes: usize,
    tx_buffer_bytes: usize,
    deferred_reclaim_capacity: usize,
    ephemeral_port_first: u16,
    ephemeral_port_last: u16,
}

impl TcpPolicy {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        endpoint_capacity: usize,
        engine_timer_capacity: usize,
        listener_completed_capacity: usize,
        rx_buffer_bytes: usize,
        tx_buffer_bytes: usize,
        deferred_reclaim_capacity: usize,
        ephemeral_port_first: u16,
        ephemeral_port_last: u16,
    ) -> Self {
        Self {
            endpoint_capacity,
            engine_timer_capacity,
            listener_completed_capacity,
            rx_buffer_bytes,
            tx_buffer_bytes,
            deferred_reclaim_capacity,
            ephemeral_port_first,
            ephemeral_port_last,
        }
    }
}

pub(crate) struct EndpointSlot {
    id: Option<TcpEndpointId>,
    pub(crate) role: EndpointRole,
}

pub(crate) enum EndpointRole {
    Vacant,
    Idle,
    Bound(TcpLocalBinding),
    Listener(Listener),
    Connection(Connection),
    Reclaiming {
        remaining: usize,
        binding: Option<TcpLocalBinding>,
    },
}

pub(crate) struct Listener {
    pub(crate) interface: InterfaceId,
    pub(crate) binding: TcpLocalBinding,
    pub(crate) slots: Vec<ListenerSlot>,
}

pub(crate) struct ListenerSlot {
    pub(crate) generation: u64,
    pub(crate) handle: Option<SocketHandle>,
    pub(crate) claimed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectionPhase {
    Connecting,
    Connected,
    Failed(TcpDisconnectCause),
}

pub(crate) struct Connection {
    pub(crate) interface: InterfaceId,
    pub(crate) handle: SocketHandle,
    /// Namespace reservation may remain wildcard while the connected tuple
    /// has a concrete source selected by the control-plane owner.
    pub(crate) binding: TcpLocalBinding,
    pub(crate) local: TcpLocalBinding,
    pub(crate) peer: TcpPeer,
    pub(crate) phase: ConnectionPhase,
    pub(crate) reservation: Option<OutstandingReceive>,
    pub(crate) retire_requested: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct OutstandingReceive {
    pub(crate) id: TcpReceiveReservationId,
    pub(crate) offered: usize,
}

pub(crate) struct DeferredReclaim {
    pub(crate) interface: InterfaceId,
    pub(crate) handle: SocketHandle,
    pub(crate) action: ReclaimAction,
}

pub(crate) enum ReclaimAction {
    RearmListener {
        listener: TcpEndpointId,
        slot: usize,
        generation: u64,
    },
    ReleaseEndpoint(TcpEndpointId),
}

/// Aggregate owner for every TCP fact and protocol engine in one Stack.
pub(crate) struct TcpEndpoints {
    policy: TcpPolicy,
    endpoints: Vec<EndpointSlot>,
    next_endpoint_id: u64,
    next_reservation_id: u64,
    engine_count: usize,
    deferred: Vec<DeferredReclaim>,
}

impl TcpEndpoints {
    pub(crate) fn new(policy: TcpPolicy) -> Self {
        assert!(
            policy.deferred_reclaim_capacity >= policy.engine_timer_capacity,
            "TCP deferred reclaim storage must cover every protocol engine"
        );
        let mut endpoints = Vec::with_capacity(policy.endpoint_capacity);
        for _ in 0..policy.endpoint_capacity {
            endpoints.push(EndpointSlot {
                id: None,
                role: EndpointRole::Vacant,
            });
        }
        Self {
            policy,
            endpoints,
            next_endpoint_id: 1,
            next_reservation_id: 1,
            engine_count: 0,
            // Construction reserves every possible retiring engine. Normal
            // cleanup therefore never allocates and cannot be rejected after
            // a capability has crossed into the kernel.
            deferred: Vec::with_capacity(policy.deferred_reclaim_capacity),
        }
    }

    pub(crate) fn policy(&self) -> TcpPolicy {
        self.policy
    }

    pub(crate) fn reservation_interface(
        &self,
        reservation: TcpReceiveReservationId,
    ) -> Option<InterfaceId> {
        self.endpoints.iter().find_map(|slot| match &slot.role {
            EndpointRole::Connection(connection)
                if connection
                    .reservation
                    .is_some_and(|entry| entry.id == reservation) =>
            {
                Some(connection.interface)
            },
            _ => None,
        })
    }

    pub(crate) fn endpoint(&self, id: TcpEndpointId) -> Option<&EndpointSlot> {
        self.endpoints.iter().find(|slot| slot.id == Some(id))
    }

    pub(crate) fn endpoint_mut(&mut self, id: TcpEndpointId) -> Option<&mut EndpointSlot> {
        self.endpoints.iter_mut().find(|slot| slot.id == Some(id))
    }

    pub(crate) fn endpoint_index(&self, id: TcpEndpointId) -> Option<usize> {
        self.endpoints.iter().position(|slot| slot.id == Some(id))
    }

    pub(crate) fn connection(&self, id: TcpEndpointId) -> Option<&Connection> {
        let EndpointRole::Connection(connection) = &self.endpoint(id)?.role else {
            return None;
        };
        Some(connection)
    }

    pub(crate) fn connection_mut(&mut self, id: TcpEndpointId) -> Option<&mut Connection> {
        let EndpointRole::Connection(connection) = &mut self.endpoint_mut(id)?.role else {
            return None;
        };
        Some(connection)
    }

    pub(crate) fn listener(&self, id: TcpEndpointId) -> Option<&Listener> {
        let EndpointRole::Listener(listener) = &self.endpoint(id)?.role else {
            return None;
        };
        Some(listener)
    }

    pub(crate) fn listener_mut(&mut self, id: TcpEndpointId) -> Option<&mut Listener> {
        let EndpointRole::Listener(listener) = &mut self.endpoint_mut(id)?.role else {
            return None;
        };
        Some(listener)
    }

    pub(crate) fn ensure_engine_capacity(&self, additional: usize) -> bool {
        self.engine_count
            .checked_add(additional)
            .is_some_and(|count| count <= self.policy.engine_timer_capacity)
    }

    pub(crate) fn add_listener_engine(
        &mut self,
        sockets: &mut SocketSet<'static>,
        binding: TcpLocalBinding,
    ) -> SocketHandle {
        let mut socket = tcp_socket(self.policy);
        socket
            .listen(to_smoltcp_listen(binding))
            .expect("owner-validated TCP listener binding must be valid");
        let handle = sockets.add(socket);
        self.engine_count += 1;
        assert!(self.engine_count <= self.policy.engine_timer_capacity);
        handle
    }

    pub(crate) fn remove_engine(&mut self, sockets: &mut SocketSet<'static>, handle: SocketHandle) {
        match sockets.remove(handle) {
            socket::Socket::Tcp(_) => {},
            _ => unreachable!("TCP owner only records TCP engine handles"),
        }
        self.engine_count = self
            .engine_count
            .checked_sub(1)
            .expect("TCP engine count underflow");
    }

    pub(crate) fn queue_reclaim(&mut self, reclaim: DeferredReclaim) {
        self.deferred.push(reclaim);
        // Every engine contributes at most one entry, and construction has
        // already reserved at least this many slots. Assert after publishing
        // cleanup so a bookkeeping bug cannot turn final release into a leak.
        assert!(self.deferred.len() <= self.engine_count);
        assert!(self.deferred.len() <= self.policy.deferred_reclaim_capacity);
    }
}

pub(crate) fn tcp_socket(policy: TcpPolicy) -> tcp::Socket<'static> {
    tcp::Socket::new(
        tcp::SocketBuffer::new(vec![0; policy.rx_buffer_bytes]),
        tcp::SocketBuffer::new(vec![0; policy.tx_buffer_bytes]),
    )
}

pub(crate) fn completed_child_state(state: tcp::State) -> bool {
    matches!(state, tcp::State::Established | tcp::State::CloseWait)
}

pub(crate) fn to_smoltcp_listen(binding: TcpLocalBinding) -> IpListenEndpoint {
    let address = (!binding.address().is_unspecified())
        .then(|| smoltcp::wire::IpAddress::Ipv4(binding.address().octets().into()));
    IpListenEndpoint {
        addr: address,
        port: binding.port(),
    }
}

pub(crate) fn map_disconnect(reason: tcp::DisconnectReason) -> TcpDisconnectCause {
    match reason {
        tcp::DisconnectReason::Reset => TcpDisconnectCause::Reset,
        tcp::DisconnectReason::Timeout => TcpDisconnectCause::Timeout,
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use anemone_net_api::{
        Instant, Ipv4Address, Ipv4Cidr, Ipv4EgressSelection,
        icmp_raw::IcmpRawNamespacePolicy,
        tcp::{
            TcpBindRequest, TcpConnectionObservation, TcpDisconnectCause, TcpPeer, TcpQueryError,
            TcpReceiveResolveError,
        },
        udp::UdpNamespacePolicy,
    };
    use smoltcp::{
        iface::{Config, Interface, SocketSet},
        phy::{FaultInjector, Loopback, Medium},
        time::{Duration as SmoltcpDuration, Instant as SmoltcpInstant},
        wire::{HardwareAddress, IpAddress, IpCidr, IpEndpoint, IpListenEndpoint},
    };

    use crate::{
        PumpBudget,
        stack::{Stack, StackPolicy},
    };

    use super::*;

    const LOCAL: Ipv4Address = Ipv4Address::new([127, 0, 0, 1]);
    const LISTEN_PORT: u16 = 2345;
    const POLICY: TcpPolicy = TcpPolicy::new(64, 64, 10, 32, 32, 64, 40000, 40063);

    fn test_stack(policy: TcpPolicy) -> (Stack, InterfaceId) {
        let mut stack = Stack::with_policy(StackPolicy::new(
            UdpNamespacePolicy::new(4, 30000, 30003),
            IcmpRawNamespacePolicy::new(4),
            policy,
        ));
        let interface = stack
            .add_local_ipv4(Ipv4Cidr::new(LOCAL, 8).unwrap(), 128, 1500, Instant::ZERO)
            .unwrap();
        (stack, interface)
    }

    fn drive(stack: &mut Stack, interface: InterfaceId, first_tick: i64) {
        for tick in first_tick..first_tick + 128 {
            let _ = stack
                .pump_local(
                    interface,
                    Instant::from_micros(tick * 1_000),
                    PumpBudget::new(32, 32),
                )
                .unwrap();
        }
    }

    #[test]
    fn active_passive_namespace_stream_and_generation_share_one_owner() {
        let (mut stack, interface) = test_stack(POLICY);
        let listener = stack.create_tcp_endpoint().unwrap();
        let binding = stack
            .bind_tcp_endpoint(listener, TcpBindRequest::new(LOCAL, LISTEN_PORT))
            .unwrap();
        assert_eq!(binding.port(), LISTEN_PORT);
        let _ = stack
            .listen_tcp_endpoint(listener, interface, Ipv4Address::UNSPECIFIED)
            .unwrap();

        let conflict = stack.create_tcp_endpoint().unwrap();
        assert!(matches!(
            stack.bind_tcp_endpoint(conflict, TcpBindRequest::new(LOCAL, LISTEN_PORT)),
            Err(anemone_net_api::tcp::TcpBindError::PortInUse)
        ));

        let mut clients = Vec::new();
        for _ in 0..10 {
            let client = stack.create_tcp_endpoint().unwrap();
            let _ = stack
                .start_tcp_connect(
                    client,
                    Ipv4EgressSelection::new(interface, LOCAL),
                    TcpPeer::new(LOCAL, LISTEN_PORT),
                )
                .unwrap();
            assert!(matches!(
                stack.observe_tcp_connection(client).unwrap(),
                TcpConnectionObservation::Connecting { .. }
            ));
            clients.push(client);
        }
        drive(&mut stack, interface, 0);
        for client in &clients {
            assert!(matches!(
                stack.observe_tcp_connection(*client).unwrap(),
                TcpConnectionObservation::Connected { .. }
            ));
        }

        let overflow = stack.create_tcp_endpoint().unwrap();
        let _ = stack
            .start_tcp_connect(
                overflow,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, LISTEN_PORT),
            )
            .unwrap();
        drive(&mut stack, interface, 128);
        assert!(matches!(
            stack.observe_tcp_connection(overflow).unwrap(),
            TcpConnectionObservation::Failed {
                cause: TcpDisconnectCause::Reset,
                ..
            }
        ));
        stack.retire_tcp_endpoint(overflow).unwrap();

        let child = stack
            .claim_tcp_pending_child(listener)
            .unwrap()
            .expect("one completed child must be claimable");
        let accepted = stack.take_tcp_child(child).unwrap();
        assert!(matches!(
            stack.observe_tcp_connection(accepted).unwrap(),
            TcpConnectionObservation::Connected { .. }
        ));
        assert!(matches!(
            stack.cancel_tcp_child(child),
            Err(anemone_net_api::tcp::TcpChildError::StaleChild)
        ));

        let accepted_prefix = stack.send_tcp_endpoint(clients[0], &[0x5a; 64]).unwrap().0;
        assert_eq!(accepted_prefix, POLICY.tx_buffer_bytes);
        drive(&mut stack, interface, 256);

        let reservation = stack.reserve_tcp_receive(accepted, 64).unwrap();
        assert_eq!(reservation.bytes(), &[0x5a; 32]);
        let (reservation_id, _) = reservation.into_owner_parts();
        stack.resolve_tcp_receive(reservation_id, 7).unwrap();

        let rolled_back = stack.reserve_tcp_receive(accepted, 64).unwrap();
        assert_eq!(rolled_back.bytes(), &[0x5a; 25]);
        let (rolled_back_id, _) = rolled_back.into_owner_parts();
        stack.resolve_tcp_receive(rolled_back_id, 0).unwrap();
        let repeated = stack.reserve_tcp_receive(accepted, 64).unwrap();
        assert_eq!(repeated.bytes(), &[0x5a; 25]);
        let (repeated_id, _) = repeated.into_owner_parts();
        assert_eq!(
            stack.resolve_tcp_receive(repeated_id, 26),
            Err(TcpReceiveResolveError::InvalidPrefix)
        );
        stack.resolve_tcp_receive(repeated_id, 25).unwrap();

        let _ = stack.send_tcp_endpoint(accepted, b"final").unwrap().1;
        drive(&mut stack, interface, 384);
        let outstanding = stack.reserve_tcp_receive(clients[0], 5).unwrap();
        assert_eq!(outstanding.bytes(), b"final");
        let (outstanding_id, _) = outstanding.into_owner_parts();
        stack.retire_tcp_endpoint(clients[0]).unwrap();
        assert!(matches!(
            stack.observe_tcp_connection(clients[0]).unwrap(),
            TcpConnectionObservation::Connected { .. }
        ));
        stack.resolve_tcp_receive(outstanding_id, 3).unwrap();
        drive(&mut stack, interface, 512);
        assert_eq!(
            stack.observe_tcp_connection(clients[0]),
            Err(TcpQueryError::UnknownEndpoint)
        );

        let cancelled = stack
            .claim_tcp_pending_child(listener)
            .unwrap()
            .expect("another completed child must remain");
        let _ = stack.cancel_tcp_child(cancelled).unwrap();
        drive(&mut stack, interface, 640);
        assert!(stack.claim_tcp_pending_child(listener).unwrap().is_some());
    }

    #[test]
    fn every_engine_can_enter_preallocated_reclaim_without_cleanup_failure() {
        const CAPACITY: usize = 12;
        let policy = TcpPolicy::new(16, CAPACITY, 10, 16, 16, CAPACITY, 41000, 41015);
        let (mut stack, interface) = test_stack(policy);
        let mut endpoints = Vec::new();
        for port in 42000..42000 + CAPACITY as u16 {
            let endpoint = stack.create_tcp_endpoint().unwrap();
            let _ = stack
                .start_tcp_connect(
                    endpoint,
                    Ipv4EgressSelection::new(interface, LOCAL),
                    TcpPeer::new(LOCAL, port),
                )
                .unwrap();
            endpoints.push(endpoint);
        }
        assert_eq!(stack.protocols.tcp.engine_count, CAPACITY);
        for endpoint in endpoints {
            assert!(stack.retire_tcp_endpoint(endpoint).unwrap().is_some());
        }
        assert_eq!(stack.protocols.tcp.deferred.len(), CAPACITY);
        let while_reclaiming = stack.create_tcp_endpoint().unwrap();
        assert!(matches!(
            stack.bind_tcp_endpoint(
                while_reclaiming,
                TcpBindRequest::new(LOCAL, policy.ephemeral_port_first),
            ),
            Err(anemone_net_api::tcp::TcpBindError::PortInUse)
        ));
        drive(&mut stack, interface, 0);
        assert_eq!(stack.protocols.tcp.engine_count, 0);
        assert!(stack.protocols.tcp.deferred.is_empty());
        assert!(
            stack
                .bind_tcp_endpoint(
                    while_reclaiming,
                    TcpBindRequest::new(LOCAL, policy.ephemeral_port_first),
                )
                .is_ok()
        );
    }

    #[test]
    fn endpoint_engine_and_ephemeral_capacity_fail_typed_and_recover() {
        let policy = TcpPolicy::new(2, 1, 1, 16, 16, 1, 43000, 43000);
        let (mut stack, interface) = test_stack(policy);
        let first = stack.create_tcp_endpoint().unwrap();
        let second = stack.create_tcp_endpoint().unwrap();
        assert_eq!(
            stack.create_tcp_endpoint(),
            Err(anemone_net_api::tcp::TcpCreateError::EndpointCapacity)
        );

        stack
            .bind_tcp_endpoint(first, TcpBindRequest::new(LOCAL, 0))
            .unwrap();
        assert_eq!(
            stack.bind_tcp_endpoint(second, TcpBindRequest::new(LOCAL, 0)),
            Err(anemone_net_api::tcp::TcpBindError::EphemeralPortsExhausted)
        );
        stack.retire_tcp_endpoint(first).unwrap();
        stack
            .bind_tcp_endpoint(second, TcpBindRequest::new(LOCAL, 0))
            .unwrap();
        let recovered = stack.create_tcp_endpoint().unwrap();
        stack
            .bind_tcp_endpoint(recovered, TcpBindRequest::new(LOCAL, 43001))
            .unwrap();

        let _ = stack
            .start_tcp_connect(
                second,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, 44000),
            )
            .unwrap();
        assert_eq!(
            stack.start_tcp_connect(
                recovered,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, 44001),
            ),
            Err(anemone_net_api::tcp::TcpConnectError::EngineCapacity)
        );
        stack.retire_tcp_endpoint(second).unwrap();
        drive(&mut stack, interface, 0);
        assert!(
            stack
                .start_tcp_connect(
                    recovered,
                    Ipv4EgressSelection::new(interface, LOCAL),
                    TcpPeer::new(LOCAL, 44001),
                )
                .is_ok()
        );
    }

    #[test]
    fn listener_retirement_subsumes_pending_child_rearm_before_releasing_port() {
        let (mut stack, interface) = test_stack(POLICY);
        let listener = stack.create_tcp_endpoint().unwrap();
        stack
            .bind_tcp_endpoint(listener, TcpBindRequest::new(LOCAL, LISTEN_PORT))
            .unwrap();
        stack
            .listen_tcp_endpoint(listener, interface, Ipv4Address::UNSPECIFIED)
            .unwrap();
        let client = stack.create_tcp_endpoint().unwrap();
        let _ = stack
            .start_tcp_connect(
                client,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, LISTEN_PORT),
            )
            .unwrap();
        drive(&mut stack, interface, 0);

        let child = stack
            .claim_tcp_pending_child(listener)
            .unwrap()
            .expect("completed child must be claimable");
        assert!(stack.cancel_tcp_child(child).unwrap().is_some());
        assert_eq!(stack.protocols.tcp.deferred.len(), 1);
        assert!(stack.retire_tcp_endpoint(listener).unwrap().is_none());

        let contender = stack.create_tcp_endpoint().unwrap();
        assert!(matches!(
            stack.bind_tcp_endpoint(contender, TcpBindRequest::new(LOCAL, LISTEN_PORT)),
            Err(anemone_net_api::tcp::TcpBindError::PortInUse)
        ));
        drive(&mut stack, interface, 128);
        assert!(
            stack
                .bind_tcp_endpoint(contender, TcpBindRequest::new(LOCAL, LISTEN_PORT))
                .is_ok()
        );
    }

    #[test]
    fn claimed_child_cancel_does_not_depend_on_completed_state_remaining_stable() {
        let (mut stack, interface) = test_stack(POLICY);
        let listener = stack.create_tcp_endpoint().unwrap();
        stack
            .bind_tcp_endpoint(listener, TcpBindRequest::new(LOCAL, LISTEN_PORT))
            .unwrap();
        stack
            .listen_tcp_endpoint(listener, interface, Ipv4Address::UNSPECIFIED)
            .unwrap();
        let client = stack.create_tcp_endpoint().unwrap();
        let _ = stack
            .start_tcp_connect(
                client,
                Ipv4EgressSelection::new(interface, LOCAL),
                TcpPeer::new(LOCAL, LISTEN_PORT),
            )
            .unwrap();
        drive(&mut stack, interface, 0);

        let child = stack
            .claim_tcp_pending_child(listener)
            .unwrap()
            .expect("completed child must be claimable");
        let (_, slot, _) = child.owner_parts();
        let handle = stack.protocols.tcp.listener(listener).unwrap().slots[slot]
            .handle
            .unwrap();
        stack
            .local
            .as_mut()
            .unwrap()
            .sockets
            .get_mut::<tcp::Socket>(handle)
            .abort();
        let _ = stack.cancel_tcp_child(child).unwrap();
        drive(&mut stack, interface, 128);
        assert!(stack.protocols.tcp.deferred.is_empty());
        let slot = &stack.protocols.tcp.listener(listener).unwrap().slots[slot];
        assert!(slot.handle.is_some());
        assert!(!slot.claimed);
    }

    #[test]
    fn connecting_timeout_is_preserved_as_a_typed_owner_observation() {
        let mut device = FaultInjector::new(Loopback::new(Medium::Ip), 1);
        device.set_drop_chance(100);
        let mut interface = Interface::new(
            Config::new(HardwareAddress::Ip),
            &mut device,
            SmoltcpInstant::ZERO,
        );
        interface.update_ip_addrs(|addresses| {
            addresses
                .push(IpCidr::new(IpAddress::v4(10, 0, 0, 1), 24))
                .unwrap();
        });
        let mut sockets = SocketSet::new(Vec::new());
        let mut owner = TcpEndpoints::new(POLICY);
        let endpoint = owner.create_endpoint().unwrap();
        let binding = owner
            .prepare_binding(endpoint, Ipv4Address::new([10, 0, 0, 1]))
            .unwrap();
        let local =
            TcpLocalBinding::from_owner_commit(Ipv4Address::new([10, 0, 0, 1]), binding.port());
        let peer = TcpPeer::new(Ipv4Address::new([10, 0, 0, 2]), 80);
        owner.prepare_connect(endpoint, binding, peer).unwrap();
        let mut socket = tcp_socket(POLICY);
        socket.set_timeout(Some(SmoltcpDuration::from_millis(100)));
        socket
            .connect(
                interface.context(),
                IpEndpoint::new(IpAddress::v4(10, 0, 0, 2), 80),
                IpListenEndpoint {
                    addr: Some(IpAddress::v4(10, 0, 0, 1)),
                    port: local.port(),
                },
            )
            .unwrap();
        let handle = sockets.add(socket);
        owner.commit_connect(
            endpoint,
            InterfaceId::from_index(0),
            handle,
            binding,
            local,
            peer,
        );

        interface.poll(SmoltcpInstant::from_millis(0), &mut device, &mut sockets);
        interface.poll(SmoltcpInstant::from_millis(101), &mut device, &mut sockets);
        assert!(matches!(
            owner
                .connection_observation(&mut sockets, endpoint)
                .unwrap(),
            TcpConnectionObservation::Failed {
                cause: TcpDisconnectCause::Timeout,
                ..
            }
        ));
    }
}
