use alloc::{collections::VecDeque, vec, vec::Vec};

use anemone_net_api::InterfaceId;
use smoltcp::{
    iface::{Config, Interface, SocketSet},
    phy::{
        Device, DeviceCapabilities, Medium, RxToken as SmoltcpRxToken, TxToken as SmoltcpTxToken,
    },
    wire::HardwareAddress,
};

use crate::{stack::PumpOrder, udp::EndpointId};

struct LocalPacket {
    owner: Option<EndpointId>,
    bytes: Vec<u8>,
}

/// Bounded owner of IP packets between protocol egress and later ingress.
///
/// `owner` is cleanup protocol state, not a diagnostic label: it lets
/// aggregate Endpoint retirement withdraw packets that have not reached
/// normal ingress yet. Packet bytes never drive Endpoint lookup directly.
pub(crate) struct LocalLink {
    ingress: VecDeque<LocalPacket>,
    egress: VecDeque<LocalPacket>,
    packet_capacity: usize,
    mtu: usize,
    tx_owner: Option<EndpointId>,
}

impl LocalLink {
    fn new(packet_capacity: usize, mtu: usize) -> Self {
        assert!(packet_capacity > 0);
        assert!(mtu > 0);
        Self {
            ingress: VecDeque::with_capacity(packet_capacity),
            egress: VecDeque::with_capacity(packet_capacity),
            packet_capacity,
            mtu,
            tx_owner: None,
        }
    }

    pub(crate) fn occupied(&self) -> usize {
        self.ingress.len() + self.egress.len()
    }

    fn full(&self) -> bool {
        self.occupied() >= self.packet_capacity
    }

    pub(crate) fn set_tx_owner(&mut self, owner: Option<EndpointId>) {
        self.tx_owner = owner;
    }

    pub(crate) fn transfer(&mut self, budget: usize) -> usize {
        let mut transferred = 0;
        while transferred < budget {
            let Some(packet) = self.egress.pop_front() else {
                break;
            };
            self.ingress.push_back(packet);
            transferred += 1;
        }
        transferred
    }

    pub(crate) fn remove_owner(&mut self, owner: EndpointId) {
        self.ingress.retain(|packet| packet.owner != Some(owner));
        self.egress.retain(|packet| packet.owner != Some(owner));
        if self.tx_owner == Some(owner) {
            self.tx_owner = None;
        }
    }
}

pub(crate) struct LocalPort {
    pub(crate) id: InterfaceId,
    pub(crate) interface: Interface,
    pub(crate) sockets: SocketSet<'static>,
    pub(crate) link: LocalLink,
    pub(crate) next_pump_order: PumpOrder,
}

impl LocalPort {
    pub(crate) fn new(
        id: InterfaceId,
        now: smoltcp::time::Instant,
        packet_capacity: usize,
        mtu: usize,
    ) -> Self {
        let mut link = LocalLink::new(packet_capacity, mtu);
        let interface = Interface::new(
            Config::new(HardwareAddress::Ip),
            &mut LocalDevice::new(&mut link),
            now,
        );
        Self {
            id,
            interface,
            sockets: SocketSet::new(vec![]),
            link,
            next_pump_order: PumpOrder::IngressFirst,
        }
    }

    pub(crate) fn ip_mtu(&self) -> usize {
        self.link.mtu
    }

    pub(crate) fn local_link_capacity(&self) -> usize {
        self.link.packet_capacity
    }
}

pub(crate) struct LocalDevice<'a> {
    link: &'a mut LocalLink,
    blocked_work: bool,
}

impl<'a> LocalDevice<'a> {
    pub(crate) fn new(link: &'a mut LocalLink) -> Self {
        Self {
            link,
            blocked_work: false,
        }
    }

    pub(crate) fn blocked_work(&self) -> bool {
        self.blocked_work
    }
}

pub(crate) struct LocalRxToken<'a> {
    queue: &'a mut VecDeque<LocalPacket>,
    packet: Option<LocalPacket>,
}

impl SmoltcpRxToken for LocalRxToken<'_> {
    fn consume<R, F>(mut self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        let packet = self
            .packet
            .take()
            .expect("local RX token can be consumed only once");
        f(&packet.bytes)
    }
}

impl Drop for LocalRxToken<'_> {
    fn drop(&mut self) {
        if let Some(packet) = self.packet.take() {
            self.queue.push_front(packet);
        }
    }
}

pub(crate) struct LocalTxToken<'a> {
    queue: &'a mut VecDeque<LocalPacket>,
    owner: Option<EndpointId>,
    mtu: usize,
}

impl SmoltcpTxToken for LocalTxToken<'_> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        assert!(len <= self.mtu, "smoltcp exceeded local IP-medium MTU");
        let mut bytes = vec![0; len];
        let result = f(&mut bytes);
        self.queue.push_back(LocalPacket {
            owner: self.owner,
            bytes,
        });
        result
    }
}

impl Device for LocalDevice<'_> {
    type RxToken<'a>
        = LocalRxToken<'a>
    where
        Self: 'a;
    type TxToken<'a>
        = LocalTxToken<'a>
    where
        Self: 'a;

    fn receive(
        &mut self,
        _timestamp: smoltcp::time::Instant,
    ) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        // Removing the packet reserves one unit of the shared capacity for a
        // possible protocol reply. An unconsumed RX token restores it.
        let packet = self.link.ingress.pop_front()?;
        let packet_owner = packet.owner;
        Some((
            LocalRxToken {
                queue: &mut self.link.ingress,
                packet: Some(packet),
            },
            LocalTxToken {
                queue: &mut self.link.egress,
                // A protocol reply belongs to the packet being ingressed,
                // not to an unrelated Endpoint selected for this pump round.
                owner: packet_owner,
                mtu: self.link.mtu,
            },
        ))
    }

    fn transmit(&mut self, _timestamp: smoltcp::time::Instant) -> Option<Self::TxToken<'_>> {
        if self.link.full() {
            self.blocked_work = true;
            return None;
        }
        Some(LocalTxToken {
            queue: &mut self.link.egress,
            owner: self.link.tx_owner,
            mtu: self.link.mtu,
        })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut capabilities = DeviceCapabilities::default();
        capabilities.medium = Medium::Ip;
        capabilities.max_transmission_unit = self.link.mtu;
        capabilities
    }
}
