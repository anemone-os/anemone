//! Non-blocking endpoint retirement and deferred engine reclaim.

use alloc::vec::Vec;

use anemone_net_api::{
    InterfaceId,
    tcp::{TcpEndpointId, TcpRetireError},
};
use smoltcp::{iface::SocketSet, socket::tcp};

use super::{DeferredReclaim, EndpointRole, ReclaimAction, TcpEndpoints};

impl TcpEndpoints {
    pub(crate) fn retire_without_engine(
        &mut self,
        id: TcpEndpointId,
    ) -> Result<(), TcpRetireError> {
        let Some(endpoint) = self.endpoint(id) else {
            return Err(TcpRetireError::UnknownEndpoint);
        };
        if !matches!(endpoint.role, EndpointRole::Idle | EndpointRole::Bound(_)) {
            return Err(TcpRetireError::UnknownEndpoint);
        }
        self.release_endpoint(id);
        Ok(())
    }

    pub(crate) fn retire_endpoint(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
    ) -> Result<Option<InterfaceId>, TcpRetireError> {
        let Some(index) = self.endpoint_index(id) else {
            return Err(TcpRetireError::UnknownEndpoint);
        };
        match &self.endpoints[index].role {
            EndpointRole::Idle | EndpointRole::Bound(_) => {
                self.release_endpoint(id);
                Ok(None)
            },
            EndpointRole::Connection(connection) => {
                let interface = connection.interface;
                if connection.reservation.is_some() {
                    let handle = connection.handle;
                    let has_remote = sockets
                        .get::<tcp::Socket>(handle)
                        .remote_endpoint()
                        .is_some();
                    if has_remote {
                        sockets.get_mut::<tcp::Socket>(handle).abort();
                    }
                    self.connection_mut(id)
                        .expect("retiring TCP connection disappeared")
                        .retire_requested = true;
                    return Ok(has_remote.then_some(interface));
                }
                let has_remote = sockets
                    .get::<tcp::Socket>(connection.handle)
                    .remote_endpoint()
                    .is_some();
                self.queue_connection_reclaim(sockets, id);
                Ok(has_remote.then_some(interface))
            },
            EndpointRole::Listener(listener) => {
                let interface = listener.interface;
                let binding = listener.binding;
                let newly_deferred = listener
                    .slots
                    .iter()
                    .filter_map(|slot| slot.handle)
                    .filter(|handle| {
                        sockets
                            .get::<tcp::Socket>(*handle)
                            .remote_endpoint()
                            .is_some()
                    })
                    .count();
                let mut already_deferred = 0;
                for reclaim in &mut self.deferred {
                    if matches!(
                        &reclaim.action,
                        ReclaimAction::RearmListener { listener, .. } if *listener == id
                    ) {
                        // Retirement subsumes an earlier child cancellation.
                        // Keep that engine in the Endpoint reclaim count so
                        // its binding cannot be reused before final cleanup.
                        reclaim.action = ReclaimAction::ReleaseEndpoint(id);
                        already_deferred += 1;
                    }
                }
                let deferred_needed = newly_deferred + already_deferred;
                let role = core::mem::replace(
                    &mut self.endpoints[index].role,
                    EndpointRole::Reclaiming {
                        remaining: deferred_needed,
                        binding: Some(binding),
                    },
                );
                let EndpointRole::Listener(listener) = role else {
                    unreachable!("validated TCP listener changed during retirement")
                };
                // Move handles out of the old role instead of allocating a
                // temporary collection in final-release cleanup.
                for slot in listener.slots {
                    let Some(handle) = slot.handle else {
                        continue;
                    };
                    if sockets
                        .get::<tcp::Socket>(handle)
                        .remote_endpoint()
                        .is_some()
                    {
                        sockets.get_mut::<tcp::Socket>(handle).abort();
                        self.queue_reclaim(DeferredReclaim {
                            interface,
                            handle,
                            action: ReclaimAction::ReleaseEndpoint(id),
                        });
                    } else {
                        self.remove_engine(sockets, handle);
                    }
                }
                if deferred_needed == 0 {
                    self.release_endpoint(id);
                }
                Ok((newly_deferred != 0).then_some(interface))
            },
            EndpointRole::Reclaiming { .. } | EndpointRole::Vacant => {
                Err(TcpRetireError::UnknownEndpoint)
            },
        }
    }

    pub(crate) fn queue_connection_reclaim(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
    ) {
        let connection = self
            .connection(id)
            .expect("TCP receive resolve lost its retiring connection");
        assert!(connection.reservation.is_none());
        let interface = connection.interface;
        let handle = connection.handle;
        if sockets
            .get::<tcp::Socket>(handle)
            .remote_endpoint()
            .is_none()
        {
            self.remove_engine(sockets, handle);
            self.release_endpoint(id);
            return;
        }
        sockets.get_mut::<tcp::Socket>(handle).abort();
        self.endpoint_mut(id)
            .expect("retiring TCP connection disappeared before publication")
            .role = EndpointRole::Reclaiming {
            remaining: 1,
            binding: Some(connection.binding),
        };
        self.queue_reclaim(DeferredReclaim {
            interface,
            handle,
            action: ReclaimAction::ReleaseEndpoint(id),
        });
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

    pub(crate) fn rearm_listener(
        &mut self,
        sockets: &mut SocketSet<'static>,
        listener_id: TcpEndpointId,
        slot_index: usize,
        generation: u64,
    ) {
        let Some(listener) = self.listener(listener_id) else {
            return;
        };
        let Some(slot) = listener.slots.get(slot_index) else {
            return;
        };
        if slot.generation != generation || slot.handle.is_some() {
            return;
        }
        let binding = listener.binding;
        assert!(self.ensure_engine_capacity(1));
        let handle = self.add_listener_engine(sockets, binding);
        let listener = self
            .listener_mut(listener_id)
            .expect("TCP listener changed during exclusive rearm");
        let slot = &mut listener.slots[slot_index];
        assert_eq!(slot.generation, generation);
        assert!(slot.handle.is_none());
        slot.handle = Some(handle);
        slot.claimed = false;
    }

    fn rearm_closed_listener_engines(
        &mut self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
    ) {
        let mut closed = Vec::new();
        for endpoint in &self.endpoints {
            let Some(id) = endpoint.id else {
                continue;
            };
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
                // A closed engine can still owe its final RST. Reuse only
                // after the private tuple has disappeared.
                if socket.state() == tcp::State::Closed && socket.remote_endpoint().is_none() {
                    closed.push((id, slot_index, slot.generation, listener.binding, handle));
                }
            }
        }

        for (listener_id, slot_index, generation, binding, handle) in closed {
            let listener = self
                .listener(listener_id)
                .expect("TCP listener changed during one reclaim window");
            let slot = &listener.slots[slot_index];
            assert_eq!(slot.generation, generation);
            assert_eq!(slot.handle, Some(handle));
            self.remove_engine(sockets, handle);
            let replacement = self.add_listener_engine(sockets, binding);
            let listener = self
                .listener_mut(listener_id)
                .expect("TCP listener changed during one reclaim window");
            let slot = &mut listener.slots[slot_index];
            slot.generation = slot
                .generation
                .checked_add(1)
                .expect("TCP listener slot generation exhausted");
            slot.handle = Some(replacement);
            slot.claimed = false;
        }
    }

    fn complete_endpoint_reclaim(&mut self, id: TcpEndpointId) {
        let slot = self
            .endpoint_mut(id)
            .expect("deferred TCP reclaim lost its Endpoint identity");
        let EndpointRole::Reclaiming { remaining, .. } = &mut slot.role else {
            panic!("deferred TCP reclaim lost its endpoint owner");
        };
        assert!(*remaining > 0);
        *remaining -= 1;
        if *remaining == 0 {
            self.release_endpoint(id);
        }
    }

    fn release_endpoint(&mut self, id: TcpEndpointId) {
        let slot = self
            .endpoint_mut(id)
            .expect("TCP Endpoint disappeared before owner release");
        slot.role = EndpointRole::Vacant;
        slot.id = None;
    }
}
