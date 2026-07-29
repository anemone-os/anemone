use std::collections::VecDeque;

use anemone_net_api::InterfaceId;
use smoltcp::{
    iface::{Config, Interface, SocketSet},
    phy::{
        ChecksumCapabilities, Device, DeviceCapabilities, Medium, RxToken as SmoltcpRxToken,
        TxToken as SmoltcpTxToken,
    },
    socket::udp,
    time::Instant,
    wire::{
        ArpOperation, ArpPacket, ArpRepr, EthernetAddress, EthernetFrame, EthernetProtocol,
        EthernetRepr, HardwareAddress, IpAddress, IpCidr, IpEndpoint, Ipv4Address, Ipv4Packet,
        UdpPacket,
    },
};

const FIRST_MAC: EthernetAddress = EthernetAddress([0x02, 0, 0, 0, 1, 2]);
const FIRST_GATEWAY_MAC: EthernetAddress = EthernetAddress([0x02, 0, 0, 0, 1, 1]);
const FIRST_IP: Ipv4Address = Ipv4Address::new(10, 0, 1, 2);
const FIRST_GATEWAY_IP: Ipv4Address = Ipv4Address::new(10, 0, 1, 1);
const SECOND_MAC: EthernetAddress = EthernetAddress([0x02, 0, 0, 0, 2, 2]);
const SECOND_IP: Ipv4Address = Ipv4Address::new(10, 0, 2, 2);
const REMOTE_IP: Ipv4Address = Ipv4Address::new(192, 0, 2, 1);

struct CandidateSelection {
    interface: InterfaceId,
    source: Ipv4Address,
}

struct ObservedEthernet {
    rx: VecDeque<Vec<u8>>,
    tx: Vec<Vec<u8>>,
}

impl ObservedEthernet {
    fn new() -> Self {
        Self {
            rx: VecDeque::new(),
            tx: Vec::new(),
        }
    }
}

struct ObservedRx(Vec<u8>);

impl SmoltcpRxToken for ObservedRx {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.0)
    }
}

struct ObservedTx<'a>(&'a mut Vec<Vec<u8>>);

impl SmoltcpTxToken for ObservedTx<'_> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut frame = vec![0; len];
        let result = f(&mut frame);
        self.0.push(frame);
        result
    }
}

impl Device for ObservedEthernet {
    type RxToken<'a> = ObservedRx;
    type TxToken<'a> = ObservedTx<'a>;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let frame = self.rx.pop_front()?;
        Some((ObservedRx(frame), ObservedTx(&mut self.tx)))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(ObservedTx(&mut self.tx))
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut capabilities = DeviceCapabilities::default();
        capabilities.medium = Medium::Ethernet;
        capabilities.max_transmission_unit = 1514;
        capabilities.checksum = ChecksumCapabilities::ignored();
        capabilities
    }
}

fn ethernet_interface(
    device: &mut ObservedEthernet,
    mac: EthernetAddress,
    address: Ipv4Address,
) -> Interface {
    let mut interface = Interface::new(
        Config::new(HardwareAddress::Ethernet(mac)),
        device,
        Instant::ZERO,
    );
    interface.update_ip_addrs(|addresses| {
        addresses
            .push(IpCidr::new(IpAddress::Ipv4(address), 24))
            .unwrap();
    });
    interface
}

fn arp_reply(
    source_mac: EthernetAddress,
    source_ip: Ipv4Address,
    destination_mac: EthernetAddress,
    destination_ip: Ipv4Address,
) -> Vec<u8> {
    let ethernet = EthernetRepr {
        src_addr: source_mac,
        dst_addr: destination_mac,
        ethertype: EthernetProtocol::Arp,
    };
    let arp = ArpRepr::EthernetIpv4 {
        operation: ArpOperation::Reply,
        source_hardware_addr: source_mac,
        source_protocol_addr: source_ip,
        target_hardware_addr: destination_mac,
        target_protocol_addr: destination_ip,
    };
    let mut frame = vec![0; ethernet.buffer_len() + arp.buffer_len()];
    ethernet.emit(&mut EthernetFrame::new_unchecked(&mut frame));
    arp.emit(&mut ArpPacket::new_unchecked(
        &mut frame[ethernet.buffer_len()..],
    ));
    frame
}

/// Characterizes the naive shared-engine seam; it is not a production Stack
/// regression. Checkpoint 0B must remove this expected-wrong-behavior test once
/// selected-interface admission exists.
#[test]
fn naive_shared_udp_resource_allows_wrong_interface_egress() {
    let mut first_device = ObservedEthernet::new();
    let mut second_device = ObservedEthernet::new();
    let mut first_interface = ethernet_interface(&mut first_device, FIRST_MAC, FIRST_IP);
    let mut second_interface = ethernet_interface(&mut second_device, SECOND_MAC, SECOND_IP);
    first_interface
        .routes_mut()
        .add_default_ipv4_route(FIRST_GATEWAY_IP)
        .unwrap();

    // This ARP reply only prepares an ordinary Ethernet neighbor. The risk is
    // reproduced below by a real UDP socket enqueue and smoltcp socket egress;
    // no UDP packet is injected or manually encoded.
    first_device.rx.push_back(arp_reply(
        FIRST_GATEWAY_MAC,
        FIRST_GATEWAY_IP,
        FIRST_MAC,
        FIRST_IP,
    ));
    let mut empty_sockets = SocketSet::new(Vec::new());
    first_interface.poll(Instant::ZERO, &mut first_device, &mut empty_sockets);
    first_device.tx.clear();

    let mut socket = udp::Socket::new(
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY], vec![0; 32]),
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY], vec![0; 32]),
    );
    socket.bind(40000).unwrap();
    let selection = CandidateSelection {
        interface: InterfaceId::from_index(1),
        source: SECOND_IP,
    };
    socket
        .send_slice(
            b"0A",
            udp::UdpMetadata {
                endpoint: IpEndpoint::new(IpAddress::Ipv4(REMOTE_IP), 40001),
                local_address: Some(IpAddress::Ipv4(selection.source)),
                meta: Default::default(),
            },
        )
        .unwrap();
    assert_eq!(socket.send_queue(), 2);

    let mut sockets = SocketSet::new(Vec::new());
    let handle = sockets.add(socket);
    assert_eq!(selection.interface, InterfaceId::from_index(1));

    // The candidate makes one engine-owned UDP resource visible to both
    // interfaces. Polling the non-selected interface first consumes it and
    // emits through that provider because smoltcp receives the selected source
    // address but no selected-interface admission capability.
    first_interface.poll(Instant::from_millis(1), &mut first_device, &mut sockets);

    assert_eq!(sockets.get::<udp::Socket>(handle).send_queue(), 0);
    assert_eq!(first_device.tx.len(), 1);
    assert!(second_device.tx.is_empty());

    let ethernet = EthernetFrame::new_checked(&first_device.tx[0]).unwrap();
    assert_eq!(ethernet.src_addr(), FIRST_MAC);
    assert_eq!(ethernet.dst_addr(), FIRST_GATEWAY_MAC);
    let ipv4 = Ipv4Packet::new_checked(ethernet.payload()).unwrap();
    assert_eq!(ipv4.src_addr(), SECOND_IP);
    assert_eq!(ipv4.dst_addr(), REMOTE_IP);
    let udp = UdpPacket::new_checked(ipv4.payload()).unwrap();
    assert_eq!(udp.src_port(), 40000);
    assert_eq!(udp.dst_port(), 40001);
    assert_eq!(udp.payload(), b"0A");

    // Pumping the selected interface afterwards cannot recover the datagram:
    // queue ownership was already consumed by the wrong-interface poll.
    second_interface.poll(Instant::from_millis(2), &mut second_device, &mut sockets);
    assert_eq!(sockets.get::<udp::Socket>(handle).send_queue(), 0);
    assert!(second_device.tx.is_empty());
}
