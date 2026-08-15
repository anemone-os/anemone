//! Non-blocking endpoint retirement and deferred engine reclaim.

use alloc::vec::Vec;

use anemone_net_api::{
    InterfaceId,
    tcp::{TcpEndpointId, TcpReleaseReason, TcpRetireError},
};
use smoltcp::{iface::SocketSet, socket::tcp, time::Duration};

use super::{
    DeferredReclaim, EndpointRole, Listener, ListenerSlotPhase, ReclaimAction, TcpEndpoints,
    engine_connection_tuple,
};

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

    pub(crate) fn begin_release(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
        reason: TcpReleaseReason,
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
                let handle = connection.handle;
                let has_remote = sockets
                    .get::<tcp::Socket>(handle)
                    .remote_endpoint()
                    .is_some();
                match reason {
                    TcpReleaseReason::FinalRelease => {
                        let socket = sockets.get_mut::<tcp::Socket>(handle);
                        socket.set_timeout(Some(Duration::from_millis(
                            self.policy.orphan_timeout_ms as u64,
                        )));
                        socket.close();
                    },
                    TcpReleaseReason::CreationRollback
                    | TcpReleaseReason::AcceptedChildRollback
                    | TcpReleaseReason::ListenerWithdrawal => {
                        if has_remote {
                            sockets.get_mut::<tcp::Socket>(handle).abort();
                        }
                    },
                }
                if connection.reservation.is_some() {
                    self.connection_mut(id)
                        .expect("retiring TCP connection disappeared")
                        .release_requested = Some(reason);
                    return Ok(has_remote.then_some(interface));
                }
                self.queue_connection_reclaim(sockets, id, reason);
                Ok(has_remote.then_some(interface))
            },
            EndpointRole::Listener(_) => {
                panic!("logical listener release must withdraw every projection together")
            },
            EndpointRole::Reclaiming { .. } | EndpointRole::Vacant => {
                Err(TcpRetireError::UnknownEndpoint)
            },
        }
    }

    pub(crate) fn begin_listener_release(
        &mut self,
        id: TcpEndpointId,
    ) -> Result<Listener, TcpRetireError> {
        let index = self
            .endpoint_index(id)
            .ok_or(TcpRetireError::UnknownEndpoint)?;
        let binding = self
            .listener(id)
            .ok_or(TcpRetireError::UnknownEndpoint)?
            .binding;
        let mut already_deferred = 0;
        for reclaim in &mut self.deferred {
            if matches!(
                &reclaim.action,
                ReclaimAction::RearmListener { listener, .. } if *listener == id
            ) {
                // Withdrawal subsumes earlier candidate/child cleanup. The old
                // engine continues to retain the listener Endpoint and binding.
                reclaim.action = ReclaimAction::ReleaseEndpoint(id);
                already_deferred += 1;
            }
        }
        let role = core::mem::replace(
            &mut self.endpoints[index].role,
            EndpointRole::Reclaiming {
                remaining: already_deferred,
                binding: Some(binding),
            },
        );
        let EndpointRole::Listener(listener) = role else {
            unreachable!("validated TCP listener changed during retirement")
        };
        Ok(listener)
    }

    pub(crate) fn retire_listener_engine(
        &mut self,
        sockets: &mut SocketSet<'static>,
        listener: TcpEndpointId,
        interface: InterfaceId,
        handle: smoltcp::iface::SocketHandle,
        tuple: Option<super::ConnectionTuple>,
    ) -> bool {
        assert_eq!(
            tuple,
            engine_connection_tuple(sockets.get::<tcp::Socket>(handle))
        );
        if tuple.is_none() {
            self.remove_engine(sockets, handle);
            return false;
        }
        sockets.get_mut::<tcp::Socket>(handle).abort();
        let EndpointRole::Reclaiming { remaining, .. } = &mut self
            .endpoint_mut(listener)
            .expect("withdrawn listener lost its Endpoint identity")
            .role
        else {
            panic!("withdrawn listener lost its reclaim owner")
        };
        *remaining = remaining
            .checked_add(1)
            .expect("TCP listener reclaim count overflow");
        self.queue_reclaim(DeferredReclaim {
            interface,
            handle,
            tuple,
            action: ReclaimAction::ReleaseEndpoint(listener),
        });
        true
    }

    pub(crate) fn finish_listener_release(&mut self, id: TcpEndpointId) {
        let EndpointRole::Reclaiming { remaining, .. } = self
            .endpoint(id)
            .expect("withdrawn listener lost its Endpoint identity")
            .role
        else {
            panic!("withdrawn listener lost its reclaim owner")
        };
        if remaining == 0 {
            self.release_endpoint(id);
        }
    }

    pub(crate) fn queue_connection_reclaim(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
        reason: TcpReleaseReason,
    ) {
        let connection = self
            .connection(id)
            .expect("TCP receive resolve lost its retiring connection");
        assert!(connection.reservation.is_none());
        let interface = connection.interface;
        let handle = connection.handle;
        let tuple = super::ConnectionTuple {
            local: connection.local,
            peer: connection.peer,
        };
        if sockets
            .get::<tcp::Socket>(handle)
            .remote_endpoint()
            .is_none()
        {
            self.remove_engine(sockets, handle);
            self.release_endpoint(id);
            return;
        }
        match reason {
            TcpReleaseReason::FinalRelease => {
                let socket = sockets.get_mut::<tcp::Socket>(handle);
                socket.set_timeout(Some(Duration::from_millis(
                    self.policy.orphan_timeout_ms as u64,
                )));
                socket.close();
            },
            TcpReleaseReason::CreationRollback
            | TcpReleaseReason::AcceptedChildRollback
            | TcpReleaseReason::ListenerWithdrawal => {
                sockets.get_mut::<tcp::Socket>(handle).abort()
            },
        }
        self.endpoint_mut(id)
            .expect("retiring TCP connection disappeared before publication")
            .role = EndpointRole::Reclaiming {
            remaining: 1,
            binding: Some(connection.binding),
        };
        self.queue_reclaim(DeferredReclaim {
            interface,
            handle,
            // The active Connection role has ended. Move its exact tuple with
            // the engine so admission observes one reservation truth until
            // TIME_WAIT and all other final protocol work are reclaimed.
            tuple: Some(tuple),
            action: ReclaimAction::ReleaseEndpoint(id),
        });
    }

    pub(crate) fn reclaim_interface(
        &mut self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
    ) -> bool {
        self.refresh_listener_tuples(interface, sockets);
        let progression = self.reconcile_listener_candidates(interface, sockets);
        for index in 0..self.endpoints.len() {
            let endpoint = match &self.endpoints[index] {
                super::EndpointSlot {
                    id: Some(id),
                    role: EndpointRole::Connection(connection),
                    ..
                } if connection.interface == interface => Some(*id),
                _ => None,
            };
            if let Some(endpoint) = endpoint {
                self.refresh_connection(sockets, endpoint)
                    .expect("live TCP connection disappeared during pump refresh");
            }
        }

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
                    projection,
                    slot,
                    generation,
                } => self.rearm_listener(sockets, listener, projection, slot, generation),
                ReclaimAction::ReleaseEndpoint(endpoint) => {
                    self.complete_endpoint_reclaim(endpoint)
                },
            }
        }
        self.rearm_closed_listener_engines(interface, sockets);
        progression
    }

    pub(crate) fn rearm_listener(
        &mut self,
        sockets: &mut SocketSet<'static>,
        listener_id: TcpEndpointId,
        projection_index: usize,
        slot_index: usize,
        generation: u64,
    ) {
        let Some(listener) = self.listener(listener_id) else {
            return;
        };
        let Some(projection) = listener.projections.get(projection_index) else {
            return;
        };
        let Some(slot) = projection.slots.get(slot_index) else {
            return;
        };
        if slot.generation != generation || slot.handle.is_some() {
            return;
        }
        let active = projection
            .slots
            .iter()
            .filter(|slot| slot.handle.is_some())
            .count();
        if active >= listener.backlog {
            return;
        }
        let binding = listener.binding;
        assert!(self.ensure_engine_capacity(1));
        let handle = self.add_listener_engine(sockets, binding);
        let projection = &mut self
            .listener_mut(listener_id)
            .expect("TCP listener changed during exclusive rearm")
            .projections[projection_index];
        let slot = &mut projection.slots[slot_index];
        assert_eq!(slot.generation, generation);
        assert!(slot.handle.is_none());
        slot.handle = Some(handle);
        slot.phase = ListenerSlotPhase::Open;
        slot.tuple = None;
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
            for (projection_index, projection) in listener.projections.iter().enumerate() {
                if projection.interface != interface {
                    continue;
                }
                for (slot_index, slot) in projection.slots.iter().enumerate() {
                    let Some(handle) = slot.handle else {
                        continue;
                    };
                    let socket = sockets.get::<tcp::Socket>(handle);
                    // A closed engine can still owe its final RST. Reuse only
                    // after the private tuple has disappeared.
                    if socket.state() == tcp::State::Closed && socket.remote_endpoint().is_none() {
                        closed.push((
                            id,
                            projection_index,
                            slot_index,
                            slot.generation,
                            listener.binding,
                            handle,
                        ));
                    }
                }
            }
        }

        for (listener_id, projection_index, slot_index, generation, binding, handle) in closed {
            let listener = self
                .listener(listener_id)
                .expect("TCP listener changed during one reclaim window");
            let projection = &listener.projections[projection_index];
            let slot = &projection.slots[slot_index];
            assert_eq!(slot.generation, generation);
            assert_eq!(slot.handle, Some(handle));
            let replace = projection
                .slots
                .iter()
                .filter(|slot| slot.handle.is_some())
                .count()
                .saturating_sub(1)
                < listener.backlog;
            self.remove_engine(sockets, handle);
            let replacement = replace.then(|| self.add_listener_engine(sockets, binding));
            let projection = &mut self
                .listener_mut(listener_id)
                .expect("TCP listener changed during one reclaim window")
                .projections[projection_index];
            let slot = &mut projection.slots[slot_index];
            slot.generation = slot
                .generation
                .checked_add(1)
                .expect("TCP listener slot generation exhausted");
            slot.handle = replacement;
            slot.phase = ListenerSlotPhase::Open;
            slot.tuple = None;
        }
    }

    fn refresh_listener_tuples(&mut self, interface: InterfaceId, sockets: &SocketSet<'static>) {
        for endpoint in &mut self.endpoints {
            let EndpointRole::Listener(listener) = &mut endpoint.role else {
                continue;
            };
            for projection in &mut listener.projections {
                if projection.interface != interface {
                    continue;
                }
                for slot in &mut projection.slots {
                    slot.tuple = slot.handle.and_then(|handle| {
                        engine_connection_tuple(sockets.get::<tcp::Socket>(handle))
                    });
                }
            }
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
        slot.reuse_address = false;
        slot.no_delay = false;
    }
}
