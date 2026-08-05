//! Aggregate Stack operations for the private TCP owner.

use anemone_net_api::{
    InterfaceId, Ipv4Address, Ipv4EgressSelection,
    tcp::{
        TcpBindError, TcpBindRequest, TcpChildError, TcpConnectError, TcpConnectResult,
        TcpConnectionObservation, TcpCreateError, TcpEndpointId, TcpListenBacklog, TcpListenError,
        TcpLocalBinding, TcpPeer, TcpPendingChild, TcpPendingError, TcpQueryError, TcpReceiveError,
        TcpReceiveMode, TcpReceiveReservation, TcpReceiveReservationId, TcpReceiveResolveError,
        TcpReleaseReason, TcpRetireError, TcpSendError, TcpShutdownDirection, TcpShutdownError,
        TcpShutdownOutcome, TcpStreamObservation, TcpStreamReceiveError, TcpStreamReceiveOutcome,
        TcpStreamSendError,
    },
};
use smoltcp::{
    iface::{Interface, SocketSet},
    wire::{IpAddress, IpEndpoint, IpListenEndpoint, Ipv4Address as SmoltcpIpv4Address},
};

use crate::tcp::{TcpEndpoints, tcp_socket};

use super::{ProtocolProgression, Stack};

impl Stack {
    pub fn create_tcp_endpoint(&mut self) -> Result<TcpEndpointId, TcpCreateError> {
        self.protocols.tcp.create_endpoint()
    }

    pub fn bind_tcp_endpoint(
        &mut self,
        id: TcpEndpointId,
        request: TcpBindRequest,
    ) -> Result<TcpLocalBinding, TcpBindError> {
        self.protocols.tcp.bind_endpoint(id, request)
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

    pub fn tcp_reuse_address(&self, id: TcpEndpointId) -> Result<bool, TcpBindError> {
        self.protocols.tcp.reuse_address(id)
    }

    pub fn set_tcp_reuse_address(
        &mut self,
        id: TcpEndpointId,
        enabled: bool,
    ) -> Result<(), TcpBindError> {
        self.protocols.tcp.set_reuse_address(id, enabled)
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
            return Ok(None);
        };
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP connection references an attached interface");
        Ok(tcp_owner
            .set_no_delay(Some(sockets), id, enabled)?
            .map(ProtocolProgression::committed))
    }

    pub fn start_tcp_connect(
        &mut self,
        id: TcpEndpointId,
        selection: Ipv4EgressSelection,
        peer: TcpPeer,
    ) -> Result<ProtocolProgression, TcpConnectError> {
        let source = selection.source();
        self.validate_tcp_selection(selection.interface(), source)?;
        let (binding, local) = self.protocols.tcp.prepare_connect(id, source, peer)?;
        let policy = self.protocols.tcp.policy();
        let no_delay = self
            .protocols
            .tcp
            .no_delay(id)
            .map_err(|_| TcpConnectError::UnknownEndpoint)?;
        let (tcp_owner, interface, sockets) = self
            .tcp_owner_interface_mut(selection.interface())
            .ok_or(TcpConnectError::UnknownInterface)?;
        let mut socket = tcp_socket(policy);
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
        Ok(ProtocolProgression::committed(selection.interface()))
    }

    pub fn observe_tcp_connection(
        &mut self,
        id: TcpEndpointId,
    ) -> Result<TcpConnectionObservation, TcpQueryError> {
        let interface = match self.protocols.tcp.connection(id) {
            Some(connection) => Some(connection.interface),
            None => None,
        };
        let Some(interface_id) = interface else {
            return match self.protocols.tcp.endpoint(id) {
                Some(endpoint) => match endpoint.role {
                    crate::tcp::EndpointRole::Idle => Ok(TcpConnectionObservation::Idle),
                    crate::tcp::EndpointRole::Bound(binding) => {
                        Ok(TcpConnectionObservation::Bound(binding))
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
            .tcp_owner_interface_mut(interface_id)
            .expect("live TCP connection references an attached interface");
        tcp_owner.connection_observation(sockets, id)
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
        tcp_owner.connection_result(sockets, id)
    }

    pub fn consume_tcp_pending_error(
        &mut self,
        id: TcpEndpointId,
    ) -> Result<Option<TcpPendingError>, TcpQueryError> {
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
        tcp_owner.consume_pending_error(sockets, id)
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
        tcp_owner.stream_observation(sockets, id)
    }

    pub fn listen_tcp_endpoint(
        &mut self,
        id: TcpEndpointId,
        interface: InterfaceId,
        implicit_address: Ipv4Address,
    ) -> Result<(), TcpListenError> {
        let backlog =
            TcpListenBacklog::new(self.protocols.tcp.policy().listener_completed_capacity());
        self.listen_tcp_endpoint_with_backlog(id, interface, implicit_address, backlog)
    }

    pub fn listen_tcp_endpoint_with_backlog(
        &mut self,
        id: TcpEndpointId,
        interface: InterfaceId,
        implicit_address: Ipv4Address,
        backlog: TcpListenBacklog,
    ) -> Result<(), TcpListenError> {
        if self.tcp_sockets(interface).is_none() {
            return Err(TcpListenError::UnknownInterface);
        }
        let binding = self
            .protocols
            .tcp
            .prepare_binding(id, implicit_address)
            .map_err(map_listen_bind_error)?;
        self.protocols.tcp.prepare_listener(id, binding, backlog)?;
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("validated TCP listener interface disappeared");
        tcp_owner.commit_listener(sockets, id, interface, binding, backlog);
        Ok(())
    }

    pub fn claim_tcp_pending_child(
        &mut self,
        listener: TcpEndpointId,
    ) -> Result<Option<TcpPendingChild>, TcpChildError> {
        let interface = self
            .protocols
            .tcp
            .listener(listener)
            .ok_or_else(|| {
                if self.protocols.tcp.endpoint(listener).is_some() {
                    TcpChildError::WrongRole
                } else {
                    TcpChildError::UnknownEndpoint
                }
            })?
            .interface;
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP listener references an attached interface");
        tcp_owner.claim_pending_child(sockets, listener)
    }

    pub fn take_tcp_child(
        &mut self,
        child: TcpPendingChild,
    ) -> Result<TcpEndpointId, TcpChildError> {
        let (listener, _, _) = child.owner_parts();
        let interface = self
            .protocols
            .tcp
            .listener(listener)
            .ok_or(TcpChildError::StaleChild)?
            .interface;
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP listener references an attached interface");
        tcp_owner.take_child(sockets, child)
    }

    pub fn cancel_tcp_child(
        &mut self,
        child: TcpPendingChild,
    ) -> Result<Option<ProtocolProgression>, TcpChildError> {
        let (listener, _, _) = child.owner_parts();
        let interface = self
            .protocols
            .tcp
            .listener(listener)
            .ok_or(TcpChildError::StaleChild)?
            .interface;
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP listener references an attached interface");
        let interface = tcp_owner.cancel_child(sockets, child)?;
        Ok(interface.map(ProtocolProgression::committed))
    }

    pub fn send_tcp_endpoint(
        &mut self,
        id: TcpEndpointId,
        bytes: &[u8],
    ) -> Result<(usize, Option<ProtocolProgression>), TcpSendError> {
        let interface = self
            .protocols
            .tcp
            .connection(id)
            .ok_or_else(|| {
                if self.protocols.tcp.endpoint(id).is_some() {
                    TcpSendError::NotConnected
                } else {
                    TcpSendError::UnknownEndpoint
                }
            })?
            .interface;
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP connection references an attached interface");
        let accepted = tcp_owner.send(sockets, id, bytes)?;
        Ok((
            accepted,
            (accepted != 0).then(|| ProtocolProgression::committed(interface)),
        ))
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
        let accepted = tcp_owner.send_stream(sockets, id, bytes)?;
        Ok((
            accepted,
            (accepted != 0).then(|| ProtocolProgression::committed(interface)),
        ))
    }

    pub fn reserve_tcp_receive(
        &mut self,
        id: TcpEndpointId,
        maximum: usize,
    ) -> Result<TcpReceiveReservation, TcpReceiveError> {
        let interface = self
            .protocols
            .tcp
            .connection(id)
            .ok_or_else(|| {
                if self.protocols.tcp.endpoint(id).is_some() {
                    TcpReceiveError::NotConnected
                } else {
                    TcpReceiveError::UnknownEndpoint
                }
            })?
            .interface;
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP connection references an attached interface");
        tcp_owner.reserve_receive(sockets, id, maximum)
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
        tcp_owner.receive_stream(sockets, id, maximum, mode)
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
        let (outcome, progression) = tcp_owner.shutdown(sockets, id, direction)?;
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
        let progression = tcp_owner.resolve_receive(sockets, reservation, committed)?;
        Ok(progression.map(ProtocolProgression::committed))
    }

    pub fn retire_tcp_endpoint(
        &mut self,
        id: TcpEndpointId,
    ) -> Result<Option<ProtocolProgression>, TcpRetireError> {
        // Stage 2's syscall-unreachable adapter uses this legacy operation for
        // both unpublished and accepted-child rollback. CKPT 3B must replace
        // those call sites with explicit reasons before wiring semantic final
        // release; until then this compatibility bridge must remain aborting.
        self.release_tcp_endpoint(id, TcpReleaseReason::CreationRollback)
    }

    pub fn release_tcp_endpoint(
        &mut self,
        id: TcpEndpointId,
        reason: TcpReleaseReason,
    ) -> Result<Option<ProtocolProgression>, TcpRetireError> {
        let interface = self
            .protocols
            .tcp
            .connection(id)
            .map(|connection| connection.interface)
            .or_else(|| {
                self.protocols
                    .tcp
                    .listener(id)
                    .map(|listener| listener.interface)
            });
        let Some(interface) = interface else {
            return self.protocols.tcp.retire_without_engine(id).map(|()| None);
        };
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("live TCP Endpoint references an attached interface");
        Ok(tcp_owner
            .begin_release(sockets, id, reason)?
            .map(ProtocolProgression::committed))
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
