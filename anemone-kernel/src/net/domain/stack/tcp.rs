//! Serialized kernel entrypoints into the Stack-private TCP owner.

use anemone_net_api::{
    InterfaceId, Ipv4EgressSelection,
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

use super::*;

impl DomainStack {
    pub(in crate::net) fn create_tcp_endpoint(&self) -> Result<TcpEndpointId, TcpCreateError> {
        self.protocol_transition(|stack| stack.create_tcp_endpoint())
    }

    pub(in crate::net) fn bind_tcp_endpoint(
        &self,
        endpoint: TcpEndpointId,
        request: TcpBindRequest,
    ) -> Result<TcpLocalBinding, TcpBindError> {
        self.protocol_transition(|stack| stack.bind_tcp_endpoint(endpoint, request))
    }

    pub(in crate::net) fn tcp_endpoint_binding(
        &self,
        endpoint: TcpEndpointId,
    ) -> Result<Option<TcpLocalBinding>, TcpQueryError> {
        self.stack.lock().tcp_endpoint_binding(endpoint)
    }

    pub(in crate::net) fn tcp_reuse_address(
        &self,
        endpoint: TcpEndpointId,
    ) -> Result<bool, TcpBindError> {
        self.stack.lock().tcp_reuse_address(endpoint)
    }

    pub(in crate::net) fn set_tcp_reuse_address(
        &self,
        endpoint: TcpEndpointId,
        enabled: bool,
    ) -> Result<(), TcpBindError> {
        self.protocol_transition(|stack| stack.set_tcp_reuse_address(endpoint, enabled))
    }

    pub(in crate::net) fn tcp_no_delay(
        &self,
        endpoint: TcpEndpointId,
    ) -> Result<bool, TcpQueryError> {
        self.stack.lock().tcp_no_delay(endpoint)
    }

    pub(in crate::net) fn set_tcp_no_delay(
        &self,
        endpoint: TcpEndpointId,
        enabled: bool,
    ) -> Result<(), TcpQueryError> {
        let progression =
            self.protocol_transition(|stack| stack.set_tcp_no_delay(endpoint, enabled))?;
        if let Some(progression) = progression {
            crate::net::submit_protocol_progression(progression);
        }
        Ok(())
    }

    pub(in crate::net) fn start_tcp_connect(
        &self,
        endpoint: TcpEndpointId,
        selection: Ipv4EgressSelection,
        peer: TcpPeer,
    ) -> Result<(), TcpConnectError> {
        let progression =
            self.protocol_transition(|stack| stack.start_tcp_connect(endpoint, selection, peer))?;
        crate::net::submit_protocol_progression(progression);
        Ok(())
    }

    pub(in crate::net) fn observe_tcp_connection(
        &self,
        endpoint: TcpEndpointId,
    ) -> Result<TcpConnectionObservation, TcpQueryError> {
        self.protocol_transition(|stack| stack.observe_tcp_connection(endpoint))
    }

    pub(in crate::net) fn tcp_connect_result(
        &self,
        endpoint: TcpEndpointId,
    ) -> Result<TcpConnectResult, TcpQueryError> {
        self.protocol_transition(|stack| stack.tcp_connect_result(endpoint))
    }

    pub(in crate::net) fn consume_tcp_pending_error(
        &self,
        endpoint: TcpEndpointId,
    ) -> Result<Option<TcpPendingError>, TcpQueryError> {
        self.protocol_transition(|stack| stack.consume_tcp_pending_error(endpoint))
    }

    pub(in crate::net) fn observe_tcp_stream(
        &self,
        endpoint: TcpEndpointId,
    ) -> Result<TcpStreamObservation, TcpQueryError> {
        self.protocol_transition(|stack| stack.observe_tcp_stream(endpoint))
    }

    pub(in crate::net) fn listen_tcp_endpoint(
        &self,
        endpoint: TcpEndpointId,
        interface: InterfaceId,
        implicit_address: anemone_net_api::Ipv4Address,
    ) -> Result<(), TcpListenError> {
        self.protocol_transition(|stack| {
            stack.listen_tcp_endpoint(endpoint, interface, implicit_address)
        })
    }

    pub(in crate::net) fn listen_tcp_endpoint_with_backlog(
        &self,
        endpoint: TcpEndpointId,
        interface: InterfaceId,
        implicit_address: anemone_net_api::Ipv4Address,
        backlog: TcpListenBacklog,
    ) -> Result<(), TcpListenError> {
        self.protocol_transition(|stack| {
            stack.listen_tcp_endpoint_with_backlog(endpoint, interface, implicit_address, backlog)
        })
    }

    pub(in crate::net) fn claim_tcp_pending_child(
        &self,
        listener: TcpEndpointId,
    ) -> Result<Option<TcpPendingChild>, TcpChildError> {
        self.protocol_transition(|stack| stack.claim_tcp_pending_child(listener))
    }

    pub(in crate::net) fn take_tcp_child(
        &self,
        child: TcpPendingChild,
    ) -> Result<TcpEndpointId, TcpChildError> {
        self.protocol_transition(|stack| stack.take_tcp_child(child))
    }

    pub(in crate::net) fn cancel_tcp_child(
        &self,
        child: TcpPendingChild,
    ) -> Result<(), TcpChildError> {
        let progression = self.protocol_transition(|stack| stack.cancel_tcp_child(child))?;
        if let Some(progression) = progression {
            crate::net::submit_protocol_progression(progression);
        }
        Ok(())
    }

    pub(in crate::net) fn send_tcp_endpoint(
        &self,
        endpoint: TcpEndpointId,
        bytes: &[u8],
    ) -> Result<usize, TcpSendError> {
        let (accepted, progression) =
            self.protocol_transition(|stack| stack.send_tcp_endpoint(endpoint, bytes))?;
        if let Some(progression) = progression {
            crate::net::submit_protocol_progression(progression);
        }
        Ok(accepted)
    }

    pub(in crate::net) fn send_tcp_stream(
        &self,
        endpoint: TcpEndpointId,
        bytes: &[u8],
    ) -> Result<usize, TcpStreamSendError> {
        let (accepted, progression) =
            self.protocol_transition(|stack| stack.send_tcp_stream(endpoint, bytes))?;
        if let Some(progression) = progression {
            crate::net::submit_protocol_progression(progression);
        }
        Ok(accepted)
    }

    pub(in crate::net) fn reserve_tcp_receive(
        &self,
        endpoint: TcpEndpointId,
        maximum: usize,
    ) -> Result<TcpReceiveReservation, TcpReceiveError> {
        self.protocol_transition(|stack| stack.reserve_tcp_receive(endpoint, maximum))
    }

    pub(in crate::net) fn receive_tcp_stream(
        &self,
        endpoint: TcpEndpointId,
        maximum: usize,
        mode: TcpReceiveMode,
    ) -> Result<TcpStreamReceiveOutcome, TcpStreamReceiveError> {
        self.protocol_transition(|stack| stack.receive_tcp_stream(endpoint, maximum, mode))
    }

    pub(in crate::net) fn shutdown_tcp_endpoint(
        &self,
        endpoint: TcpEndpointId,
        direction: TcpShutdownDirection,
    ) -> Result<TcpShutdownOutcome, TcpShutdownError> {
        let (outcome, progression) =
            self.protocol_transition(|stack| stack.shutdown_tcp_endpoint(endpoint, direction))?;
        if let Some(progression) = progression {
            crate::net::submit_protocol_progression(progression);
        }
        Ok(outcome)
    }

    pub(in crate::net) fn resolve_tcp_receive(
        &self,
        reservation: TcpReceiveReservationId,
        committed: usize,
    ) -> Result<(), TcpReceiveResolveError> {
        let progression =
            self.protocol_transition(|stack| stack.resolve_tcp_receive(reservation, committed))?;
        if let Some(progression) = progression {
            crate::net::submit_protocol_progression(progression);
        }
        Ok(())
    }

    pub(in crate::net) fn retire_tcp_endpoint(
        &self,
        endpoint: TcpEndpointId,
    ) -> Result<(), TcpRetireError> {
        let progression = self.protocol_transition(|stack| stack.retire_tcp_endpoint(endpoint))?;
        if let Some(progression) = progression {
            crate::net::submit_protocol_progression(progression);
        }
        Ok(())
    }

    pub(in crate::net) fn release_tcp_endpoint(
        &self,
        endpoint: TcpEndpointId,
        reason: TcpReleaseReason,
    ) -> Result<(), TcpRetireError> {
        let progression =
            self.protocol_transition(|stack| stack.release_tcp_endpoint(endpoint, reason))?;
        if let Some(progression) = progression {
            crate::net::submit_protocol_progression(progression);
        }
        Ok(())
    }
}
