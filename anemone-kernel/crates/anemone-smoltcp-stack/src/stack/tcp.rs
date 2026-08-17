//! Aggregate Stack operations for the private TCP owner.

use anemone_net_api::{
    InterfaceId, Ipv4Address, Ipv4EgressSelection,
    tcp::{
        TcpBindError, TcpBindRequest, TcpChildError, TcpConnectError, TcpConnectResult,
        TcpCreateError, TcpDiagnosticRecord, TcpEndpointFacts, TcpEndpointId, TcpListenBacklog,
        TcpListenError, TcpLocalBinding, TcpPeer, TcpPendingChild, TcpPendingError, TcpQueryError,
        TcpReceiveMode, TcpReceiveReservationId, TcpReceiveResolveError, TcpReleaseReason,
        TcpRetireError, TcpShutdownDirection, TcpShutdownError, TcpShutdownOutcome,
        TcpStreamObservation, TcpStreamReceiveError, TcpStreamReceiveOutcome, TcpStreamSendError,
    },
};
use smoltcp::{
    iface::{Interface, SocketSet},
    wire::{IpAddress, IpEndpoint, IpListenEndpoint, Ipv4Address as SmoltcpIpv4Address},
};

use crate::tcp::{TcpEndpoints, apply_socket_budgets, tcp_socket};

use super::{ProtocolProgression, Stack};

impl Stack {
    pub fn tcp_diagnostic_records(&self) -> alloc::vec::Vec<TcpDiagnosticRecord> {
        let interfaces = &self.interfaces;
        let local = &self.local;
        self.protocols.tcp.diagnostic_records(|id| {
            interfaces
                .iter()
                .find(|entry| entry.id == id)
                .map(|entry| &entry.sockets)
                .or_else(|| {
                    local
                        .as_ref()
                        .filter(|entry| entry.id == id)
                        .map(|entry| &entry.sockets)
                })
        })
    }

    pub fn create_tcp_endpoint(&mut self) -> Result<TcpEndpointId, TcpCreateError> {
        let endpoint = self.protocols.tcp.create_endpoint()?;
        self.protocols.tcp.invalidate(endpoint);
        Ok(endpoint)
    }

    pub fn bind_tcp_endpoint(
        &mut self,
        id: TcpEndpointId,
        request: TcpBindRequest,
    ) -> Result<TcpLocalBinding, TcpBindError> {
        let binding = self.protocols.tcp.bind_endpoint(id, request)?;
        self.protocols.tcp.invalidate(id);
        Ok(binding)
    }

    pub fn tcp_endpoint_binding(
        &self,
        id: TcpEndpointId,
    ) -> Result<Option<TcpLocalBinding>, TcpQueryError> {
        let Some(endpoint) = self.protocols.tcp.endpoint(id) else {
            return Err(TcpQueryError::UnknownEndpoint);
        };
        if matches!(endpoint.role, crate::tcp::EndpointRole::Reclaiming { .. }) {
            return Err(TcpQueryError::UnknownEndpoint);
        }
        Ok(self.protocols.tcp.current_binding(id))
    }

    pub fn tcp_endpoint_facts(&self, id: TcpEndpointId) -> Result<TcpEndpointFacts, TcpQueryError> {
        let endpoint = self
            .protocols
            .tcp
            .endpoint(id)
            .ok_or(TcpQueryError::UnknownEndpoint)?;
        let interface = match &endpoint.role {
            crate::tcp::EndpointRole::Listener(_) => None,
            crate::tcp::EndpointRole::Connection(connection) => Some(connection.interface),
            crate::tcp::EndpointRole::Idle | crate::tcp::EndpointRole::Bound(_) => None,
            crate::tcp::EndpointRole::Reclaiming { .. } | crate::tcp::EndpointRole::Vacant => {
                return Err(TcpQueryError::UnknownEndpoint);
            },
        };
        let sockets = interface.and_then(|interface| self.tcp_sockets(interface));
        self.protocols.tcp.endpoint_facts(sockets, id)
    }

    pub fn tcp_reuse_address(&self, id: TcpEndpointId) -> Result<bool, TcpBindError> {
        self.protocols.tcp.reuse_address(id)
    }

    pub fn set_tcp_reuse_address(
        &mut self,
        id: TcpEndpointId,
        enabled: bool,
    ) -> Result<(), TcpBindError> {
        let result = self.protocols.tcp.set_reuse_address(id, enabled);
        if result.is_ok() {
            self.protocols.tcp.invalidate(id);
        }
        result
    }

    pub fn tcp_no_delay(&self, id: TcpEndpointId) -> Result<bool, TcpQueryError> {
        self.protocols.tcp.no_delay(id)
    }

    pub fn set_tcp_no_delay(
        &mut self,
        id: TcpEndpointId,
        enabled: bool,
    ) -> Result<Option<ProtocolProgression>, TcpQueryError> {
        let interface = self
            .protocols
            .tcp
            .connection(id)
            .map(|connection| connection.interface);
        let Some(interface) = interface else {
            self.protocols.tcp.set_no_delay(None, id, enabled)?;
            self.protocols.tcp.invalidate(id);
            return Ok(None);
        };
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP connection references an attached interface");
        let progression = tcp_owner
            .set_no_delay(Some(sockets), id, enabled)?
            .map(ProtocolProgression::committed);
        tcp_owner.invalidate(id);
        Ok(progression)
    }

    pub fn tcp_receive_buffer(&self, id: TcpEndpointId) -> Result<usize, TcpQueryError> {
        self.tcp_buffer_budget(id, TcpBufferDirection::Receive)
    }

    pub fn tcp_send_buffer(&self, id: TcpEndpointId) -> Result<usize, TcpQueryError> {
        self.tcp_buffer_budget(id, TcpBufferDirection::Send)
    }

    pub fn set_tcp_receive_buffer_hint(
        &mut self,
        id: TcpEndpointId,
        requested: usize,
    ) -> Result<alloc::vec::Vec<ProtocolProgression>, TcpQueryError> {
        self.set_tcp_buffer_hint(id, requested, TcpBufferDirection::Receive)
    }

    pub fn set_tcp_send_buffer_hint(
        &mut self,
        id: TcpEndpointId,
        requested: usize,
    ) -> Result<alloc::vec::Vec<ProtocolProgression>, TcpQueryError> {
        self.set_tcp_buffer_hint(id, requested, TcpBufferDirection::Send)
    }

    fn tcp_buffer_budget(
        &self,
        id: TcpEndpointId,
        direction: TcpBufferDirection,
    ) -> Result<usize, TcpQueryError> {
        let endpoint = self
            .protocols
            .tcp
            .endpoint(id)
            .ok_or(TcpQueryError::UnknownEndpoint)?;
        if matches!(
            endpoint.role,
            crate::tcp::EndpointRole::Vacant | crate::tcp::EndpointRole::Reclaiming { .. }
        ) {
            return Err(TcpQueryError::UnknownEndpoint);
        }
        Ok(match direction {
            TcpBufferDirection::Receive => endpoint.receive_budget,
            TcpBufferDirection::Send => endpoint.send_budget,
        })
    }

    fn set_tcp_buffer_hint(
        &mut self,
        id: TcpEndpointId,
        requested: usize,
        direction: TcpBufferDirection,
    ) -> Result<alloc::vec::Vec<ProtocolProgression>, TcpQueryError> {
        let policy = self.protocols.tcp.policy();
        let budget = match direction {
            TcpBufferDirection::Receive => policy.normalize_receive_buffer_hint(requested),
            TcpBufferDirection::Send => policy.normalize_send_buffer_hint(requested),
        };
        let targets = {
            let endpoint = self
                .protocols
                .tcp
                .endpoint(id)
                .ok_or(TcpQueryError::UnknownEndpoint)?;
            match &endpoint.role {
                crate::tcp::EndpointRole::Connection(connection) => {
                    alloc::vec![(connection.interface, connection.handle)]
                },
                crate::tcp::EndpointRole::Listener(listener) => listener
                    .projections
                    .iter()
                    .flat_map(|projection| {
                        projection.slots.iter().filter_map(|slot| {
                            slot.handle.map(|handle| (projection.interface, handle))
                        })
                    })
                    .collect(),
                crate::tcp::EndpointRole::Idle | crate::tcp::EndpointRole::Bound(_) => {
                    alloc::vec::Vec::new()
                },
                crate::tcp::EndpointRole::Vacant | crate::tcp::EndpointRole::Reclaiming { .. } => {
                    return Err(TcpQueryError::UnknownEndpoint);
                },
            }
        };
        let endpoint = self
            .protocols
            .tcp
            .endpoint_mut(id)
            .expect("validated TCP buffer owner disappeared before mutation");
        match direction {
            TcpBufferDirection::Receive => endpoint.receive_budget = budget,
            TcpBufferDirection::Send => endpoint.send_budget = budget,
        }

        let mut progressed = alloc::vec::Vec::new();
        let mut progressed_interfaces = alloc::vec::Vec::new();
        for (interface, handle) in targets {
            let (_, _, sockets) = self
                .tcp_owner_interface_mut(interface)
                .expect("live TCP buffer target references an attached interface");
            let socket = sockets.get_mut::<smoltcp::socket::tcp::Socket>(handle);
            match direction {
                TcpBufferDirection::Receive => socket.set_recv_capacity_limit(budget),
                TcpBufferDirection::Send => socket.set_send_capacity_limit(budget),
            }
            if direction == TcpBufferDirection::Receive
                && !progressed_interfaces.contains(&interface)
            {
                progressed_interfaces.push(interface);
                progressed.push(ProtocolProgression::committed(interface));
            }
        }
        self.protocols.tcp.invalidate(id);
        Ok(progressed)
    }

    pub fn start_tcp_connect(
        &mut self,
        id: TcpEndpointId,
        selection: Ipv4EgressSelection,
        peer: TcpPeer,
    ) -> Result<ProtocolProgression, TcpConnectError> {
        let source = selection.source();
        self.validate_tcp_selection(selection.interface(), source)?;
        let (binding, local) =
            self.protocols
                .tcp
                .prepare_connect(id, selection.interface(), source, peer)?;
        let policy = self.protocols.tcp.policy();
        let no_delay = self
            .protocols
            .tcp
            .no_delay(id)
            .map_err(|_| TcpConnectError::UnknownEndpoint)?;
        let endpoint = self
            .protocols
            .tcp
            .endpoint(id)
            .ok_or(TcpConnectError::UnknownEndpoint)?;
        let (receive_budget, send_budget) = (endpoint.receive_budget, endpoint.send_budget);
        let (tcp_owner, interface, sockets) = self
            .tcp_owner_interface_mut(selection.interface())
            .ok_or(TcpConnectError::UnknownInterface)?;
        let mut socket = tcp_socket(policy);
        apply_socket_budgets(&mut socket, receive_budget, send_budget);
        socket.set_timeout(Some(smoltcp::time::Duration::from_millis(
            policy.connect_timeout_ms() as u64,
        )));
        socket.set_nagle_enabled(!no_delay);
        socket
            .connect(
                interface.context(),
                IpEndpoint::new(to_smoltcp_address(peer.address()), peer.port()),
                IpListenEndpoint {
                    addr: Some(to_smoltcp_address(local.address())),
                    port: local.port(),
                },
            )
            .expect("owner-validated TCP connect tuple must be accepted by the engine");
        let handle = sockets.add(socket);
        tcp_owner.commit_connect(id, selection.interface(), handle, binding, local, peer);
        tcp_owner.invalidate(id);
        Ok(ProtocolProgression::committed(selection.interface()))
    }

    pub fn tcp_endpoint_is_listening(&self, id: TcpEndpointId) -> Result<bool, TcpQueryError> {
        let endpoint = self
            .protocols
            .tcp
            .endpoint(id)
            .ok_or(TcpQueryError::UnknownEndpoint)?;
        if matches!(endpoint.role, crate::tcp::EndpointRole::Reclaiming { .. }) {
            return Err(TcpQueryError::UnknownEndpoint);
        }
        Ok(matches!(
            endpoint.role,
            crate::tcp::EndpointRole::Listener(_)
        ))
    }

    pub fn tcp_endpoint_peer(&self, id: TcpEndpointId) -> Result<Option<TcpPeer>, TcpQueryError> {
        let endpoint = self
            .protocols
            .tcp
            .endpoint(id)
            .ok_or(TcpQueryError::UnknownEndpoint)?;
        match &endpoint.role {
            crate::tcp::EndpointRole::Connection(connection) => Ok(Some(connection.peer)),
            crate::tcp::EndpointRole::Reclaiming { .. } => Err(TcpQueryError::UnknownEndpoint),
            _ => Ok(None),
        }
    }

    pub fn tcp_connect_result(
        &mut self,
        id: TcpEndpointId,
    ) -> Result<TcpConnectResult, TcpQueryError> {
        let interface = self
            .protocols
            .tcp
            .connection(id)
            .map(|connection| connection.interface);
        let Some(interface) = interface else {
            return match self.protocols.tcp.endpoint(id) {
                Some(endpoint) => match endpoint.role {
                    crate::tcp::EndpointRole::Idle => Ok(TcpConnectResult::Idle),
                    crate::tcp::EndpointRole::Bound(binding) => {
                        Ok(TcpConnectResult::Bound(binding))
                    },
                    crate::tcp::EndpointRole::Reclaiming { .. } => {
                        Err(TcpQueryError::UnknownEndpoint)
                    },
                    _ => Err(TcpQueryError::WrongRole),
                },
                None => Err(TcpQueryError::UnknownEndpoint),
            };
        };
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP connection references an attached interface");
        let result = tcp_owner.connection_result(sockets, id);
        tcp_owner.invalidate(id);
        result
    }

    pub fn consume_tcp_pending_error(
        &mut self,
        id: TcpEndpointId,
    ) -> Result<Option<TcpPendingError>, TcpQueryError> {
        let interface = match self.protocols.tcp.connection(id) {
            Some(connection) => connection.interface,
            None => {
                let endpoint = self
                    .protocols
                    .tcp
                    .endpoint(id)
                    .ok_or(TcpQueryError::UnknownEndpoint)?;
                return if matches!(endpoint.role, crate::tcp::EndpointRole::Reclaiming { .. }) {
                    Err(TcpQueryError::UnknownEndpoint)
                } else {
                    Ok(None)
                };
            },
        };
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP connection references an attached interface");
        let result = tcp_owner.consume_pending_error(sockets, id);
        tcp_owner.invalidate(id);
        result
    }

    pub fn observe_tcp_stream(
        &mut self,
        id: TcpEndpointId,
    ) -> Result<TcpStreamObservation, TcpQueryError> {
        let interface = self
            .protocols
            .tcp
            .connection(id)
            .ok_or_else(|| {
                if self.protocols.tcp.endpoint(id).is_some() {
                    TcpQueryError::WrongRole
                } else {
                    TcpQueryError::UnknownEndpoint
                }
            })?
            .interface;
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP connection references an attached interface");
        let result = tcp_owner.stream_observation(sockets, id);
        tcp_owner.invalidate(id);
        result
    }

    pub fn listen_tcp_endpoint(
        &mut self,
        id: TcpEndpointId,
        implicit_address: Ipv4Address,
    ) -> Result<(), TcpListenError> {
        let backlog =
            TcpListenBacklog::new(self.protocols.tcp.policy().listener_completed_capacity());
        self.listen_tcp_endpoint_with_backlog(id, implicit_address, backlog)
    }

    pub fn listen_tcp_endpoint_with_backlog(
        &mut self,
        id: TcpEndpointId,
        implicit_address: Ipv4Address,
        backlog: TcpListenBacklog,
    ) -> Result<(), TcpListenError> {
        let binding = self
            .protocols
            .tcp
            .prepare_binding(id, implicit_address)
            .map_err(map_listen_bind_error)?;
        let interfaces = self.tcp_listener_interfaces(binding)?;
        self.protocols
            .tcp
            .prepare_listener(id, binding, &interfaces, backlog)?;

        if self.protocols.tcp.listener(id).is_some() {
            self.protocols.tcp.set_listener_backlog(id, backlog.get());
            for (projection, interface) in interfaces.into_iter().enumerate() {
                let (tcp_owner, _, sockets) = self
                    .tcp_owner_interface_mut(interface)
                    .expect("validated TCP listener interface disappeared");
                tcp_owner.resize_listener_projection(sockets, id, projection);
            }
        } else {
            let mut projections = alloc::vec::Vec::with_capacity(interfaces.len());
            for interface in interfaces {
                let (tcp_owner, _, sockets) = self
                    .tcp_owner_interface_mut(interface)
                    .expect("validated TCP listener interface disappeared");
                projections.push(tcp_owner.new_listener_projection(
                    sockets,
                    id,
                    interface,
                    binding,
                    backlog.get(),
                ));
            }
            self.protocols
                .tcp
                .commit_listener(id, binding, backlog.get(), projections);
        }
        self.protocols.tcp.invalidate(id);
        Ok(())
    }

    pub fn claim_tcp_pending_child(
        &mut self,
        listener: TcpEndpointId,
    ) -> Result<Option<TcpPendingChild>, TcpChildError> {
        let child = self.protocols.tcp.claim_pending_child(listener)?;
        if child.is_some() {
            self.protocols.tcp.invalidate(listener);
        }
        Ok(child)
    }

    pub fn take_tcp_child(
        &mut self,
        child: TcpPendingChild,
    ) -> Result<TcpEndpointId, TcpChildError> {
        let (listener, _, _, _) = child.owner_parts();
        let interface = self.protocols.tcp.child_interface(child)?;
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP listener references an attached interface");
        let endpoint = tcp_owner.take_child(sockets, child)?;
        tcp_owner.invalidate(listener);
        tcp_owner.invalidate(endpoint);
        Ok(endpoint)
    }

    pub fn cancel_tcp_child(
        &mut self,
        child: TcpPendingChild,
    ) -> Result<Option<ProtocolProgression>, TcpChildError> {
        let (listener, _, _, _) = child.owner_parts();
        let interface = self.protocols.tcp.child_interface(child)?;
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP listener references an attached interface");
        let interface = tcp_owner.cancel_child(sockets, child)?;
        tcp_owner.invalidate(listener);
        Ok(interface.map(ProtocolProgression::committed))
    }

    pub fn send_tcp_stream(
        &mut self,
        id: TcpEndpointId,
        bytes: &[u8],
    ) -> Result<(usize, Option<ProtocolProgression>), TcpStreamSendError> {
        let interface = self
            .protocols
            .tcp
            .connection(id)
            .ok_or_else(|| {
                if self.protocols.tcp.endpoint(id).is_some() {
                    TcpStreamSendError::NotConnected
                } else {
                    TcpStreamSendError::UnknownEndpoint
                }
            })?
            .interface;
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP connection references an attached interface");
        let result = tcp_owner.send_stream(sockets, id, bytes);
        tcp_owner.invalidate(id);
        let accepted = result?;
        Ok((
            accepted,
            (accepted != 0).then(|| ProtocolProgression::committed(interface)),
        ))
    }

    pub fn receive_tcp_stream(
        &mut self,
        id: TcpEndpointId,
        maximum: usize,
        mode: TcpReceiveMode,
    ) -> Result<TcpStreamReceiveOutcome, TcpStreamReceiveError> {
        let interface = self
            .protocols
            .tcp
            .connection(id)
            .ok_or_else(|| {
                if self.protocols.tcp.endpoint(id).is_some() {
                    TcpStreamReceiveError::NotConnected
                } else {
                    TcpStreamReceiveError::UnknownEndpoint
                }
            })?
            .interface;
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP connection references an attached interface");
        let result = tcp_owner.receive_stream(sockets, id, maximum, mode);
        tcp_owner.invalidate(id);
        result
    }

    pub fn shutdown_tcp_endpoint(
        &mut self,
        id: TcpEndpointId,
        direction: TcpShutdownDirection,
    ) -> Result<(TcpShutdownOutcome, Option<ProtocolProgression>), TcpShutdownError> {
        let interface = self
            .protocols
            .tcp
            .connection(id)
            .ok_or_else(|| {
                if self.protocols.tcp.endpoint(id).is_some() {
                    TcpShutdownError::NotConnected
                } else {
                    TcpShutdownError::UnknownEndpoint
                }
            })?
            .interface;
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP connection references an attached interface");
        let result = tcp_owner.shutdown(sockets, id, direction);
        tcp_owner.invalidate(id);
        let (outcome, progression) = result?;
        Ok((outcome, progression.map(ProtocolProgression::committed)))
    }

    pub fn resolve_tcp_receive(
        &mut self,
        reservation: TcpReceiveReservationId,
        committed: usize,
    ) -> Result<Option<ProtocolProgression>, TcpReceiveResolveError> {
        let interface = self
            .protocols
            .tcp
            .reservation_interface(reservation)
            .ok_or(TcpReceiveResolveError::UnknownReservation)?;
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP reservation references an attached interface");
        let result = tcp_owner.resolve_receive(sockets, reservation, committed);
        tcp_owner.invalidate_interface(interface);
        let progression = result?;
        Ok(progression.map(ProtocolProgression::committed))
    }

    pub fn release_tcp_endpoint(
        &mut self,
        id: TcpEndpointId,
        reason: TcpReleaseReason,
    ) -> Result<alloc::vec::Vec<ProtocolProgression>, TcpRetireError> {
        self.protocols.tcp.invalidate(id);
        if self.protocols.tcp.listener(id).is_some() {
            let listener = self.protocols.tcp.begin_listener_release(id)?;
            let mut affected = alloc::vec::Vec::new();
            for projection in listener.projections {
                let interface = projection.interface;
                let (tcp_owner, _, sockets) = self
                    .tcp_owner_interface_mut(interface)
                    .expect("live TCP listener references an attached interface");
                let mut progressed = false;
                for slot in projection.slots {
                    let Some(handle) = slot.handle else { continue };
                    progressed |= tcp_owner
                        .retire_listener_engine(sockets, id, interface, handle, slot.tuple);
                }
                if progressed {
                    affected.push(ProtocolProgression::committed(interface));
                }
            }
            self.protocols.tcp.finish_listener_release(id);
            return Ok(affected);
        }
        let interface = self
            .protocols
            .tcp
            .connection(id)
            .map(|connection| connection.interface);
        let Some(interface) = interface else {
            return self
                .protocols
                .tcp
                .retire_without_engine(id)
                .map(|()| alloc::vec::Vec::new());
        };
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP Endpoint references an attached interface");
        Ok(tcp_owner
            .begin_release(sockets, id, reason)?
            .map(ProtocolProgression::committed)
            .into_iter()
            .collect())
    }

    fn validate_tcp_selection(
        &self,
        interface: InterfaceId,
        source: Ipv4Address,
    ) -> Result<(), TcpConnectError> {
        let source = SmoltcpIpv4Address::from_octets(source.octets());
        let (supported, _) = self
            .interface_ipv4_and_mtu(interface, source)
            .ok_or(TcpConnectError::UnknownInterface)?;
        if !supported {
            return Err(TcpConnectError::UnsupportedSource);
        }
        Ok(())
    }

    fn tcp_owner_interface_mut(
        &mut self,
        id: InterfaceId,
    ) -> Option<(&mut TcpEndpoints, &mut Interface, &mut SocketSet<'static>)> {
        let tcp = &mut self.protocols.tcp;
        if let Some(entry) = self.interfaces.iter_mut().find(|entry| entry.id == id) {
            return Some((tcp, &mut entry.interface, &mut entry.sockets));
        }
        self.local
            .as_mut()
            .filter(|local| local.id == id)
            .map(|local| (tcp, &mut local.interface, &mut local.sockets))
    }

    fn tcp_sockets(&self, id: InterfaceId) -> Option<&SocketSet<'static>> {
        self.interfaces
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| &entry.sockets)
            .or_else(|| {
                self.local
                    .as_ref()
                    .filter(|local| local.id == id)
                    .map(|local| &local.sockets)
            })
    }

    fn tcp_listener_interfaces(
        &self,
        binding: TcpLocalBinding,
    ) -> Result<alloc::vec::Vec<InterfaceId>, TcpListenError> {
        let local = self
            .local
            .as_ref()
            .ok_or(TcpListenError::UnknownInterface)?;
        let mut interfaces = alloc::vec![local.id];
        if binding.address().is_loopback() {
            return Ok(interfaces);
        }
        for entry in &self.interfaces {
            let matches = binding.address().is_unspecified()
                || entry.interface.ip_addrs().iter().any(|cidr| match cidr {
                    smoltcp::wire::IpCidr::Ipv4(cidr) => {
                        cidr.address().octets() == binding.address().octets()
                    },
                });
            let configured = entry
                .interface
                .ip_addrs()
                .iter()
                .any(|cidr| matches!(cidr, smoltcp::wire::IpCidr::Ipv4(_)));
            if configured && matches {
                interfaces.push(entry.id);
            }
        }
        if !binding.address().is_unspecified() && interfaces.len() == 1 {
            return Err(TcpListenError::UnknownInterface);
        }
        Ok(interfaces)
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum TcpBufferDirection {
    Receive,
    Send,
}

fn map_listen_bind_error(error: TcpBindError) -> TcpListenError {
    match error {
        TcpBindError::UnknownEndpoint => TcpListenError::UnknownEndpoint,
        TcpBindError::WrongRole => TcpListenError::WrongRole,
        TcpBindError::PortInUse => TcpListenError::PortInUse,
        TcpBindError::EphemeralPortsExhausted => TcpListenError::EphemeralPortsExhausted,
    }
}

fn to_smoltcp_address(address: Ipv4Address) -> IpAddress {
    IpAddress::Ipv4(SmoltcpIpv4Address::from_octets(address.octets()))
}
