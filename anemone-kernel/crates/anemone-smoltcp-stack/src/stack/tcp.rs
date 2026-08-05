//! Aggregate Stack operations for the private TCP owner.

use anemone_net_api::{
    InterfaceId, Ipv4Address, Ipv4EgressSelection,
    tcp::{
        TcpBindError, TcpBindRequest, TcpChildError, TcpConnectError, TcpConnectionObservation,
        TcpCreateError, TcpEndpointId, TcpListenError, TcpLocalBinding, TcpPeer, TcpPendingChild,
        TcpQueryError, TcpReceiveError, TcpReceiveReservation, TcpReceiveReservationId,
        TcpReceiveResolveError, TcpRetireError, TcpSendError,
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

    pub fn start_tcp_connect(
        &mut self,
        id: TcpEndpointId,
        selection: Ipv4EgressSelection,
        peer: TcpPeer,
    ) -> Result<ProtocolProgression, TcpConnectError> {
        let source = selection.source();
        self.validate_tcp_selection(selection.interface(), source)?;
        let binding = self
            .protocols
            .tcp
            .prepare_binding(id, source)
            .map_err(TcpEndpoints::map_binding_error)?;
        if !binding.address().is_unspecified() && binding.address() != source {
            return Err(TcpConnectError::UnsupportedSource);
        }
        self.protocols.tcp.prepare_connect(id, binding, peer)?;
        let local = TcpLocalBinding::from_owner_commit(source, binding.port());
        let policy = self.protocols.tcp.policy();
        let (tcp_owner, interface, sockets) = self
            .tcp_owner_interface_mut(selection.interface())
            .ok_or(TcpConnectError::UnknownInterface)?;
        let mut socket = tcp_socket(policy);
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

    pub fn listen_tcp_endpoint(
        &mut self,
        id: TcpEndpointId,
        interface: InterfaceId,
        implicit_address: Ipv4Address,
    ) -> Result<(), TcpListenError> {
        if self.tcp_sockets(interface).is_none() {
            return Err(TcpListenError::UnknownInterface);
        }
        let binding = self
            .protocols
            .tcp
            .prepare_binding(id, implicit_address)
            .map_err(map_listen_bind_error)?;
        self.protocols.tcp.prepare_listener(id, binding)?;
        let (tcp_owner, _, sockets) = self
            .tcp_owner_interface_mut(interface)
            .expect("validated TCP listener interface disappeared");
        tcp_owner.commit_listener(sockets, id, interface, binding);
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
            .retire_endpoint(sockets, id)?
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
