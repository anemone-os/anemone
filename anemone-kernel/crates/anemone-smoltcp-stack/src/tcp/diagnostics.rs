//! Read-only, owner-normalized TCP diagnostic projection.

use alloc::vec::Vec;

use anemone_net_api::{
    InterfaceId,
    tcp::{TcpDiagnosticRecord, TcpDiagnosticState},
};
use smoltcp::{iface::SocketSet, socket::tcp};

use super::{EndpointRole, TcpEndpoints, completed_child_state, engine_connection_tuple};

impl TcpEndpoints {
    /// Copies one coherent diagnostic record set while the caller holds the
    /// Stack's sole observation window. The returned values contain no engine
    /// handles and are safe to serialize after that owner guard is released.
    pub(crate) fn diagnostic_records<'a>(
        &self,
        sockets_for: impl Fn(InterfaceId) -> Option<&'a SocketSet<'static>>,
    ) -> Vec<TcpDiagnosticRecord> {
        let mut records =
            Vec::with_capacity(self.engine_count.saturating_add(self.endpoints.len()));

        for endpoint in &self.endpoints {
            match &endpoint.role {
                EndpointRole::Listener(listener) => {
                    let sockets = sockets_for(listener.interface)
                        .expect("live TCP listener must retain its interface owner");
                    let completed = listener
                        .slots
                        .iter()
                        .filter(|slot| {
                            !slot.claimed
                                && slot.handle.is_some_and(|handle| {
                                    completed_child_state(
                                        sockets.get::<tcp::Socket>(handle).state(),
                                    )
                                })
                        })
                        .count();
                    records.push(TcpDiagnosticRecord::from_owner_snapshot(
                        listener.interface,
                        TcpDiagnosticState::Listen,
                        listener.binding,
                        None,
                        completed,
                        listener.backlog,
                    ));

                    for slot in &listener.slots {
                        let Some(handle) = slot.handle else { continue };
                        let socket = sockets.get::<tcp::Socket>(handle);
                        if socket.state() != tcp::State::SynReceived
                            && !completed_child_state(socket.state())
                        {
                            continue;
                        }
                        let tuple = engine_connection_tuple(socket)
                            .expect("passive TCP child must have an owner-normalized tuple");
                        records.push(connection_record(
                            listener.interface,
                            tuple.local,
                            tuple.peer,
                            socket,
                        ));
                    }
                },
                EndpointRole::Connection(connection) => {
                    let sockets = sockets_for(connection.interface)
                        .expect("live TCP connection must retain its interface owner");
                    records.push(connection_record(
                        connection.interface,
                        connection.local,
                        connection.peer,
                        sockets.get::<tcp::Socket>(connection.handle),
                    ));
                },
                EndpointRole::Vacant
                | EndpointRole::Idle
                | EndpointRole::Bound(_)
                | EndpointRole::Reclaiming { .. } => {},
            }
        }

        for reclaim in &self.deferred {
            let Some(tuple) = reclaim.tuple else { continue };
            let sockets = sockets_for(reclaim.interface)
                .expect("deferred TCP engine must retain its interface owner");
            records.push(connection_record(
                reclaim.interface,
                tuple.local,
                tuple.peer,
                sockets.get::<tcp::Socket>(reclaim.handle),
            ));
        }
        records
    }
}

fn connection_record(
    interface: InterfaceId,
    local: anemone_net_api::tcp::TcpLocalBinding,
    peer: anemone_net_api::tcp::TcpPeer,
    socket: &tcp::Socket<'static>,
) -> TcpDiagnosticRecord {
    TcpDiagnosticRecord::from_owner_snapshot(
        interface,
        diagnostic_state(socket.state()),
        local,
        Some(peer),
        socket.recv_queue(),
        socket.send_queue(),
    )
}

fn diagnostic_state(state: tcp::State) -> TcpDiagnosticState {
    match state {
        tcp::State::Closed => TcpDiagnosticState::Closed,
        tcp::State::Listen => TcpDiagnosticState::Listen,
        tcp::State::SynSent => TcpDiagnosticState::SynSent,
        tcp::State::SynReceived => TcpDiagnosticState::SynReceived,
        tcp::State::Established => TcpDiagnosticState::Established,
        tcp::State::FinWait1 => TcpDiagnosticState::FinWait1,
        tcp::State::FinWait2 => TcpDiagnosticState::FinWait2,
        tcp::State::CloseWait => TcpDiagnosticState::CloseWait,
        tcp::State::Closing => TcpDiagnosticState::Closing,
        tcp::State::LastAck => TcpDiagnosticState::LastAck,
        tcp::State::TimeWait => TcpDiagnosticState::TimeWait,
    }
}
