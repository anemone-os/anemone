//! Active connection observation and bounded scalar byte transactions.

use alloc::vec::Vec;

use anemone_net_api::{
    InterfaceId,
    tcp::{
        TcpConnectError, TcpConnectResult, TcpConnectionObservation, TcpEndpointId,
        TcpLocalBinding, TcpPeer, TcpPendingError, TcpQueryError, TcpReceiveError, TcpReceiveMode,
        TcpReceiveReservation, TcpReceiveReservationId, TcpReceiveResolveError, TcpSendError,
        TcpShutdownDirection, TcpShutdownError, TcpShutdownOutcome, TcpStreamObservation,
        TcpStreamReceiveError, TcpStreamReceiveOutcome, TcpStreamSendError,
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
        source: anemone_net_api::Ipv4Address,
        peer: TcpPeer,
    ) -> Result<(TcpLocalBinding, TcpLocalBinding), TcpConnectError> {
        let binding = match &self
            .endpoint(id)
            .ok_or(TcpConnectError::UnknownEndpoint)?
            .role
        {
            EndpointRole::Idle => None,
            EndpointRole::Bound(binding) => Some(*binding),
            _ => return Err(TcpConnectError::WrongRole),
        };
        if peer.port() == 0 || !peer.address().is_unicast() {
            return Err(TcpConnectError::InvalidPeer);
        }
        if !self.ensure_engine_capacity(1) {
            return Err(TcpConnectError::EngineCapacity);
        }
        if let Some(binding) = binding {
            if !binding.address().is_unspecified() && binding.address() != source {
                return Err(TcpConnectError::UnsupportedSource);
            }
            let local = TcpLocalBinding::from_owner_commit(source, binding.port());
            if self.connection_tuple_conflicts(id, local, peer) {
                return Err(TcpConnectError::PortInUse);
            }
            return Ok((binding, local));
        }

        for port in self.policy.ephemeral_port_first..=self.policy.ephemeral_port_last {
            let binding = TcpLocalBinding::from_owner_commit(source, port);
            if self.binding_conflicts(id, binding)
                || self.connection_tuple_conflicts(id, binding, peer)
            {
                continue;
            }
            return Ok((binding, binding));
        }
        Err(TcpConnectError::EphemeralPortsExhausted)
    }

    fn connection_tuple_conflicts(
        &self,
        id: TcpEndpointId,
        local: TcpLocalBinding,
        peer: TcpPeer,
    ) -> bool {
        self.endpoints.iter().any(|slot| match &slot.role {
            EndpointRole::Connection(connection) if slot.id != Some(id) => {
                connection.local == local && connection.peer == peer
            },
            EndpointRole::Listener(listener) => listener
                .slots
                .iter()
                .filter_map(|slot| slot.tuple)
                .any(|tuple| tuple.local == local && tuple.peer == peer),
            EndpointRole::Reclaiming {
                tuple: Some(tuple), ..
            } => tuple.local == local && tuple.peer == peer,
            _ => false,
        }) || self
            .deferred
            .iter()
            .filter_map(|reclaim| reclaim.tuple)
            .any(|tuple| tuple.local == local && tuple.peer == peer)
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
            was_connected: false,
            pending_error: None,
            read_shutdown: false,
            write_shutdown: false,
            reservation: None,
            release_requested: None,
        });
    }

    pub(crate) fn connection_observation(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
    ) -> Result<TcpConnectionObservation, TcpQueryError> {
        // Stage 2's syscall-unreachable scalar adapter still consumes this
        // non-consuming snapshot. Stage 3 CKPT 3B must move that adapter to
        // `connection_result`, after which ordinary connect and SO_ERROR share
        // the consuming owner path below.
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

        self.refresh_connection(sockets, id)?;
        let connection = self
            .connection(id)
            .expect("checked TCP connection role disappeared before observation");

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

    pub(crate) fn connection_result(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
    ) -> Result<TcpConnectResult, TcpQueryError> {
        self.refresh_connection(sockets, id)?;
        let connection = self
            .connection_mut(id)
            .expect("refreshed TCP connection disappeared");
        Ok(match connection.phase {
            ConnectionPhase::Connecting => TcpConnectResult::Connecting {
                local: connection.local,
                peer: connection.peer,
            },
            ConnectionPhase::Connected => TcpConnectResult::Connected {
                local: connection.local,
                peer: connection.peer,
            },
            ConnectionPhase::Failed(_) => match connection.pending_error.take() {
                Some(error) => TcpConnectResult::Failed(error),
                None => TcpConnectResult::Terminal,
            },
        })
    }

    pub(crate) fn consume_pending_error(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
    ) -> Result<Option<TcpPendingError>, TcpQueryError> {
        self.refresh_connection(sockets, id)?;
        Ok(self
            .connection_mut(id)
            .expect("refreshed TCP connection disappeared")
            .pending_error
            .take())
    }

    pub(crate) fn no_delay(&self, id: TcpEndpointId) -> Result<bool, TcpQueryError> {
        let endpoint = self.endpoint(id).ok_or(TcpQueryError::UnknownEndpoint)?;
        if matches!(endpoint.role, EndpointRole::Reclaiming { .. }) {
            return Err(TcpQueryError::UnknownEndpoint);
        }
        Ok(endpoint.no_delay)
    }

    pub(crate) fn set_no_delay(
        &mut self,
        sockets: Option<&mut SocketSet<'static>>,
        id: TcpEndpointId,
        enabled: bool,
    ) -> Result<Option<InterfaceId>, TcpQueryError> {
        let endpoint = self
            .endpoint_mut(id)
            .ok_or(TcpQueryError::UnknownEndpoint)?;
        if matches!(endpoint.role, EndpointRole::Reclaiming { .. }) {
            return Err(TcpQueryError::UnknownEndpoint);
        }
        if endpoint.no_delay == enabled {
            return Ok(None);
        }
        endpoint.no_delay = enabled;
        let EndpointRole::Connection(connection) = &endpoint.role else {
            return Ok(None);
        };
        let interface = connection.interface;
        let handle = connection.handle;
        sockets
            .expect("connected TCP option mutation needs its engine owner")
            .get_mut::<tcp::Socket>(handle)
            .set_nagle_enabled(!enabled);
        Ok(Some(interface))
    }

    pub(crate) fn stream_observation(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
    ) -> Result<TcpStreamObservation, TcpQueryError> {
        self.refresh_connection(sockets, id)?;
        let connection = self
            .connection(id)
            .expect("refreshed TCP connection disappeared");
        let socket = sockets.get::<tcp::Socket>(connection.handle);
        let send_capacity = if connection.write_shutdown || !socket.may_send() {
            0
        } else {
            socket.send_capacity().saturating_sub(socket.send_queue())
        };
        let has_bytes = socket.can_recv();
        let has_pending_error = connection.pending_error.is_some();
        let end_of_stream = !has_bytes
            && !has_pending_error
            && (connection.read_shutdown || (connection.was_connected && !socket.may_recv()));
        Ok(TcpStreamObservation::from_owner_fact(
            send_capacity,
            has_bytes,
            !connection.read_shutdown && socket.may_recv(),
            has_pending_error,
            end_of_stream,
        ))
    }

    pub(crate) fn send(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
        bytes: &[u8],
    ) -> Result<usize, TcpSendError> {
        self.send_stream(sockets, id, bytes)
            .map_err(|error| match error {
                TcpStreamSendError::UnknownEndpoint => TcpSendError::UnknownEndpoint,
                TcpStreamSendError::WouldBlock => TcpSendError::WouldBlock,
                TcpStreamSendError::NotConnected
                | TcpStreamSendError::BrokenStream
                | TcpStreamSendError::ConnectionRefused
                | TcpStreamSendError::ConnectionReset
                | TcpStreamSendError::TimedOut => TcpSendError::NotConnected,
            })
    }

    pub(crate) fn send_stream(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
        bytes: &[u8],
    ) -> Result<usize, TcpStreamSendError> {
        self.refresh_connection(sockets, id)
            .map_err(|error| match error {
                TcpQueryError::UnknownEndpoint => TcpStreamSendError::UnknownEndpoint,
                TcpQueryError::WrongRole => TcpStreamSendError::NotConnected,
            })?;
        let connection = self
            .connection_mut(id)
            .expect("refreshed TCP connection disappeared");
        if let Some(error) = connection.pending_error.take() {
            return Err(map_send_pending_error(error));
        }
        if connection.write_shutdown {
            return Err(TcpStreamSendError::BrokenStream);
        }
        if connection.phase != ConnectionPhase::Connected {
            return Err(if connection.was_connected {
                TcpStreamSendError::BrokenStream
            } else {
                TcpStreamSendError::NotConnected
            });
        }
        let socket = sockets.get_mut::<tcp::Socket>(connection.handle);
        if !socket.may_send() {
            return Err(TcpStreamSendError::BrokenStream);
        }
        let accepted = socket
            .send_slice(bytes)
            .map_err(|_| TcpStreamSendError::BrokenStream)?;
        if accepted == 0 && !bytes.is_empty() {
            return Err(TcpStreamSendError::WouldBlock);
        }
        Ok(accepted)
    }

    pub(crate) fn reserve_receive(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
        maximum: usize,
    ) -> Result<TcpReceiveReservation, TcpReceiveError> {
        match self.receive_stream(sockets, id, maximum, TcpReceiveMode::Consume) {
            Ok(TcpStreamReceiveOutcome::Data(reservation)) => Ok(reservation),
            Ok(TcpStreamReceiveOutcome::EndOfStream) => Err(TcpReceiveError::WouldBlock),
            Err(error) => Err(match error {
                TcpStreamReceiveError::UnknownEndpoint => TcpReceiveError::UnknownEndpoint,
                TcpStreamReceiveError::WouldBlock => TcpReceiveError::WouldBlock,
                TcpStreamReceiveError::ReservationOutstanding => {
                    TcpReceiveError::ReservationOutstanding
                },
                TcpStreamReceiveError::NotConnected
                | TcpStreamReceiveError::ConnectionRefused
                | TcpStreamReceiveError::ConnectionReset
                | TcpStreamReceiveError::TimedOut => TcpReceiveError::NotConnected,
            }),
        }
    }

    pub(crate) fn receive_stream(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
        maximum: usize,
        mode: TcpReceiveMode,
    ) -> Result<TcpStreamReceiveOutcome, TcpStreamReceiveError> {
        self.refresh_connection(sockets, id)
            .map_err(|error| match error {
                TcpQueryError::UnknownEndpoint => TcpStreamReceiveError::UnknownEndpoint,
                TcpQueryError::WrongRole => TcpStreamReceiveError::NotConnected,
            })?;
        let connection = self
            .connection(id)
            .expect("refreshed TCP connection disappeared");
        if connection.reservation.is_some() {
            return Err(TcpStreamReceiveError::ReservationOutstanding);
        }
        let socket = sockets.get_mut::<tcp::Socket>(connection.handle);
        let bytes = if maximum == 0 || socket.can_recv() {
            socket
                .peek(maximum)
                .map_err(|_| TcpStreamReceiveError::NotConnected)?
                .to_vec()
        } else {
            Vec::new()
        };
        if bytes.is_empty() && maximum != 0 {
            let connection = self
                .connection_mut(id)
                .expect("refreshed TCP connection disappeared");
            if let Some(error) = connection.pending_error.take() {
                return Err(map_receive_pending_error(error));
            }
            let socket = sockets.get::<tcp::Socket>(connection.handle);
            if connection.read_shutdown || (connection.was_connected && !socket.may_recv()) {
                return Ok(TcpStreamReceiveOutcome::EndOfStream);
            }
            if connection.phase != ConnectionPhase::Connected {
                return Err(TcpStreamReceiveError::NotConnected);
            }
            return Err(TcpStreamReceiveError::WouldBlock);
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
            mode,
        });
        Ok(TcpStreamReceiveOutcome::Data(
            TcpReceiveReservation::from_owner_reservation(reservation, bytes),
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
        let (handle, interface, offered, mode, release_requested) = {
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
                outstanding.mode,
                connection.release_requested,
            )
        };

        if committed != 0 && mode == TcpReceiveMode::Consume {
            sockets
                .get_mut::<tcp::Socket>(handle)
                .recv(|available| {
                    assert!(available.len() >= offered);
                    (committed, ())
                })
                .expect("reserved TCP receive prefix must remain owner-readable");
        }
        if let Some(reason) = release_requested {
            self.queue_connection_reclaim(sockets, id, reason);
        }
        Ok((committed != 0 && mode == TcpReceiveMode::Consume).then_some(interface))
    }

    pub(crate) fn shutdown(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
        direction: TcpShutdownDirection,
    ) -> Result<(TcpShutdownOutcome, Option<InterfaceId>), TcpShutdownError> {
        self.refresh_connection(sockets, id)
            .map_err(|error| match error {
                TcpQueryError::UnknownEndpoint => TcpShutdownError::UnknownEndpoint,
                TcpQueryError::WrongRole => TcpShutdownError::NotConnected,
            })?;
        let connection = self
            .connection_mut(id)
            .expect("refreshed TCP connection disappeared");
        let socket_state = sockets.get::<tcp::Socket>(connection.handle).state();
        if connection.phase != ConnectionPhase::Connected || socket_state == tcp::State::Closed {
            return Err(TcpShutdownError::NotConnected);
        }
        let shut_read = matches!(
            direction,
            TcpShutdownDirection::Read | TcpShutdownDirection::ReadWrite
        );
        let shut_write = matches!(
            direction,
            TcpShutdownDirection::Write | TcpShutdownDirection::ReadWrite
        );
        let mut changed = false;
        if shut_read && !connection.read_shutdown {
            connection.read_shutdown = true;
            changed = true;
        }
        if shut_write && !connection.write_shutdown {
            connection.write_shutdown = true;
            sockets.get_mut::<tcp::Socket>(connection.handle).close();
            changed = true;
        }
        Ok((
            if changed {
                TcpShutdownOutcome::Changed
            } else {
                TcpShutdownOutcome::Unchanged
            },
            changed.then_some(connection.interface),
        ))
    }

    pub(super) fn refresh_connection(
        &mut self,
        sockets: &mut SocketSet<'static>,
        id: TcpEndpointId,
    ) -> Result<(), TcpQueryError> {
        let connection = self.connection(id).ok_or_else(|| {
            if self.endpoint(id).is_some() {
                TcpQueryError::WrongRole
            } else {
                TcpQueryError::UnknownEndpoint
            }
        })?;
        let handle = connection.handle;
        let previous_phase = connection.phase;
        let socket = sockets.get_mut::<tcp::Socket>(handle);
        let reason = socket.take_disconnect_reason();
        let state = socket.state();
        if previous_phase == ConnectionPhase::Connecting
            && reason.is_none()
            && matches!(
                state,
                tcp::State::Established
                    | tcp::State::CloseWait
                    | tcp::State::FinWait1
                    | tcp::State::FinWait2
                    | tcp::State::Closing
                    | tcp::State::LastAck
                    | tcp::State::TimeWait
            )
        {
            // The active-open deadline owns only the handshake. Established
            // stream and final-release timeout policy are installed by their
            // respective owner transitions.
            socket.set_timeout(None);
        }

        let connection = self
            .connection_mut(id)
            .expect("TCP connection disappeared during owner refresh");
        if let Some(reason) = reason {
            let pending = match reason {
                tcp::DisconnectReason::ResetBeforeEstablished => TcpPendingError::ConnectionRefused,
                tcp::DisconnectReason::ResetAfterEstablished => TcpPendingError::ConnectionReset,
                tcp::DisconnectReason::Timeout => TcpPendingError::TimedOut,
            };
            connection.pending_error.get_or_insert(pending);
            connection.phase = ConnectionPhase::Failed(map_disconnect(reason));
        } else if matches!(
            state,
            tcp::State::Established
                | tcp::State::CloseWait
                | tcp::State::FinWait1
                | tcp::State::FinWait2
                | tcp::State::Closing
                | tcp::State::LastAck
                | tcp::State::TimeWait
        ) {
            connection.phase = ConnectionPhase::Connected;
            connection.was_connected = true;
        } else {
            connection.phase = previous_phase;
        }
        Ok(())
    }
}

fn map_send_pending_error(error: TcpPendingError) -> TcpStreamSendError {
    match error {
        TcpPendingError::ConnectionRefused => TcpStreamSendError::ConnectionRefused,
        TcpPendingError::ConnectionReset => TcpStreamSendError::ConnectionReset,
        TcpPendingError::TimedOut => TcpStreamSendError::TimedOut,
    }
}

fn map_receive_pending_error(error: TcpPendingError) -> TcpStreamReceiveError {
    match error {
        TcpPendingError::ConnectionRefused => TcpStreamReceiveError::ConnectionRefused,
        TcpPendingError::ConnectionReset => TcpStreamReceiveError::ConnectionReset,
        TcpPendingError::TimedOut => TcpStreamReceiveError::TimedOut,
    }
}
