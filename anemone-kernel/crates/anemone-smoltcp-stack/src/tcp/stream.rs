//! Active connection observation and bounded scalar byte transactions.

use anemone_net_api::{
    InterfaceId,
    tcp::{
        TcpBindError, TcpConnectError, TcpConnectionObservation, TcpEndpointId, TcpLocalBinding,
        TcpPeer, TcpQueryError, TcpReceiveError, TcpReceiveReservation, TcpReceiveReservationId,
        TcpReceiveResolveError, TcpSendError,
    },
};
use smoltcp::{iface::SocketSet, socket::tcp};

use super::{
    Connection, ConnectionPhase, EndpointRole, OutstandingReceive, TcpEndpoints, map_disconnect,
};

impl TcpEndpoints {
    pub(crate) fn prepare_connect(
        &self,
        id: TcpEndpointId,
        local: TcpLocalBinding,
        peer: TcpPeer,
    ) -> Result<(), TcpConnectError> {
        match &self
            .endpoint(id)
            .ok_or(TcpConnectError::UnknownEndpoint)?
            .role
        {
            EndpointRole::Idle | EndpointRole::Bound(_) => {},
            _ => return Err(TcpConnectError::WrongRole),
        }
        if peer.port() == 0 || !peer.address().is_unicast() {
            return Err(TcpConnectError::InvalidPeer);
        }
        if !self.ensure_engine_capacity(1) {
            return Err(TcpConnectError::EngineCapacity);
        }
        assert_eq!(self.current_binding(id).unwrap_or(local), local);
        Ok(())
    }

    pub(crate) fn commit_connect(
        &mut self,
        id: TcpEndpointId,
        interface: InterfaceId,
        handle: smoltcp::iface::SocketHandle,
        binding: TcpLocalBinding,
        local: TcpLocalBinding,
        peer: TcpPeer,
    ) {
        self.engine_count += 1;
        assert!(self.engine_count <= self.policy.engine_timer_capacity);
        self.endpoint_mut(id)
            .expect("prepared TCP Endpoint disappeared before connect commit")
            .role = EndpointRole::Connection(Connection {
            interface,
            handle,
            binding,
            local,
            peer,
            phase: ConnectionPhase::Connecting,
            reservation: None,
            retire_requested: false,
        });
    }

    pub(crate) fn connection_observation(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
    ) -> Result<TcpConnectionObservation, TcpQueryError> {
        let Some(endpoint) = self.endpoint(id) else {
            return Err(TcpQueryError::UnknownEndpoint);
        };
        match &endpoint.role {
            EndpointRole::Idle => return Ok(TcpConnectionObservation::Idle),
            EndpointRole::Bound(binding) => {
                return Ok(TcpConnectionObservation::Bound(*binding));
            },
            EndpointRole::Connection(_) => {},
            _ => return Err(TcpQueryError::WrongRole),
        }

        let connection = self
            .connection_mut(id)
            .expect("checked TCP connection role disappeared before observation");
        let socket = sockets.get_mut::<tcp::Socket>(connection.handle);
        if let Some(reason) = socket.take_disconnect_reason() {
            connection.phase = ConnectionPhase::Failed(map_disconnect(reason));
        } else if matches!(
            socket.state(),
            tcp::State::Established
                | tcp::State::CloseWait
                | tcp::State::FinWait1
                | tcp::State::FinWait2
                | tcp::State::Closing
                | tcp::State::LastAck
                | tcp::State::TimeWait
        ) {
            connection.phase = ConnectionPhase::Connected;
        }

        Ok(match connection.phase {
            ConnectionPhase::Connecting => TcpConnectionObservation::Connecting {
                local: connection.local,
                peer: connection.peer,
            },
            ConnectionPhase::Connected => TcpConnectionObservation::Connected {
                local: connection.local,
                peer: connection.peer,
            },
            ConnectionPhase::Failed(cause) => TcpConnectionObservation::Failed {
                local: connection.local,
                peer: connection.peer,
                cause,
            },
        })
    }

    pub(crate) fn send(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
        bytes: &[u8],
    ) -> Result<usize, TcpSendError> {
        let connection = self.connection(id).ok_or_else(|| {
            if self.endpoint(id).is_some() {
                TcpSendError::NotConnected
            } else {
                TcpSendError::UnknownEndpoint
            }
        })?;
        if connection.phase != ConnectionPhase::Connected {
            return Err(TcpSendError::NotConnected);
        }
        let handle = connection.handle;
        let accepted = sockets
            .get_mut::<tcp::Socket>(handle)
            .send_slice(bytes)
            .map_err(|_| TcpSendError::NotConnected)?;
        if accepted == 0 && !bytes.is_empty() {
            return Err(TcpSendError::WouldBlock);
        }
        Ok(accepted)
    }

    pub(crate) fn reserve_receive(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
        maximum: usize,
    ) -> Result<TcpReceiveReservation, TcpReceiveError> {
        let connection = self.connection(id).ok_or_else(|| {
            if self.endpoint(id).is_some() {
                TcpReceiveError::NotConnected
            } else {
                TcpReceiveError::UnknownEndpoint
            }
        })?;
        if connection.phase != ConnectionPhase::Connected {
            return Err(TcpReceiveError::NotConnected);
        }
        if connection.reservation.is_some() {
            return Err(TcpReceiveError::ReservationOutstanding);
        }
        let bytes = sockets
            .get_mut::<tcp::Socket>(connection.handle)
            .peek(maximum)
            .map_err(|_| TcpReceiveError::NotConnected)?
            .to_vec();
        if bytes.is_empty() && maximum != 0 {
            return Err(TcpReceiveError::WouldBlock);
        }

        let raw = self.next_reservation_id;
        self.next_reservation_id = raw
            .checked_add(1)
            .expect("TCP receive reservation namespace exhausted");
        let reservation = TcpReceiveReservationId::from_owner_raw(raw);
        self.connection_mut(id)
            .expect("TCP connection disappeared before receive reservation commit")
            .reservation = Some(OutstandingReceive {
            id: reservation,
            offered: bytes.len(),
        });
        Ok(TcpReceiveReservation::from_owner_reservation(
            reservation,
            bytes,
        ))
    }

    pub(crate) fn resolve_receive(
        &mut self,
        sockets: &mut SocketSet<'static>,
        reservation: TcpReceiveReservationId,
        committed: usize,
    ) -> Result<Option<InterfaceId>, TcpReceiveResolveError> {
        let Some(index) = self.endpoints.iter().position(|slot| {
            matches!(
                &slot.role,
                EndpointRole::Connection(connection)
                    if connection.reservation.is_some_and(|entry| entry.id == reservation)
            )
        }) else {
            return Err(TcpReceiveResolveError::UnknownReservation);
        };
        let id = self.endpoints[index]
            .id
            .expect("live TCP connection must retain its Endpoint identity");
        let (handle, interface, offered, retire_requested) = {
            let connection = self
                .connection_mut(id)
                .expect("reservation owner stopped being a TCP connection");
            let outstanding = connection
                .reservation
                .take()
                .expect("located TCP reservation disappeared before resolve");
            if committed > outstanding.offered {
                connection.reservation = Some(outstanding);
                return Err(TcpReceiveResolveError::InvalidPrefix);
            }
            (
                connection.handle,
                connection.interface,
                outstanding.offered,
                connection.retire_requested,
            )
        };

        if committed != 0 {
            sockets
                .get_mut::<tcp::Socket>(handle)
                .recv(|available| {
                    assert!(available.len() >= offered);
                    (committed, ())
                })
                .expect("reserved TCP receive prefix must remain owner-readable");
        }
        if retire_requested {
            self.queue_connection_reclaim(sockets, id);
        }
        Ok((committed != 0).then_some(interface))
    }

    pub(crate) fn map_binding_error(error: TcpBindError) -> TcpConnectError {
        match error {
            TcpBindError::UnknownEndpoint => TcpConnectError::UnknownEndpoint,
            TcpBindError::WrongRole => TcpConnectError::WrongRole,
            TcpBindError::PortInUse => TcpConnectError::PortInUse,
            TcpBindError::EphemeralPortsExhausted => TcpConnectError::EphemeralPortsExhausted,
        }
    }
}
