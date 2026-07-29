use alloc::vec::Vec;

use anemone_net_api::{EthernetAddress, FrameProvider, Instant, InterfaceId};
use smoltcp::{
    iface::{Config, Interface, SocketSet},
    wire::{EthernetAddress as SmoltcpEthernetAddress, HardwareAddress},
};

use crate::{
    adapter::FrameDevice,
    local_link::LocalPort,
    udp::{EndpointCreateError, EndpointId, RetireError, SendError, UdpEndpoints},
};

#[cfg(feature = "host-test")]
mod host_validation;

#[cfg(feature = "host-test")]
pub use host_validation::{
    HostEndpointCreateError, HostEndpointId, HostEndpointObservation, HostLocalLinkObservation,
    HostReceivedDatagram, HostRetireError, HostSelection, HostSendError,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PumpError {
    UnknownInterface(InterfaceId),
}

#[derive(Clone, Copy)]
pub(crate) enum PumpOrder {
    IngressFirst,
    EgressFirst,
}

impl PumpOrder {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::IngressFirst => Self::EgressFirst,
            Self::EgressFirst => Self::IngressFirst,
        }
    }
}

pub(crate) struct InterfaceEntry {
    pub(crate) id: InterfaceId,
    // Stable attach-time snapshot required by smoltcp. The provider remains
    // the truth source; every pump asserts that this snapshot is not stale.
    pub(crate) frame_capacity: usize,
    pub(crate) interface: Interface,
    pub(crate) sockets: SocketSet<'static>,
    // This owner-local cursor chooses only the next software admission order.
    // It is not queue, link, resource, or deadline truth and never bypasses
    // either direction's finite PumpBudget.
    pub(crate) next_pump_order: PumpOrder,
}

/// Owns the private smoltcp interface resources and their opaque ID mapping.
///
/// `&mut Stack` is the unique pump capability. The kernel wiring owner may put
/// the stack behind its chosen synchronization primitive, but admission,
/// contention, and requeue policy stay outside this protocol-state owner.
#[derive(Default)]
pub struct Stack {
    pub(crate) interfaces: Vec<InterfaceEntry>,
    #[allow(dead_code)]
    pub(crate) local: Option<LocalPort>,
    pub(crate) udp: UdpEndpoints,
    pub(crate) next_interface_id: u32,
}

impl Stack {
    pub const fn new() -> Self {
        Self {
            interfaces: Vec::new(),
            local: None,
            udp: UdpEndpoints::new(),
            next_interface_id: 0,
        }
    }

    pub fn add_interface<P: FrameProvider>(
        &mut self,
        provider: &mut P,
        ethernet_address: EthernetAddress,
        now: Instant,
    ) -> InterfaceId {
        let raw_id = self.next_interface_id;
        self.next_interface_id = raw_id
            .checked_add(1)
            .expect("InterfaceId namespace exhausted");
        let id = InterfaceId::from_index(raw_id);
        let frame_capacity = provider.capabilities().max_frame_len;
        let mut device = FrameDevice::new(provider);
        let hardware_address = HardwareAddress::Ethernet(SmoltcpEthernetAddress::from_bytes(
            &ethernet_address.octets(),
        ));
        let interface = Interface::new(
            Config::new(hardware_address),
            &mut device,
            crate::adapter::to_smoltcp_instant(now),
        );

        let mut sockets = SocketSet::new(Vec::new());
        self.udp.add_interface(id, &mut sockets);
        self.interfaces.push(InterfaceEntry {
            id,
            frame_capacity,
            interface,
            sockets,
            next_pump_order: PumpOrder::IngressFirst,
        });
        id
    }

    /// Withdraws a transaction-local mapping before active publication.
    ///
    /// Interface IDs remain monotonic and are not reused. Runtime detach is not
    /// part of R0; the kernel attach authority only uses this for rollback when
    /// worker/wake/time preparation fails.
    pub fn remove_interface(&mut self, id: InterfaceId) -> Result<(), PumpError> {
        let Some(index) = self.interfaces.iter().position(|entry| entry.id == id) else {
            return Err(PumpError::UnknownInterface(id));
        };
        let mut entry = self.interfaces.remove(index);
        self.udp.remove_interface(id, &mut entry.sockets);
        Ok(())
    }

    #[allow(dead_code)]
    fn interface_ipv4_and_mtu(
        &self,
        id: InterfaceId,
        source: smoltcp::wire::Ipv4Address,
    ) -> Option<(bool, usize)> {
        if let Some(entry) = self.interfaces.iter().find(|entry| entry.id == id) {
            let ip_mtu = entry
                .frame_capacity
                .checked_sub(smoltcp::wire::EthernetFrame::<&[u8]>::header_len())?;
            return Some((entry.interface.has_ip_addr(source), ip_mtu));
        }
        self.local
            .as_ref()
            .filter(|local| local.id == id)
            .map(|local| (local.interface.has_ip_addr(source), local.ip_mtu()))
    }

    #[allow(dead_code)]
    fn create_udp_endpoint(
        &mut self,
        port: u16,
        receive_packet_capacity: usize,
        engine_payload_capacity: usize,
    ) -> Result<EndpointId, EndpointCreateError> {
        let mut endpoint =
            self.udp
                .prepare_endpoint(port, receive_packet_capacity, engine_payload_capacity)?;
        for entry in &mut self.interfaces {
            endpoint.add_engine(entry.id, &mut entry.sockets);
        }
        if let Some(local) = &mut self.local {
            endpoint.add_engine(local.id, &mut local.sockets);
        }
        Ok(self.udp.publish_endpoint(endpoint))
    }

    #[allow(dead_code)]
    fn send_udp(
        &mut self,
        endpoint: EndpointId,
        selected_interface: Option<InterfaceId>,
        source: smoltcp::wire::Ipv4Address,
        destination: smoltcp::wire::IpEndpoint,
        payload: &[u8],
    ) -> Result<(), SendError> {
        let selected = selected_interface.ok_or(SendError::MissingSelection)?;
        let (source_supported, ip_mtu) = self
            .interface_ipv4_and_mtu(selected, source)
            .ok_or(SendError::UnknownInterface)?;
        if !source_supported {
            return Err(SendError::UnsupportedSource);
        }
        self.udp.queue_send(
            endpoint,
            Some(selected),
            source,
            destination,
            payload,
            ip_mtu,
        )
    }

    #[allow(dead_code)]
    fn retire_udp_endpoint(&mut self, id: EndpointId) -> Result<(), RetireError> {
        // Withdraw the aggregate owner before touching private engine objects.
        let endpoint = self.udp.withdraw(id)?;
        for engine in endpoint.engines() {
            if let Some(entry) = self
                .interfaces
                .iter_mut()
                .find(|entry| entry.id == engine.interface())
            {
                entry.sockets.remove(engine.handle());
                continue;
            }
            if let Some(local) = self
                .local
                .as_mut()
                .filter(|local| local.id == engine.interface())
            {
                local.sockets.remove(engine.handle());
            }
        }
        if let Some(local) = &mut self.local {
            local.link.remove_owner(endpoint.id());
        }
        Ok(())
    }

    #[cfg(any(test, feature = "host-test"))]
    pub(crate) fn interface_mut(
        &mut self,
        id: InterfaceId,
    ) -> Result<&mut InterfaceEntry, PumpError> {
        self.interfaces
            .iter_mut()
            .find(|entry| entry.id == id)
            .ok_or(PumpError::UnknownInterface(id))
    }
}
