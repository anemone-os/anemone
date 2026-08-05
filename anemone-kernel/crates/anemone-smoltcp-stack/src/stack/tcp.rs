//! Stack-private TCP endpoint, engine, listener, and reclaim ownership.

use alloc::{vec, vec::Vec};

use anemone_net_api::InterfaceId;
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
}

impl TcpPolicy {
    pub const fn new(
        endpoint_capacity: usize,
        engine_timer_capacity: usize,
        listener_completed_capacity: usize,
        rx_buffer_bytes: usize,
        tx_buffer_bytes: usize,
        deferred_reclaim_capacity: usize,
    ) -> Self {
        Self {
            endpoint_capacity,
            engine_timer_capacity,
            listener_completed_capacity,
            rx_buffer_bytes,
            tx_buffer_bytes,
            deferred_reclaim_capacity,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EndpointId {
    index: usize,
    generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingChild {
    listener: EndpointId,
    slot: usize,
    generation: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OwnerError {
    EndpointFull,
    EngineFull,
    DeferredReclaimFull,
    StaleEndpoint,
    StaleChild,
    WrongRole,
    ChildNotCompleted,
}

struct EndpointSlot {
    generation: u64,
    role: EndpointRole,
}

enum EndpointRole {
    Vacant,
    Idle,
    Listener(Listener),
    Connected(Connection),
    Reclaiming { remaining: usize },
}

struct Listener {
    interface: InterfaceId,
    binding: IpListenEndpoint,
    slots: Vec<ListenerSlot>,
}

struct ListenerSlot {
    generation: u64,
    handle: Option<SocketHandle>,
}

struct Connection {
    interface: InterfaceId,
    handle: SocketHandle,
}

struct DeferredReclaim {
    interface: InterfaceId,
    handle: SocketHandle,
    action: ReclaimAction,
}

enum ReclaimAction {
    RearmListener {
        listener: EndpointId,
        slot: usize,
        generation: u64,
    },
    ReleaseEndpoint(EndpointId),
}

/// The sole private owner of TCP identities and protocol-engine resources.
///
/// Stage 1 intentionally has no kernel operation consumer. These methods stay
/// Stack-private until Stage 2 defines a narrow typed surface; they are not a
/// probe facade and no smoltcp handle crosses this owner boundary.
#[allow(dead_code)]
pub(crate) struct TcpEndpoints {
    policy: TcpPolicy,
    endpoints: Vec<EndpointSlot>,
    engine_count: usize,
    deferred: Vec<DeferredReclaim>,
}

#[allow(dead_code)]
impl TcpEndpoints {
    pub(crate) fn new(policy: TcpPolicy) -> Self {
        let mut endpoints = Vec::with_capacity(policy.endpoint_capacity);
        for _ in 0..policy.endpoint_capacity {
            endpoints.push(EndpointSlot {
                generation: 0,
                role: EndpointRole::Vacant,
            });
        }
        Self {
            policy,
            endpoints,
            engine_count: 0,
            deferred: Vec::with_capacity(policy.deferred_reclaim_capacity),
        }
    }

    fn create_endpoint(&mut self) -> Result<EndpointId, OwnerError> {
        let (index, slot) = self
            .endpoints
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| matches!(slot.role, EndpointRole::Vacant))
            .ok_or(OwnerError::EndpointFull)?;
        slot.role = EndpointRole::Idle;
        Ok(EndpointId {
            index,
            generation: slot.generation,
        })
    }

    fn listen(
        &mut self,
        sockets: &mut SocketSet<'static>,
        endpoint: EndpointId,
        interface: InterfaceId,
        port: u16,
    ) -> Result<(), OwnerError> {
        if !matches!(self.endpoint(endpoint)?.role, EndpointRole::Idle) {
            return Err(OwnerError::WrongRole);
        }
        let capacity = self.policy.listener_completed_capacity;
        self.ensure_engine_capacity(capacity)?;
        let binding = IpListenEndpoint::from(port);
        let mut slots = Vec::with_capacity(capacity);
        for _ in 0..capacity {
            slots.push(ListenerSlot {
                generation: 0,
                handle: Some(self.add_listener_engine(sockets, binding)),
            });
        }
        self.endpoint_mut(endpoint)?.role = EndpointRole::Listener(Listener {
            interface,
            binding,
            slots,
        });
        Ok(())
    }

    fn pending_children(
        &self,
        sockets: &SocketSet<'static>,
        listener: EndpointId,
    ) -> Result<Vec<PendingChild>, OwnerError> {
        let EndpointRole::Listener(listener_role) = &self.endpoint(listener)?.role else {
            return Err(OwnerError::WrongRole);
        };
        Ok(listener_role
            .slots
            .iter()
            .enumerate()
            .filter_map(|(slot, entry)| {
                let handle = entry.handle?;
                completed_child_state(sockets.get::<tcp::Socket>(handle).state()).then_some(
                    PendingChild {
                        listener,
                        slot,
                        generation: entry.generation,
                    },
                )
            })
            .collect())
    }

    fn take_child(
        &mut self,
        sockets: &mut SocketSet<'static>,
        child: PendingChild,
    ) -> Result<EndpointId, OwnerError> {
        self.ensure_engine_capacity(1)?;
        let connection_index = self.vacant_endpoint_index()?;
        let (interface, binding, handle) = self.completed_child(sockets, child)?;

        let replacement = self.add_listener_engine(sockets, binding);
        let listener = self.listener_mut(child.listener)?;
        let slot = &mut listener.slots[child.slot];
        slot.handle = Some(replacement);
        slot.generation = slot
            .generation
            .checked_add(1)
            .expect("TCP listener slot generation exhausted");

        let connection = &mut self.endpoints[connection_index];
        assert!(matches!(connection.role, EndpointRole::Vacant));
        connection.role = EndpointRole::Connected(Connection { interface, handle });
        Ok(EndpointId {
            index: connection_index,
            generation: connection.generation,
        })
    }

    fn cancel_child(
        &mut self,
        sockets: &mut SocketSet<'static>,
        child: PendingChild,
    ) -> Result<(), OwnerError> {
        if self.deferred.len() == self.policy.deferred_reclaim_capacity {
            return Err(OwnerError::DeferredReclaimFull);
        }
        let (interface, _, handle) = self.completed_child(sockets, child)?;
        sockets.get_mut::<tcp::Socket>(handle).abort();
        let generation = {
            let listener = self.listener_mut(child.listener)?;
            let slot = &mut listener.slots[child.slot];
            slot.handle = None;
            slot.generation = slot
                .generation
                .checked_add(1)
                .expect("TCP listener slot generation exhausted");
            slot.generation
        };
        self.deferred.push(DeferredReclaim {
            interface,
            handle,
            action: ReclaimAction::RearmListener {
                listener: child.listener,
                slot: child.slot,
                generation,
            },
        });
        Ok(())
    }

    fn take_disconnect_reason(
        &mut self,
        sockets: &mut SocketSet<'static>,
        endpoint: EndpointId,
    ) -> Result<Option<tcp::DisconnectReason>, OwnerError> {
        let connection = self.connection(endpoint)?;
        Ok(sockets
            .get_mut::<tcp::Socket>(connection.handle)
            .take_disconnect_reason())
    }

    fn connection_remote_port(
        &self,
        sockets: &SocketSet<'static>,
        endpoint: EndpointId,
    ) -> Result<Option<u16>, OwnerError> {
        let connection = self.connection(endpoint)?;
        Ok(sockets
            .get::<tcp::Socket>(connection.handle)
            .remote_endpoint()
            .map(|peer| peer.port))
    }

    fn retire_connection(
        &mut self,
        sockets: &mut SocketSet<'static>,
        endpoint: EndpointId,
    ) -> Result<(), OwnerError> {
        let connection = *self.connection(endpoint)?;
        if sockets
            .get::<tcp::Socket>(connection.handle)
            .remote_endpoint()
            .is_none()
        {
            self.remove_engine(sockets, connection.handle);
            self.release_endpoint(endpoint);
            return Ok(());
        }
        if self.deferred.len() == self.policy.deferred_reclaim_capacity {
            return Err(OwnerError::DeferredReclaimFull);
        }
        sockets.get_mut::<tcp::Socket>(connection.handle).abort();
        self.endpoint_mut(endpoint)?.role = EndpointRole::Reclaiming { remaining: 1 };
        self.deferred.push(DeferredReclaim {
            interface: connection.interface,
            handle: connection.handle,
            action: ReclaimAction::ReleaseEndpoint(endpoint),
        });
        Ok(())
    }

    fn retire_listener(
        &mut self,
        sockets: &mut SocketSet<'static>,
        endpoint: EndpointId,
    ) -> Result<(), OwnerError> {
        let (interface, handles) = {
            let EndpointRole::Listener(listener) = &self.endpoint(endpoint)?.role else {
                return Err(OwnerError::WrongRole);
            };
            (
                listener.interface,
                listener
                    .slots
                    .iter()
                    .filter_map(|slot| slot.handle)
                    .collect::<Vec<_>>(),
            )
        };
        let deferred_needed = handles
            .iter()
            .filter(|handle| {
                sockets
                    .get::<tcp::Socket>(**handle)
                    .remote_endpoint()
                    .is_some()
            })
            .count();
        if self.deferred.len() + deferred_needed > self.policy.deferred_reclaim_capacity {
            return Err(OwnerError::DeferredReclaimFull);
        }

        self.endpoint_mut(endpoint)?.role = EndpointRole::Reclaiming {
            remaining: deferred_needed,
        };
        for handle in handles {
            if sockets
                .get::<tcp::Socket>(handle)
                .remote_endpoint()
                .is_some()
            {
                sockets.get_mut::<tcp::Socket>(handle).abort();
                self.deferred.push(DeferredReclaim {
                    interface,
                    handle,
                    action: ReclaimAction::ReleaseEndpoint(endpoint),
                });
            } else {
                self.remove_engine(sockets, handle);
            }
        }
        if deferred_needed == 0 {
            self.release_endpoint(endpoint);
        }
        Ok(())
    }

    pub(crate) fn reclaim_interface(
        &mut self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
    ) {
        let mut index = 0;
        while index < self.deferred.len() {
            if self.deferred[index].interface != interface
                || sockets
                    .get::<tcp::Socket>(self.deferred[index].handle)
                    .remote_endpoint()
                    .is_some()
            {
                index += 1;
                continue;
            }
            let reclaim = self.deferred.remove(index);
            self.remove_engine(sockets, reclaim.handle);
            match reclaim.action {
                ReclaimAction::RearmListener {
                    listener,
                    slot,
                    generation,
                } => self.rearm_listener(sockets, listener, slot, generation),
                ReclaimAction::ReleaseEndpoint(endpoint) => {
                    self.complete_endpoint_reclaim(endpoint)
                },
            }
        }
        self.rearm_closed_listener_engines(interface, sockets);
    }

    fn rearm_closed_listener_engines(
        &mut self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
    ) {
        let mut closed = Vec::new();
        for (endpoint_index, endpoint) in self.endpoints.iter().enumerate() {
            let EndpointRole::Listener(listener) = &endpoint.role else {
                continue;
            };
            if listener.interface != interface {
                continue;
            }
            for (slot_index, slot) in listener.slots.iter().enumerate() {
                let Some(handle) = slot.handle else {
                    continue;
                };
                let socket = sockets.get::<tcp::Socket>(handle);
                // A closed engine may still owe its final RST to the provider.
                // Keep it attached until the tuple disappears, then replace
                // the whole generation instead of manufacturing a second
                // pending-child or half-open state alongside smoltcp.
                if socket.state() == tcp::State::Closed && socket.remote_endpoint().is_none() {
                    closed.push((
                        EndpointId {
                            index: endpoint_index,
                            generation: endpoint.generation,
                        },
                        slot_index,
                        slot.generation,
                        listener.binding,
                        handle,
                    ));
                }
            }
        }

        for (listener_id, slot_index, generation, binding, handle) in closed {
            let listener = self
                .listener(listener_id)
                .expect("TCP listener changed during one exclusive reclaim window");
            let slot = &listener.slots[slot_index];
            assert_eq!(slot.generation, generation);
            assert_eq!(slot.handle, Some(handle));

            self.remove_engine(sockets, handle);
            let replacement = self.add_listener_engine(sockets, binding);
            let listener = self
                .listener_mut(listener_id)
                .expect("TCP listener changed during one exclusive reclaim window");
            let slot = &mut listener.slots[slot_index];
            slot.generation = slot
                .generation
                .checked_add(1)
                .expect("TCP listener slot generation exhausted");
            slot.handle = Some(replacement);
        }
    }

    fn completed_child(
        &self,
        sockets: &SocketSet<'static>,
        child: PendingChild,
    ) -> Result<(InterfaceId, IpListenEndpoint, SocketHandle), OwnerError> {
        let listener = self.listener(child.listener)?;
        let slot = listener
            .slots
            .get(child.slot)
            .ok_or(OwnerError::StaleChild)?;
        if slot.generation != child.generation {
            return Err(OwnerError::StaleChild);
        }
        let handle = slot.handle.ok_or(OwnerError::StaleChild)?;
        if !completed_child_state(sockets.get::<tcp::Socket>(handle).state()) {
            return Err(OwnerError::ChildNotCompleted);
        }
        Ok((listener.interface, listener.binding, handle))
    }

    fn rearm_listener(
        &mut self,
        sockets: &mut SocketSet<'static>,
        listener_id: EndpointId,
        slot_index: usize,
        generation: u64,
    ) {
        let Ok(listener) = self.listener(listener_id) else {
            return;
        };
        let Some(slot) = listener.slots.get(slot_index) else {
            return;
        };
        if slot.generation != generation || slot.handle.is_some() {
            return;
        }
        let binding = listener.binding;
        assert!(self.engine_count < self.policy.engine_timer_capacity);
        let handle = self.add_listener_engine(sockets, binding);
        let listener = self
            .listener_mut(listener_id)
            .expect("listener changed while rearming without an intervening owner operation");
        let slot = &mut listener.slots[slot_index];
        assert_eq!(slot.generation, generation);
        assert!(slot.handle.is_none());
        slot.handle = Some(handle);
    }

    fn complete_endpoint_reclaim(&mut self, endpoint: EndpointId) {
        let slot = self
            .endpoint_mut(endpoint)
            .expect("deferred TCP endpoint generation changed before reclaim");
        let EndpointRole::Reclaiming { remaining } = &mut slot.role else {
            panic!("deferred TCP reclaim lost its endpoint owner");
        };
        assert!(*remaining > 0);
        *remaining -= 1;
        if *remaining == 0 {
            self.release_endpoint(endpoint);
        }
    }

    fn release_endpoint(&mut self, endpoint: EndpointId) {
        let slot = self
            .endpoint_mut(endpoint)
            .expect("TCP endpoint generation changed before release");
        slot.generation = slot
            .generation
            .checked_add(1)
            .expect("TCP endpoint generation exhausted");
        slot.role = EndpointRole::Vacant;
    }

    fn add_listener_engine(
        &mut self,
        sockets: &mut SocketSet<'static>,
        binding: IpListenEndpoint,
    ) -> SocketHandle {
        let mut socket = tcp_socket(self.policy);
        socket
            .listen(binding)
            .expect("kernel-validated TCP listener binding must be valid");
        let handle = sockets.add(socket);
        self.engine_count += 1;
        assert!(self.engine_count <= self.policy.engine_timer_capacity);
        handle
    }

    fn remove_engine(&mut self, sockets: &mut SocketSet<'static>, handle: SocketHandle) {
        match sockets.remove(handle) {
            socket::Socket::Tcp(_) => {},
            _ => unreachable!("TCP owner only records TCP engine handles"),
        }
        self.engine_count = self
            .engine_count
            .checked_sub(1)
            .expect("TCP engine count underflow");
    }

    fn ensure_engine_capacity(&self, additional: usize) -> Result<(), OwnerError> {
        if self
            .engine_count
            .checked_add(additional)
            .is_none_or(|count| count > self.policy.engine_timer_capacity)
        {
            return Err(OwnerError::EngineFull);
        }
        Ok(())
    }

    fn vacant_endpoint_index(&self) -> Result<usize, OwnerError> {
        self.endpoints
            .iter()
            .position(|slot| matches!(slot.role, EndpointRole::Vacant))
            .ok_or(OwnerError::EndpointFull)
    }

    fn endpoint(&self, endpoint: EndpointId) -> Result<&EndpointSlot, OwnerError> {
        self.endpoints
            .get(endpoint.index)
            .filter(|slot| slot.generation == endpoint.generation)
            .ok_or(OwnerError::StaleEndpoint)
    }

    fn endpoint_mut(&mut self, endpoint: EndpointId) -> Result<&mut EndpointSlot, OwnerError> {
        self.endpoints
            .get_mut(endpoint.index)
            .filter(|slot| slot.generation == endpoint.generation)
            .ok_or(OwnerError::StaleEndpoint)
    }

    fn listener(&self, endpoint: EndpointId) -> Result<&Listener, OwnerError> {
        let EndpointRole::Listener(listener) = &self.endpoint(endpoint)?.role else {
            return Err(OwnerError::WrongRole);
        };
        Ok(listener)
    }

    fn listener_mut(&mut self, endpoint: EndpointId) -> Result<&mut Listener, OwnerError> {
        let EndpointRole::Listener(listener) = &mut self.endpoint_mut(endpoint)?.role else {
            return Err(OwnerError::WrongRole);
        };
        Ok(listener)
    }

    fn connection(&self, endpoint: EndpointId) -> Result<&Connection, OwnerError> {
        let EndpointRole::Connected(connection) = &self.endpoint(endpoint)?.role else {
            return Err(OwnerError::WrongRole);
        };
        Ok(connection)
    }
}

impl Copy for Connection {}

impl Clone for Connection {
    fn clone(&self) -> Self {
        *self
    }
}

fn tcp_socket(policy: TcpPolicy) -> tcp::Socket<'static> {
    tcp::Socket::new(
        tcp::SocketBuffer::new(vec![0; policy.rx_buffer_bytes]),
        tcp::SocketBuffer::new(vec![0; policy.tx_buffer_bytes]),
    )
}

fn completed_child_state(state: tcp::State) -> bool {
    matches!(state, tcp::State::Established | tcp::State::CloseWait)
}

#[cfg(test)]
mod tests {
    use smoltcp::{
        iface::{Config, Interface},
        phy::{Loopback, Medium},
        time::Instant,
        wire::{HardwareAddress, IpAddress, IpCidr},
    };

    use super::*;

    const LOCAL_ADDRESS: IpAddress = IpAddress::v4(127, 0, 0, 1);
    const LISTEN_PORT: u16 = 2345;
    const COMPLETED_CAPACITY: usize = 10;
    const POLICY: TcpPolicy = TcpPolicy::new(32, 64, COMPLETED_CAPACITY, 128, 128, 32);

    fn add_client(
        interface: &mut Interface,
        sockets: &mut SocketSet<'static>,
        source_port: u16,
    ) -> SocketHandle {
        let mut socket = tcp_socket(POLICY);
        socket
            .connect(
                interface.context(),
                (LOCAL_ADDRESS, LISTEN_PORT),
                source_port,
            )
            .unwrap();
        sockets.add(socket)
    }

    fn drive(
        interface: &mut Interface,
        device: &mut Loopback,
        sockets: &mut SocketSet<'static>,
        first_tick: i64,
    ) {
        for tick in first_tick..first_tick + 64 {
            interface.poll(Instant::from_millis(tick), device, sockets);
        }
    }

    #[test]
    fn listener_completion_generation_cause_and_reclaim_share_one_bounded_owner() {
        let interface_id = InterfaceId::from_index(0);
        let mut device = Loopback::new(Medium::Ip);
        let mut interface =
            Interface::new(Config::new(HardwareAddress::Ip), &mut device, Instant::ZERO);
        interface.update_ip_addrs(|addresses| {
            addresses.push(IpCidr::new(LOCAL_ADDRESS, 8)).unwrap();
        });
        let mut sockets = SocketSet::new(Vec::new());
        let mut owner = TcpEndpoints::new(POLICY);
        let listener = owner.create_endpoint().unwrap();
        owner
            .listen(&mut sockets, listener, interface_id, LISTEN_PORT)
            .unwrap();

        let mut clients = Vec::new();
        for port in 32000..32000 + COMPLETED_CAPACITY as u16 {
            clients.push(add_client(&mut interface, &mut sockets, port));
        }
        drive(&mut interface, &mut device, &mut sockets, 0);
        assert_eq!(
            owner.pending_children(&sockets, listener).unwrap().len(),
            COMPLETED_CAPACITY
        );

        let rejected = add_client(&mut interface, &mut sockets, 32100);
        drive(&mut interface, &mut device, &mut sockets, 64);
        let rejected = sockets.get_mut::<tcp::Socket>(rejected);
        assert_eq!(rejected.state(), tcp::State::Closed);
        assert_eq!(
            rejected.take_disconnect_reason(),
            Some(tcp::DisconnectReason::Reset)
        );

        let child = owner.pending_children(&sockets, listener).unwrap()[0];
        let child_handle = owner.listener(listener).unwrap().slots[child.slot]
            .handle
            .unwrap();
        let peer_port = sockets
            .get::<tcp::Socket>(child_handle)
            .remote_endpoint()
            .unwrap()
            .port;
        let peer = clients
            .iter()
            .copied()
            .find(|handle| {
                sockets
                    .get::<tcp::Socket>(*handle)
                    .local_endpoint()
                    .is_some_and(|endpoint| endpoint.port == peer_port)
            })
            .unwrap();
        sockets.get_mut::<tcp::Socket>(peer).close();
        drive(&mut interface, &mut device, &mut sockets, 128);
        assert_eq!(
            sockets.get::<tcp::Socket>(child_handle).state(),
            tcp::State::CloseWait
        );
        let connected = owner.take_child(&mut sockets, child).unwrap();
        assert_eq!(
            owner.connection_remote_port(&sockets, connected).unwrap(),
            Some(peer_port)
        );
        sockets.get_mut::<tcp::Socket>(peer).abort();
        drive(&mut interface, &mut device, &mut sockets, 192);
        assert_eq!(
            owner
                .take_disconnect_reason(&mut sockets, connected)
                .unwrap(),
            Some(tcp::DisconnectReason::Reset)
        );
        assert_eq!(
            owner
                .take_disconnect_reason(&mut sockets, connected)
                .unwrap(),
            None
        );
        owner.retire_connection(&mut sockets, connected).unwrap();
        assert_eq!(
            owner.connection_remote_port(&sockets, connected),
            Err(OwnerError::StaleEndpoint)
        );

        let replacement = add_client(&mut interface, &mut sockets, 32101);
        drive(&mut interface, &mut device, &mut sockets, 256);
        assert_eq!(
            sockets.get::<tcp::Socket>(replacement).state(),
            tcp::State::Established
        );
        assert_eq!(
            owner.pending_children(&sockets, listener).unwrap().len(),
            COMPLETED_CAPACITY
        );
        assert_eq!(
            owner.cancel_child(&mut sockets, child),
            Err(OwnerError::StaleChild)
        );

        let reset_before_take = owner.pending_children(&sockets, listener).unwrap()[1];
        let reset_handle = owner.listener(listener).unwrap().slots[reset_before_take.slot]
            .handle
            .unwrap();
        let reset_peer_port = sockets
            .get::<tcp::Socket>(reset_handle)
            .remote_endpoint()
            .unwrap()
            .port;
        let reset_peer = clients
            .iter()
            .copied()
            .find(|handle| {
                sockets
                    .get::<tcp::Socket>(*handle)
                    .local_endpoint()
                    .is_some_and(|endpoint| endpoint.port == reset_peer_port)
            })
            .unwrap();
        sockets.get_mut::<tcp::Socket>(reset_peer).abort();
        drive(&mut interface, &mut device, &mut sockets, 320);
        owner.reclaim_interface(interface_id, &mut sockets);
        assert_eq!(
            owner.cancel_child(&mut sockets, reset_before_take),
            Err(OwnerError::StaleChild)
        );
        let post_reset = add_client(&mut interface, &mut sockets, 32102);
        drive(&mut interface, &mut device, &mut sockets, 384);
        assert_eq!(
            sockets.get::<tcp::Socket>(post_reset).state(),
            tcp::State::Established
        );
        assert_eq!(
            owner.pending_children(&sockets, listener).unwrap().len(),
            COMPLETED_CAPACITY
        );

        let cancelled = owner.pending_children(&sockets, listener).unwrap()[1];
        owner.cancel_child(&mut sockets, cancelled).unwrap();
        assert_eq!(
            owner.pending_children(&sockets, listener).unwrap().len(),
            COMPLETED_CAPACITY - 1
        );
        drive(&mut interface, &mut device, &mut sockets, 448);
        owner.reclaim_interface(interface_id, &mut sockets);
        let post_cancel = add_client(&mut interface, &mut sockets, 32103);
        drive(&mut interface, &mut device, &mut sockets, 512);
        assert_eq!(
            sockets.get::<tcp::Socket>(post_cancel).state(),
            tcp::State::Established
        );
        assert_eq!(
            owner.pending_children(&sockets, listener).unwrap().len(),
            COMPLETED_CAPACITY
        );

        owner.retire_listener(&mut sockets, listener).unwrap();
        drive(&mut interface, &mut device, &mut sockets, 576);
        owner.reclaim_interface(interface_id, &mut sockets);
        assert_eq!(owner.engine_count, 0);
        assert_eq!(
            owner.pending_children(&sockets, listener),
            Err(OwnerError::StaleEndpoint)
        );
        let reused = owner.create_endpoint().unwrap();
        assert_eq!(reused.index, listener.index);
        assert_ne!(reused.generation, listener.generation);
    }
}
