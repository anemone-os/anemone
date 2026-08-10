//! Role-aware point-in-time facts and recheck-only invalidation ownership.

use alloc::vec::Vec;

use anemone_net_api::{
    InterfaceId,
    tcp::{
        TcpConnectFact, TcpConnectionFacts, TcpEndpointFacts, TcpEndpointId,
        TcpEndpointInvalidation, TcpQueryError,
    },
};
use smoltcp::{iface::SocketSet, socket::tcp};

use super::{ConnectionPhase, EndpointRole, TcpEndpoints, completed_child_state};

impl TcpEndpoints {
    pub(crate) fn endpoint_facts(
        &self,
        sockets: Option<&SocketSet<'static>>,
        id: TcpEndpointId,
    ) -> Result<TcpEndpointFacts, TcpQueryError> {
        let endpoint = self.endpoint(id).ok_or(TcpQueryError::UnknownEndpoint)?;
        Ok(match &endpoint.role {
            EndpointRole::Idle => TcpEndpointFacts::Idle,
            EndpointRole::Bound(_) => TcpEndpointFacts::Bound,
            EndpointRole::Listener(listener) => {
                let sockets = sockets.expect("TCP listener facts need its engine owner");
                let has_pending_child = listener.slots.iter().any(|slot| {
                    slot.handle.is_some_and(|handle| {
                        !slot.claimed
                            && completed_child_state(sockets.get::<tcp::Socket>(handle).state())
                    })
                });
                TcpEndpointFacts::Listener { has_pending_child }
            },
            EndpointRole::Connection(connection) => {
                let sockets = sockets.expect("TCP connection facts need its engine owner");
                let socket = sockets.get::<tcp::Socket>(connection.handle);
                let connect = match connection.phase {
                    ConnectionPhase::Connecting => TcpConnectFact::Connecting,
                    ConnectionPhase::Connected => TcpConnectFact::Connected,
                    ConnectionPhase::Failed => TcpConnectFact::Failed,
                };
                let peer_receive_closed = connect == TcpConnectFact::Connected
                    && connection.pending_error.is_none()
                    && matches!(
                        socket.state(),
                        tcp::State::CloseWait
                            | tcp::State::Closing
                            | tcp::State::LastAck
                            | tcp::State::TimeWait
                            | tcp::State::Closed
                    );
                let send_capacity = if connect != TcpConnectFact::Connected
                    || connection.write_shutdown
                    || !socket.may_send()
                {
                    0
                } else {
                    socket.send_capacity().saturating_sub(socket.send_queue())
                };
                let received_bytes = socket.recv_queue();
                let receive_terminal = connection.read_shutdown || peer_receive_closed;
                let send_terminal = connection.write_shutdown || !socket.may_send();
                TcpEndpointFacts::Connection(TcpConnectionFacts::from_owner_snapshot(
                    connect,
                    send_capacity,
                    received_bytes,
                    connection.pending_error.is_some(),
                    connection.read_shutdown,
                    connection.write_shutdown,
                    peer_receive_closed,
                    connect == TcpConnectFact::Failed || (receive_terminal && send_terminal),
                ))
            },
            EndpointRole::Reclaiming { .. } | EndpointRole::Vacant => {
                return Err(TcpQueryError::UnknownEndpoint);
            },
        })
    }

    pub(crate) fn invalidate(&mut self, id: TcpEndpointId) {
        let endpoints = &self.endpoints;
        self.pending_invalidations.retain(|pending| {
            pending.endpoint() == id
                || endpoints
                    .iter()
                    .any(|endpoint| endpoint.id == Some(pending.endpoint()))
        });
        if self
            .pending_invalidations
            .iter()
            .any(|pending| pending.endpoint() == id)
        {
            return;
        }
        self.pending_invalidations
            .push(TcpEndpointInvalidation::from_owner_transition(id));
    }

    pub(crate) fn invalidate_interface(&mut self, interface: InterfaceId) {
        for index in 0..self.endpoints.len() {
            let endpoint = &self.endpoints[index];
            let id = endpoint.id.filter(|_| match &endpoint.role {
                EndpointRole::Listener(listener) => listener.interface == interface,
                EndpointRole::Connection(connection) => connection.interface == interface,
                EndpointRole::Reclaiming { .. }
                | EndpointRole::Idle
                | EndpointRole::Bound(_)
                | EndpointRole::Vacant => false,
            });
            if let Some(id) = id {
                self.invalidate(id);
            }
        }
    }

    pub(crate) fn take_invalidations(&mut self) -> Vec<TcpEndpointInvalidation> {
        core::mem::take(&mut self.pending_invalidations)
    }
}
