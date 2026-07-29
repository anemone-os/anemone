use alloc::vec::Vec;

use anemone_net_api::InterfaceId;
use smoltcp::{
    iface::SocketSet,
    socket::udp,
    wire::{IpAddress, IpEndpoint, Ipv4Address},
};

use super::{EndpointId, SendError, UdpEndpoints};

const IPV4_HEADER_LEN: usize = 20;
const UDP_HEADER_LEN: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReceivedDatagram {
    pub(crate) payload: Vec<u8>,
    pub(crate) source: IpEndpoint,
}

pub(super) struct PendingDatagram {
    pub(super) selected_interface: InterfaceId,
    pub(super) source: Ipv4Address,
    pub(super) destination: IpEndpoint,
    pub(super) payload: Vec<u8>,
}

pub(super) enum TxPhase {
    Idle,
    Queued(PendingDatagram),
    EngineOwned { interface: InterfaceId },
}

impl UdpEndpoints {
    pub(crate) fn queue_send(
        &mut self,
        endpoint_id: EndpointId,
        selected_interface: Option<InterfaceId>,
        source: Ipv4Address,
        destination: IpEndpoint,
        payload: &[u8],
        interface_ip_mtu: usize,
    ) -> Result<(), SendError> {
        let selected_interface = selected_interface.ok_or(SendError::MissingSelection)?;
        if destination.addr.is_unspecified() || destination.port == 0 {
            return Err(SendError::InvalidDestination);
        }
        let endpoint = self
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.id == endpoint_id)
            .ok_or(SendError::UnknownEndpoint)?;
        // Admission owns both limits that the private engine must satisfy.
        // Accepting against MTU alone would defer an engine-buffer failure to
        // pump time, after the operation has already reported success.
        let Some(interface_payload_capacity) =
            interface_ip_mtu.checked_sub(IPV4_HEADER_LEN + UDP_HEADER_LEN)
        else {
            // A zero-length UDP payload still needs both headers. Reporting a
            // zero payload maximum as admissible here would defer failure to
            // the device after the operation had already committed success.
            return Err(SendError::Oversize { maximum: 0 });
        };
        let maximum = interface_payload_capacity.min(endpoint.engine_payload_capacity);
        if payload.len() > maximum {
            return Err(SendError::Oversize { maximum });
        }
        if endpoint.engine(selected_interface).is_none() {
            return Err(SendError::UnknownInterface);
        }
        if !matches!(endpoint.tx, TxPhase::Idle) {
            return Err(SendError::TxFull);
        }
        endpoint.tx = TxPhase::Queued(PendingDatagram {
            selected_interface,
            source,
            destination,
            payload: payload.to_vec(),
        });
        Ok(())
    }

    /// Moves at most one Endpoint's datagram into an interface engine.
    ///
    /// A previous engine-owned datagram has priority so a blocked provider
    /// cannot be bypassed by another logical datagram on the same interface.
    pub(crate) fn prepare_egress(
        &mut self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
    ) -> Option<EndpointId> {
        if let Some((index, endpoint)) = self
            .endpoints
            .iter()
            .enumerate()
            .find(|(_, endpoint)| {
                matches!(endpoint.tx, TxPhase::EngineOwned { interface: owner } if owner == interface)
            })
        {
            self.next_egress_endpoint = (index + 1) % self.endpoints.len();
            return Some(endpoint.id);
        }
        if self.endpoints.is_empty() {
            return None;
        }

        let len = self.endpoints.len();
        for offset in 0..len {
            let index = (self.next_egress_endpoint + offset) % len;
            let endpoint = &mut self.endpoints[index];
            let selected = matches!(
                &endpoint.tx,
                TxPhase::Queued(pending) if pending.selected_interface == interface
            );
            if !selected {
                continue;
            }
            let TxPhase::Queued(pending) = core::mem::replace(&mut endpoint.tx, TxPhase::Idle)
            else {
                unreachable!();
            };
            let engine = endpoint
                .engine(interface)
                .expect("selected interface was validated before commit");
            let result = sockets.get_mut::<udp::Socket>(engine.handle).send_slice(
                &pending.payload,
                udp::UdpMetadata {
                    endpoint: pending.destination,
                    local_address: Some(IpAddress::Ipv4(pending.source)),
                    meta: Default::default(),
                },
            );
            assert!(
                result.is_ok(),
                "owner admission must reserve enough private engine capacity"
            );
            endpoint.tx = TxPhase::EngineOwned { interface };
            self.next_egress_endpoint = (index + 1) % len;
            return Some(endpoint.id);
        }
        None
    }

    pub(crate) fn complete_egress(
        &mut self,
        endpoint_id: Option<EndpointId>,
        interface: InterfaceId,
        sockets: &SocketSet<'static>,
    ) -> bool {
        let Some(endpoint_id) = endpoint_id else {
            return false;
        };
        let endpoint = self
            .endpoints
            .iter_mut()
            .find(|endpoint| endpoint.id == endpoint_id)
            .expect("active engine owner must remain published during pump");
        let engine = endpoint
            .engine(interface)
            .expect("active engine mapping must remain published during pump");
        if sockets.get::<udp::Socket>(engine.handle).send_queue() == 0 {
            endpoint.tx = TxPhase::Idle;
            false
        } else {
            true
        }
    }

    pub(crate) fn drain_ingress(
        &mut self,
        interface: InterfaceId,
        sockets: &mut SocketSet<'static>,
    ) {
        for endpoint in &mut self.endpoints {
            let Some(engine) = endpoint.engine(interface) else {
                continue;
            };
            let socket = sockets.get_mut::<udp::Socket>(engine.handle);
            while endpoint.received.len() < endpoint.receive_packet_capacity && socket.can_recv() {
                let (payload, metadata) = socket
                    .recv()
                    .expect("can_recv must imply one engine-owned datagram");
                endpoint.received.push_back(ReceivedDatagram {
                    payload: payload.to_vec(),
                    source: metadata.endpoint,
                });
            }
            // A full aggregate queue leaves this Endpoint's oldest engine
            // datagram in place. It must not gate other Endpoints or the whole
            // interface: later packets for this full UDP Endpoint may be
            // dropped by its bounded engine queue while unrelated sockets keep
            // making normal ingress progress.
        }
    }

    pub(crate) fn receive(&mut self, id: EndpointId) -> Option<ReceivedDatagram> {
        self.endpoints
            .iter_mut()
            .find(|endpoint| endpoint.id == id)?
            .received
            .pop_front()
    }
}
