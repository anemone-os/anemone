//! Logical listener, ingress projection, aggregate admission, and child
//! handoff.

use alloc::vec::Vec;

use anemone_net_api::{
    InterfaceId,
    tcp::{
        TcpChildError, TcpEndpointId, TcpListenBacklog, TcpListenError, TcpLocalBinding, TcpPeer,
        TcpPendingChild,
    },
};
use smoltcp::{iface::SocketSet, socket::tcp};

use super::{
    Connection, ConnectionPhase, ConnectionTuple, DeferredReclaim, EndpointRole, Listener,
    ListenerProjection, ListenerSlot, ListenerSlotPhase, ReclaimAction, TcpEndpoints,
    completed_child_state,
};

impl TcpEndpoints {
    pub(crate) fn prepare_listener(
        &self,
        id: TcpEndpointId,
        binding: TcpLocalBinding,
        interfaces: &[InterfaceId],
        backlog: TcpListenBacklog,
    ) -> Result<(), TcpListenError> {
        if backlog.get() > self.policy.listener_completed_capacity
            || interfaces.len() > self.policy.listener_projection_capacity()
        {
            return Err(TcpListenError::EngineCapacity);
        }
        match &self
            .endpoint(id)
            .ok_or(TcpListenError::UnknownEndpoint)?
            .role
        {
            EndpointRole::Idle | EndpointRole::Bound(_) | EndpointRole::Listener(_) => {},
            _ => return Err(TcpListenError::WrongRole),
        }
        if self.listener_binding_conflicts(id, binding) {
            return Err(TcpListenError::PortInUse);
        }

        let additional = if let Some(listener) = self.listener(id) {
            if listener.projections.len() != interfaces.len()
                || listener
                    .projections
                    .iter()
                    .zip(interfaces)
                    .any(|(projection, interface)| projection.interface != *interface)
            {
                return Err(TcpListenError::UnknownInterface);
            }
            listener
                .projections
                .iter()
                .try_fold(0usize, |total, projection| {
                    let active = projection
                        .slots
                        .iter()
                        .filter(|slot| slot.handle.is_some())
                        .count();
                    total.checked_add(backlog.get().saturating_sub(active))
                })
        } else {
            interfaces.len().checked_mul(backlog.get())
        }
        .ok_or(TcpListenError::EngineCapacity)?;
        if !self.ensure_engine_capacity(additional) {
            return Err(TcpListenError::EngineCapacity);
        }
        assert_eq!(self.current_binding(id).unwrap_or(binding), binding);
        Ok(())
    }

    pub(crate) fn new_listener_projection(
        &mut self,
        sockets: &mut SocketSet<'static>,
        interface: InterfaceId,
        binding: TcpLocalBinding,
        backlog: usize,
    ) -> ListenerProjection {
        let mut slots = Vec::with_capacity(self.policy.listener_completed_capacity);
        for _ in 0..backlog {
            slots.push(ListenerSlot {
                generation: 0,
                handle: Some(self.add_listener_engine(sockets, binding)),
                phase: ListenerSlotPhase::Open,
                tuple: None,
            });
        }
        ListenerProjection { interface, slots }
    }

    pub(crate) fn commit_listener(
        &mut self,
        id: TcpEndpointId,
        binding: TcpLocalBinding,
        backlog: usize,
        projections: Vec<ListenerProjection>,
    ) {
        assert!(self.listener(id).is_none());
        self.endpoint_mut(id)
            .expect("prepared TCP listener disappeared before commit")
            .role = EndpointRole::Listener(Listener {
            binding,
            backlog,
            projections,
        });
    }

    pub(crate) fn set_listener_backlog(&mut self, id: TcpEndpointId, backlog: usize) {
        self.listener_mut(id)
            .expect("re-listen owner disappeared")
            .backlog = backlog;
    }

    pub(crate) fn resize_listener_projection(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
        projection_index: usize,
    ) {
        let backlog = self
            .listener(id)
            .expect("re-listen owner disappeared")
            .backlog;
        let mut active = self
            .listener(id)
            .expect("re-listen owner disappeared")
            .projections[projection_index]
            .slots
            .iter()
            .filter(|slot| slot.handle.is_some())
            .count();
        while active > backlog {
            let removable = self
                .listener(id)
                .expect("re-listen owner disappeared")
                .projections[projection_index]
                .slots
                .iter()
                .enumerate()
                .find_map(|(index, slot)| {
                    let handle = slot.handle?;
                    (slot.phase == ListenerSlotPhase::Open
                        && sockets
                            .get::<tcp::Socket>(handle)
                            .remote_endpoint()
                            .is_none())
                    .then_some((index, handle))
                });
            let Some((index, handle)) = removable else {
                break;
            };
            self.remove_engine(sockets, handle);
            let slot = &mut self
                .listener_mut(id)
                .expect("re-listen owner disappeared")
                .projections[projection_index]
                .slots[index];
            assert_eq!(slot.handle, Some(handle));
            assert!(slot.tuple.is_none());
            slot.handle = None;
            slot.generation = slot
                .generation
                .checked_add(1)
                .expect("TCP listener slot generation exhausted");
            active -= 1;
        }

        while active < backlog {
            let binding = self
                .listener(id)
                .expect("re-listen owner disappeared")
                .binding;
            let replacement = self.add_listener_engine(sockets, binding);
            let projection = &mut self
                .listener_mut(id)
                .expect("re-listen owner disappeared")
                .projections[projection_index];
            if let Some(index) = projection
                .slots
                .iter()
                .position(|slot| slot.handle.is_none())
            {
                let slot = &mut projection.slots[index];
                slot.handle = Some(replacement);
                slot.phase = ListenerSlotPhase::Open;
                slot.tuple = None;
            } else {
                projection.slots.push(ListenerSlot {
                    generation: 0,
                    handle: Some(replacement),
                    phase: ListenerSlotPhase::Open,
                    tuple: None,
                });
            }
            active += 1;
        }
    }

    /// Converts transport-completed candidates on one pumped interface into
    /// aggregate logical children, or aborts them when the shared backlog is
    /// full.
    pub(crate) fn reconcile_listener_candidates(
        &mut self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
    ) -> bool {
        let mut candidates = Vec::new();
        for endpoint in &self.endpoints {
            let (Some(listener_id), EndpointRole::Listener(listener)) =
                (endpoint.id, &endpoint.role)
            else {
                continue;
            };
            for (projection_index, projection) in listener.projections.iter().enumerate() {
                if projection.interface != interface {
                    continue;
                }
                for (slot_index, slot) in projection.slots.iter().enumerate() {
                    let Some(handle) = slot.handle else { continue };
                    if slot.phase == ListenerSlotPhase::Open
                        && completed_child_state(sockets.get::<tcp::Socket>(handle).state())
                    {
                        candidates.push((
                            listener_id,
                            projection_index,
                            slot_index,
                            slot.generation,
                            handle,
                        ));
                    }
                }
            }
        }

        let mut progression = false;
        for (listener_id, projection_index, slot_index, generation, handle) in candidates {
            let occupancy = self.listener_occupancy(listener_id);
            let backlog = self
                .listener(listener_id)
                .expect("candidate listener disappeared during one Stack window")
                .backlog;
            if occupancy < backlog {
                let slot = &mut self
                    .listener_mut(listener_id)
                    .expect("candidate listener disappeared during one Stack window")
                    .projections[projection_index]
                    .slots[slot_index];
                assert_eq!(slot.generation, generation);
                assert_eq!(slot.handle, Some(handle));
                assert_eq!(slot.phase, ListenerSlotPhase::Open);
                slot.phase = ListenerSlotPhase::Pending;
                self.invalidate(listener_id);
                continue;
            }

            let tuple = self
                .listener(listener_id)
                .expect("candidate listener disappeared during one Stack window")
                .projections[projection_index]
                .slots[slot_index]
                .tuple;
            assert!(
                tuple.is_some(),
                "completed listener candidate must retain its tuple"
            );
            sockets.get_mut::<tcp::Socket>(handle).abort();
            let next_generation = {
                let slot = &mut self
                    .listener_mut(listener_id)
                    .expect("candidate listener disappeared during one Stack window")
                    .projections[projection_index]
                    .slots[slot_index];
                assert_eq!(slot.generation, generation);
                assert_eq!(slot.handle, Some(handle));
                slot.handle = None;
                slot.phase = ListenerSlotPhase::Open;
                slot.tuple = None;
                slot.generation = slot
                    .generation
                    .checked_add(1)
                    .expect("TCP listener slot generation exhausted");
                slot.generation
            };
            self.queue_reclaim(DeferredReclaim {
                interface,
                handle,
                tuple,
                action: ReclaimAction::RearmListener {
                    listener: listener_id,
                    projection: projection_index,
                    slot: slot_index,
                    generation: next_generation,
                },
            });
            progression = true;
        }
        progression
    }

    pub(crate) fn claim_pending_child(
        &mut self,
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
        for (projection_index, projection) in listener.projections.iter_mut().enumerate() {
            for (slot_index, slot) in projection.slots.iter_mut().enumerate() {
                if slot.phase == ListenerSlotPhase::Pending {
                    slot.phase = ListenerSlotPhase::Claimed;
                    return Ok(Some(TcpPendingChild::from_owner_observation(
                        listener_id,
                        projection_index,
                        slot_index,
                        slot.generation,
                    )));
                }
            }
        }
        Ok(None)
    }

    pub(crate) fn child_interface(
        &self,
        child: TcpPendingChild,
    ) -> Result<InterfaceId, TcpChildError> {
        let (listener, projection, _, _) = child.owner_parts();
        self.listener(listener)
            .and_then(|listener| listener.projections.get(projection))
            .map(|projection| projection.interface)
            .ok_or(TcpChildError::StaleChild)
    }

    pub(crate) fn take_child(
        &mut self,
        sockets: &mut SocketSet<'static>,
        child: TcpPendingChild,
    ) -> Result<TcpEndpointId, TcpChildError> {
        let connection_index = self
            .endpoints
            .iter()
            .position(|slot| matches!(slot.role, EndpointRole::Vacant))
            .ok_or(TcpChildError::EndpointCapacity)?;
        let (listener_id, projection_index, slot_index, generation) = child.owner_parts();
        let (interface, binding, local, handle, peer) = self.completed_child(
            sockets,
            listener_id,
            projection_index,
            slot_index,
            generation,
        )?;
        let listener_reuse = self
            .endpoint(listener_id)
            .expect("completed TCP child lost its listener owner")
            .reuse_address;
        let listener_no_delay = self
            .endpoint(listener_id)
            .expect("completed TCP child lost its listener owner")
            .no_delay;

        let projection = &self
            .listener(listener_id)
            .expect("completed TCP child lost its listener before handoff")
            .projections[projection_index];
        let active = projection
            .slots
            .iter()
            .filter(|slot| slot.handle.is_some())
            .count();
        let backlog = self
            .listener(listener_id)
            .expect("completed TCP child lost its listener before handoff")
            .backlog;
        let replace = active.saturating_sub(1) < backlog;
        if replace && !self.ensure_engine_capacity(1) {
            return Err(TcpChildError::EngineCapacity);
        }
        let replacement = replace.then(|| self.add_listener_engine(sockets, binding));
        let slot = &mut self
            .listener_mut(listener_id)
            .expect("completed TCP child lost its listener before handoff")
            .projections[projection_index]
            .slots[slot_index];
        assert_eq!(slot.tuple, Some(ConnectionTuple { local, peer }));
        slot.handle = replacement;
        slot.phase = ListenerSlotPhase::Open;
        slot.tuple = None;
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
        endpoint.reuse_address = listener_reuse;
        endpoint.no_delay = listener_no_delay;
        sockets
            .get_mut::<tcp::Socket>(handle)
            .set_nagle_enabled(!listener_no_delay);
        endpoint.role = EndpointRole::Connection(Connection {
            interface,
            handle,
            binding,
            local,
            peer,
            phase: ConnectionPhase::Connected,
            was_connected: true,
            pending_error: None,
            read_shutdown: false,
            write_shutdown: false,
            reservation: None,
            release_requested: None,
        });
        Ok(id)
    }

    pub(crate) fn cancel_child(
        &mut self,
        sockets: &mut SocketSet<'static>,
        child: TcpPendingChild,
    ) -> Result<Option<InterfaceId>, TcpChildError> {
        let (listener_id, projection_index, slot_index, generation) = child.owner_parts();
        let (interface, _, handle) =
            self.claimed_child(listener_id, projection_index, slot_index, generation)?;
        let tuple = self
            .listener(listener_id)
            .expect("claimed TCP child lost its listener before cancellation")
            .projections[projection_index]
            .slots[slot_index]
            .tuple;
        let has_remote = sockets
            .get::<tcp::Socket>(handle)
            .remote_endpoint()
            .is_some();
        if has_remote {
            sockets.get_mut::<tcp::Socket>(handle).abort();
        }
        let generation = {
            let slot = &mut self
                .listener_mut(listener_id)
                .expect("claimed TCP child lost its listener before cancellation")
                .projections[projection_index]
                .slots[slot_index];
            slot.handle = None;
            slot.phase = ListenerSlotPhase::Open;
            slot.tuple = None;
            slot.generation = slot
                .generation
                .checked_add(1)
                .expect("TCP listener slot generation exhausted");
            slot.generation
        };
        if !has_remote {
            self.remove_engine(sockets, handle);
            self.rearm_listener(
                sockets,
                listener_id,
                projection_index,
                slot_index,
                generation,
            );
            return Ok(None);
        }
        self.queue_reclaim(DeferredReclaim {
            interface,
            handle,
            tuple,
            action: ReclaimAction::RearmListener {
                listener: listener_id,
                projection: projection_index,
                slot: slot_index,
                generation,
            },
        });
        Ok(Some(interface))
    }

    fn listener_occupancy(&self, listener_id: TcpEndpointId) -> usize {
        self.listener(listener_id)
            .expect("logical listener disappeared during owner admission")
            .projections
            .iter()
            .flat_map(|projection| &projection.slots)
            .filter(|slot| {
                matches!(
                    slot.phase,
                    ListenerSlotPhase::Pending | ListenerSlotPhase::Claimed
                )
            })
            .count()
    }

    fn claimed_child(
        &self,
        listener_id: TcpEndpointId,
        projection_index: usize,
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
        let projection = listener
            .projections
            .get(projection_index)
            .ok_or(TcpChildError::StaleChild)?;
        let slot = projection
            .slots
            .get(slot_index)
            .ok_or(TcpChildError::StaleChild)?;
        if slot.generation != generation || slot.phase != ListenerSlotPhase::Claimed {
            return Err(TcpChildError::StaleChild);
        }
        let handle = slot.handle.ok_or(TcpChildError::StaleChild)?;
        Ok((projection.interface, listener.binding, handle))
    }

    fn completed_child(
        &self,
        sockets: &SocketSet<'static>,
        listener_id: TcpEndpointId,
        projection_index: usize,
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
            self.claimed_child(listener_id, projection_index, slot_index, generation)?;
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
