//! Logical listener, completed-child handoff, and rearm ownership.

use alloc::vec::Vec;

use anemone_net_api::{
    InterfaceId,
    tcp::{
        TcpChildError, TcpEndpointId, TcpListenError, TcpLocalBinding, TcpPeer, TcpPendingChild,
    },
};
use smoltcp::{iface::SocketSet, socket::tcp};

use super::{
    Connection, ConnectionPhase, DeferredReclaim, EndpointRole, Listener, ListenerSlot,
    ReclaimAction, TcpEndpoints, completed_child_state,
};

impl TcpEndpoints {
    pub(crate) fn prepare_listener(
        &self,
        id: TcpEndpointId,
        binding: TcpLocalBinding,
    ) -> Result<(), TcpListenError> {
        match &self
            .endpoint(id)
            .ok_or(TcpListenError::UnknownEndpoint)?
            .role
        {
            EndpointRole::Idle | EndpointRole::Bound(_) => {},
            _ => return Err(TcpListenError::WrongRole),
        }
        if !self.ensure_engine_capacity(self.policy.listener_completed_capacity) {
            return Err(TcpListenError::EngineCapacity);
        }
        assert_eq!(self.current_binding(id).unwrap_or(binding), binding);
        Ok(())
    }

    pub(crate) fn commit_listener(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
        interface: InterfaceId,
        binding: TcpLocalBinding,
    ) {
        let mut slots = Vec::with_capacity(self.policy.listener_completed_capacity);
        for _ in 0..self.policy.listener_completed_capacity {
            slots.push(ListenerSlot {
                generation: 0,
                handle: Some(self.add_listener_engine(sockets, binding)),
                claimed: false,
            });
        }
        self.endpoint_mut(id)
            .expect("prepared TCP listener disappeared before commit")
            .role = EndpointRole::Listener(Listener {
            interface,
            binding,
            slots,
        });
    }

    pub(crate) fn claim_pending_child(
        &mut self,
        sockets: &SocketSet<'static>,
        listener_id: TcpEndpointId,
    ) -> Result<Option<TcpPendingChild>, TcpChildError> {
        let Some(endpoint) = self.endpoint(listener_id) else {
            return Err(TcpChildError::UnknownEndpoint);
        };
        if !matches!(endpoint.role, EndpointRole::Listener(_)) {
            return Err(TcpChildError::WrongRole);
        }
        let listener = self
            .listener_mut(listener_id)
            .expect("checked TCP listener role disappeared before child claim");
        for (slot, entry) in listener.slots.iter_mut().enumerate() {
            let Some(handle) = entry.handle else {
                continue;
            };
            if !entry.claimed && completed_child_state(sockets.get::<tcp::Socket>(handle).state()) {
                entry.claimed = true;
                return Ok(Some(TcpPendingChild::from_owner_observation(
                    listener_id,
                    slot,
                    entry.generation,
                )));
            }
        }
        Ok(None)
    }

    pub(crate) fn take_child(
        &mut self,
        sockets: &mut SocketSet<'static>,
        child: TcpPendingChild,
    ) -> Result<TcpEndpointId, TcpChildError> {
        if !self.ensure_engine_capacity(1) {
            return Err(TcpChildError::EngineCapacity);
        }
        let connection_index = self
            .endpoints
            .iter()
            .position(|slot| matches!(slot.role, EndpointRole::Vacant))
            .ok_or(TcpChildError::EndpointCapacity)?;
        let (listener_id, slot_index, generation) = child.owner_parts();
        let (interface, binding, local, handle, peer) =
            self.completed_child(sockets, listener_id, slot_index, generation)?;

        let replacement = self.add_listener_engine(sockets, binding);
        let listener = self
            .listener_mut(listener_id)
            .expect("completed TCP child lost its listener before handoff");
        let slot = &mut listener.slots[slot_index];
        slot.handle = Some(replacement);
        slot.claimed = false;
        slot.generation = slot
            .generation
            .checked_add(1)
            .expect("TCP listener slot generation exhausted");

        let raw = self.next_endpoint_id;
        self.next_endpoint_id = raw
            .checked_add(1)
            .expect("TCP Endpoint identity namespace exhausted");
        let id = TcpEndpointId::from_owner_raw(raw);
        let endpoint = &mut self.endpoints[connection_index];
        assert!(matches!(endpoint.role, EndpointRole::Vacant));
        endpoint.id = Some(id);
        endpoint.role = EndpointRole::Connection(Connection {
            interface,
            handle,
            binding,
            local,
            peer,
            phase: ConnectionPhase::Connected,
            reservation: None,
            retire_requested: false,
        });
        Ok(id)
    }

    pub(crate) fn cancel_child(
        &mut self,
        sockets: &mut SocketSet<'static>,
        child: TcpPendingChild,
    ) -> Result<Option<InterfaceId>, TcpChildError> {
        let (listener_id, slot_index, generation) = child.owner_parts();
        let (interface, _, handle) = self.claimed_child(listener_id, slot_index, generation)?;
        let has_remote = sockets
            .get::<tcp::Socket>(handle)
            .remote_endpoint()
            .is_some();
        if has_remote {
            sockets.get_mut::<tcp::Socket>(handle).abort();
        }
        let generation = {
            let listener = self
                .listener_mut(listener_id)
                .expect("claimed TCP child lost its listener before cancellation");
            let slot = &mut listener.slots[slot_index];
            slot.handle = None;
            slot.claimed = false;
            slot.generation = slot
                .generation
                .checked_add(1)
                .expect("TCP listener slot generation exhausted");
            slot.generation
        };
        if !has_remote {
            self.remove_engine(sockets, handle);
            self.rearm_listener(sockets, listener_id, slot_index, generation);
            return Ok(None);
        }
        self.queue_reclaim(DeferredReclaim {
            interface,
            handle,
            action: ReclaimAction::RearmListener {
                listener: listener_id,
                slot: slot_index,
                generation,
            },
        });
        Ok(Some(interface))
    }

    fn claimed_child(
        &self,
        listener_id: TcpEndpointId,
        slot_index: usize,
        generation: u64,
    ) -> Result<(InterfaceId, TcpLocalBinding, smoltcp::iface::SocketHandle), TcpChildError> {
        let listener = self.listener(listener_id).ok_or_else(|| {
            if self.endpoint(listener_id).is_some() {
                TcpChildError::WrongRole
            } else {
                TcpChildError::UnknownEndpoint
            }
        })?;
        let slot = listener
            .slots
            .get(slot_index)
            .ok_or(TcpChildError::StaleChild)?;
        if slot.generation != generation || !slot.claimed {
            return Err(TcpChildError::StaleChild);
        }
        let handle = slot.handle.ok_or(TcpChildError::StaleChild)?;
        Ok((listener.interface, listener.binding, handle))
    }

    fn completed_child(
        &self,
        sockets: &SocketSet<'static>,
        listener_id: TcpEndpointId,
        slot_index: usize,
        generation: u64,
    ) -> Result<
        (
            InterfaceId,
            TcpLocalBinding,
            TcpLocalBinding,
            smoltcp::iface::SocketHandle,
            TcpPeer,
        ),
        TcpChildError,
    > {
        let (interface, binding, handle) =
            self.claimed_child(listener_id, slot_index, generation)?;
        let socket = sockets.get::<tcp::Socket>(handle);
        if !completed_child_state(socket.state()) {
            return Err(TcpChildError::ChildNotCompleted);
        }
        let remote = socket
            .remote_endpoint()
            .expect("completed TCP child must have a peer tuple");
        let local = socket
            .local_endpoint()
            .expect("completed TCP child must have a local tuple");
        let remote_address = match remote.addr {
            smoltcp::wire::IpAddress::Ipv4(address) => {
                anemone_net_api::Ipv4Address::new(address.octets())
            },
        };
        let local_address = match local.addr {
            smoltcp::wire::IpAddress::Ipv4(address) => {
                anemone_net_api::Ipv4Address::new(address.octets())
            },
        };
        Ok((
            interface,
            binding,
            TcpLocalBinding::from_owner_commit(local_address, local.port),
            handle,
            TcpPeer::new(remote_address, remote.port),
        ))
    }
}
