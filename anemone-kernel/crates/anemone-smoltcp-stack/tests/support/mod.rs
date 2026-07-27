#![allow(dead_code)]

use anemone_net_api::{
    Duration, EthernetAddress, FrameCapabilities, FrameProvider, FrameSizeError, Instant,
    InterfaceFacts, LinkState, ReceiveOutcome, RxToken, TransmitOutcome, TxToken,
};
use anemone_smoltcp_stack::{PumpBudget, Stack};
use smoltcp::{
    phy::ChecksumCapabilities,
    wire::{
        ArpOperation, ArpPacket, ArpRepr, EthernetAddress as SmoltcpEthernetAddress, EthernetFrame,
        EthernetProtocol, EthernetRepr, Icmpv4Packet, Icmpv4Repr, IpProtocol, Ipv4Address,
        Ipv4Packet, Ipv4Repr,
    },
};

pub(crate) const FRAME_CAPACITY: usize = 128;

// Each lane is the only owner of its slot state. Tokens borrow one lane
// exclusively: RX Drop restores Ready, while TX Drop restores Available.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RxSlot {
    Available,
    Ready,
    Reserved,
}

pub(crate) struct RxLane {
    pub(crate) backing: Vec<u8>,
    pub(crate) len: usize,
    pub(crate) slot: RxSlot,
    pub(crate) cancellations: usize,
    pub(crate) recycles: usize,
}

impl RxLane {
    fn new() -> Self {
        Self {
            backing: vec![0; FRAME_CAPACITY],
            len: 0,
            slot: RxSlot::Available,
            cancellations: 0,
            recycles: 0,
        }
    }

    pub(crate) fn inject(&mut self, frame: &[u8]) {
        assert_eq!(self.slot, RxSlot::Available);
        assert!(frame.len() <= self.backing.len());
        self.backing[..frame.len()].copy_from_slice(frame);
        self.len = frame.len();
        self.slot = RxSlot::Ready;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TxSlot {
    Available,
    Reserved,
    Submitted,
}

pub(crate) struct TxLane {
    pub(crate) backing: Vec<u8>,
    pub(crate) len: usize,
    pub(crate) slot: TxSlot,
    pub(crate) cancellations: usize,
    pub(crate) rejections: usize,
    pub(crate) submissions: usize,
    pub(crate) completions: usize,
}

impl TxLane {
    fn new() -> Self {
        Self {
            backing: vec![0; FRAME_CAPACITY],
            len: 0,
            slot: TxSlot::Available,
            cancellations: 0,
            rejections: 0,
            submissions: 0,
            completions: 0,
        }
    }

    pub(crate) fn complete(&mut self) {
        assert_eq!(self.slot, TxSlot::Submitted);
        self.slot = TxSlot::Available;
        self.completions += 1;
    }
}

pub(crate) struct DeterministicProvider {
    pub(crate) rx: RxLane,
    pub(crate) tx: TxLane,
    pub(crate) facts: InterfaceFacts,
    pub(crate) observed_at: Instant,
    pub(crate) receive_calls: usize,
}

impl DeterministicProvider {
    pub(crate) fn new() -> Self {
        Self::with_mac([0x02, 0, 0, 0, 0, 1])
    }

    pub(crate) fn with_mac(mac: [u8; 6]) -> Self {
        Self {
            rx: RxLane::new(),
            tx: TxLane::new(),
            facts: InterfaceFacts {
                ethernet_address: Some(EthernetAddress::new(mac)),
                max_frame_len: FRAME_CAPACITY,
                link_state: LinkState::Up,
            },
            observed_at: Instant::ZERO,
            receive_calls: 0,
        }
    }

    fn observe(&mut self, now: Instant) -> bool {
        self.observed_at = now;
        self.facts.link_state == LinkState::Up
    }
}

pub(crate) struct DeterministicRxToken<'a> {
    lane: &'a mut RxLane,
    consumed: bool,
}

impl RxToken for DeterministicRxToken<'_> {
    fn consume<R, F>(mut self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        assert_eq!(self.lane.slot, RxSlot::Reserved);
        let result = f(&self.lane.backing[..self.lane.len]);
        self.lane.len = 0;
        self.lane.slot = RxSlot::Available;
        self.lane.recycles += 1;
        self.consumed = true;
        result
    }
}

impl Drop for DeterministicRxToken<'_> {
    fn drop(&mut self) {
        if !self.consumed {
            assert_eq!(self.lane.slot, RxSlot::Reserved);
            self.lane.slot = RxSlot::Ready;
            self.lane.cancellations += 1;
        }
    }
}

pub(crate) struct DeterministicTxToken<'a> {
    lane: &'a mut TxLane,
    consumed: bool,
}

impl TxToken for DeterministicTxToken<'_> {
    fn capacity(&self) -> usize {
        self.lane.backing.len()
    }

    fn consume<R, F>(mut self, len: usize, f: F) -> Result<R, FrameSizeError>
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        assert_eq!(self.lane.slot, TxSlot::Reserved);
        if len > self.capacity() {
            self.lane.slot = TxSlot::Available;
            self.lane.rejections += 1;
            self.consumed = true;
            return Err(FrameSizeError::new(len, self.capacity()));
        }

        let result = f(&mut self.lane.backing[..len]);
        self.lane.len = len;
        self.lane.slot = TxSlot::Submitted;
        self.lane.submissions += 1;
        self.consumed = true;
        Ok(result)
    }
}

impl Drop for DeterministicTxToken<'_> {
    fn drop(&mut self) {
        if !self.consumed {
            assert_eq!(self.lane.slot, TxSlot::Reserved);
            self.lane.slot = TxSlot::Available;
            self.lane.cancellations += 1;
        }
    }
}

impl FrameProvider for DeterministicProvider {
    type RxToken<'a> = DeterministicRxToken<'a>;
    type TxToken<'a> = DeterministicTxToken<'a>;

    fn receive(&mut self, now: Instant) -> ReceiveOutcome<Self::RxToken<'_>, Self::TxToken<'_>> {
        self.receive_calls += 1;
        if !self.observe(now) {
            return ReceiveOutcome::LinkUnavailable;
        }
        if self.rx.slot != RxSlot::Ready {
            return ReceiveOutcome::Empty;
        }
        if self.tx.slot != TxSlot::Available {
            return ReceiveOutcome::TransmitExhausted;
        }

        self.rx.slot = RxSlot::Reserved;
        self.tx.slot = TxSlot::Reserved;
        ReceiveOutcome::Ready {
            rx: DeterministicRxToken {
                lane: &mut self.rx,
                consumed: false,
            },
            tx: DeterministicTxToken {
                lane: &mut self.tx,
                consumed: false,
            },
        }
    }

    fn transmit(&mut self, now: Instant) -> TransmitOutcome<Self::TxToken<'_>> {
        if !self.observe(now) {
            return TransmitOutcome::LinkUnavailable;
        }
        if self.tx.slot != TxSlot::Available {
            return TransmitOutcome::Exhausted;
        }

        self.tx.slot = TxSlot::Reserved;
        TransmitOutcome::Ready(DeterministicTxToken {
            lane: &mut self.tx,
            consumed: false,
        })
    }

    fn capabilities(&self) -> FrameCapabilities {
        FrameCapabilities {
            max_frame_len: self.facts.max_frame_len,
        }
    }

    fn link_state(&self) -> LinkState {
        self.facts.link_state
    }
}

pub(crate) struct ManualClock {
    pub(crate) now: Instant,
}

impl ManualClock {
    pub(crate) fn new() -> Self {
        Self { now: Instant::ZERO }
    }

    pub(crate) fn advance(&mut self, duration: Duration) {
        self.now = Instant::from_micros(
            self.now.total_micros() + i64::try_from(duration.total_micros()).unwrap(),
        );
    }
}

pub(crate) struct BoundedTxToken<'a> {
    lane: &'a mut TxLane,
    submission_log: &'a mut Vec<Vec<u8>>,
    consumed: bool,
}

impl TxToken for BoundedTxToken<'_> {
    fn capacity(&self) -> usize {
        self.lane.backing.len()
    }

    fn consume<R, F>(mut self, len: usize, f: F) -> Result<R, FrameSizeError>
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        assert_eq!(self.lane.slot, TxSlot::Reserved);
        if len > self.capacity() {
            self.lane.slot = TxSlot::Available;
            self.lane.rejections += 1;
            self.consumed = true;
            return Err(FrameSizeError::new(len, self.capacity()));
        }

        let result = f(&mut self.lane.backing[..len]);
        self.lane.len = len;
        self.submission_log.push(self.lane.backing[..len].to_vec());
        self.lane.slot = TxSlot::Submitted;
        self.lane.submissions += 1;
        self.consumed = true;
        Ok(result)
    }
}

impl Drop for BoundedTxToken<'_> {
    fn drop(&mut self) {
        if !self.consumed {
            assert_eq!(self.lane.slot, TxSlot::Reserved);
            self.lane.slot = TxSlot::Available;
            self.lane.cancellations += 1;
        }
    }
}

/// Test-owned provider whose finite credits and completion order are the
/// durable truth. The recheck bit is only a fixture edge and never decides
/// whether a slot is available.
pub(crate) struct BoundedProvider {
    rx: Vec<RxLane>,
    tx: Vec<TxLane>,
    facts: InterfaceFacts,
    submission_log: Vec<Vec<u8>>,
    normal_exhaustions: usize,
    recheck_requested: bool,
    recheck_publications: usize,
}

impl BoundedProvider {
    pub(crate) fn with_mac(mac: [u8; 6], tx_capacity: usize) -> Self {
        Self::with_capacities(mac, 1, tx_capacity)
    }

    pub(crate) fn with_capacities(mac: [u8; 6], rx_capacity: usize, tx_capacity: usize) -> Self {
        assert!(rx_capacity > 0);
        assert!(tx_capacity > 0);
        Self {
            rx: (0..rx_capacity).map(|_| RxLane::new()).collect(),
            tx: (0..tx_capacity).map(|_| TxLane::new()).collect(),
            facts: InterfaceFacts {
                ethernet_address: Some(EthernetAddress::new(mac)),
                max_frame_len: FRAME_CAPACITY,
                link_state: LinkState::Up,
            },
            submission_log: Vec::new(),
            normal_exhaustions: 0,
            recheck_requested: false,
            recheck_publications: 0,
        }
    }

    pub(crate) fn inject(&mut self, frame: &[u8]) {
        self.rx
            .iter_mut()
            .find(|lane| lane.slot == RxSlot::Available)
            .expect("bounded RX capacity exhausted")
            .inject(frame);
    }

    pub(crate) fn complete(&mut self, index: usize) {
        self.tx[index].complete();
    }

    pub(crate) fn complete_all(&mut self) {
        for lane in &mut self.tx {
            if lane.slot == TxSlot::Submitted {
                lane.complete();
            }
        }
    }

    pub(crate) fn reset_observation(&mut self) {
        assert_eq!(self.live_tx(), 0);
        self.submission_log.clear();
        self.normal_exhaustions = 0;
        self.recheck_requested = false;
        self.recheck_publications = 0;
    }

    pub(crate) fn live_tx(&self) -> usize {
        self.tx
            .iter()
            .filter(|lane| lane.slot == TxSlot::Submitted)
            .count()
    }

    pub(crate) fn ready_rx(&self) -> usize {
        self.rx
            .iter()
            .filter(|lane| lane.slot == RxSlot::Ready)
            .count()
    }

    pub(crate) fn tx_slot(&self, index: usize) -> TxSlot {
        self.tx[index].slot
    }

    pub(crate) fn tx_cancellations(&self) -> usize {
        self.tx.iter().map(|lane| lane.cancellations).sum()
    }

    pub(crate) fn tx_rejections(&self) -> usize {
        self.tx.iter().map(|lane| lane.rejections).sum()
    }

    pub(crate) fn rx_cancellations(&self) -> usize {
        self.rx.iter().map(|lane| lane.cancellations).sum()
    }

    pub(crate) fn rx_recycles(&self) -> usize {
        self.rx.iter().map(|lane| lane.recycles).sum()
    }

    pub(crate) fn submissions(&self) -> usize {
        self.submission_log.len()
    }

    pub(crate) fn submitted_frames(&self) -> &[Vec<u8>] {
        &self.submission_log
    }

    pub(crate) fn normal_exhaustions(&self) -> usize {
        self.normal_exhaustions
    }

    pub(crate) fn publish_recheck(&mut self) {
        self.recheck_requested = true;
        self.recheck_publications += 1;
    }

    pub(crate) fn take_recheck(&mut self) -> bool {
        core::mem::take(&mut self.recheck_requested)
    }

    pub(crate) fn recheck_publications(&self) -> usize {
        self.recheck_publications
    }

    pub(crate) fn set_link_state(&mut self, state: LinkState) {
        self.facts.link_state = state;
    }
}

impl FrameProvider for BoundedProvider {
    type RxToken<'a> = DeterministicRxToken<'a>;
    type TxToken<'a> = BoundedTxToken<'a>;

    fn receive(&mut self, _now: Instant) -> ReceiveOutcome<Self::RxToken<'_>, Self::TxToken<'_>> {
        if self.facts.link_state != LinkState::Up {
            return ReceiveOutcome::LinkUnavailable;
        }
        let Some(rx_index) = self.rx.iter().position(|lane| lane.slot == RxSlot::Ready) else {
            return ReceiveOutcome::Empty;
        };
        let Some(index) = self.tx.iter_mut().position(|lane| {
            if lane.slot != TxSlot::Available {
                return false;
            }
            lane.slot = TxSlot::Reserved;
            true
        }) else {
            self.normal_exhaustions += 1;
            return ReceiveOutcome::TransmitExhausted;
        };

        self.rx[rx_index].slot = RxSlot::Reserved;
        ReceiveOutcome::Ready {
            rx: DeterministicRxToken {
                lane: &mut self.rx[rx_index],
                consumed: false,
            },
            tx: BoundedTxToken {
                lane: &mut self.tx[index],
                submission_log: &mut self.submission_log,
                consumed: false,
            },
        }
    }

    fn transmit(&mut self, _now: Instant) -> TransmitOutcome<Self::TxToken<'_>> {
        if self.facts.link_state != LinkState::Up {
            return TransmitOutcome::LinkUnavailable;
        }
        let Some(index) = self
            .tx
            .iter()
            .position(|lane| lane.slot == TxSlot::Available)
        else {
            self.normal_exhaustions += 1;
            return TransmitOutcome::Exhausted;
        };
        self.tx[index].slot = TxSlot::Reserved;
        TransmitOutcome::Ready(BoundedTxToken {
            lane: &mut self.tx[index],
            submission_log: &mut self.submission_log,
            consumed: false,
        })
    }

    fn capabilities(&self) -> FrameCapabilities {
        FrameCapabilities {
            max_frame_len: self.facts.max_frame_len,
        }
    }

    fn link_state(&self) -> LinkState {
        self.facts.link_state
    }
}

pub(crate) fn build_icmp_echo_request(
    source_mac: [u8; 6],
    destination_mac: [u8; 6],
    source_ip: [u8; 4],
    destination_ip: [u8; 4],
) -> Vec<u8> {
    let payload = [0xaa, 0xbb, 0xcc, 0xdd];
    let icmp = Icmpv4Repr::EchoRequest {
        ident: 0x1234,
        seq_no: 7,
        data: &payload,
    };
    let ipv4 = Ipv4Repr {
        src_addr: Ipv4Address::from_octets(source_ip),
        dst_addr: Ipv4Address::from_octets(destination_ip),
        next_header: IpProtocol::Icmp,
        payload_len: icmp.buffer_len(),
        hop_limit: 64,
    };
    let ethernet = EthernetRepr {
        src_addr: SmoltcpEthernetAddress::from_bytes(&source_mac),
        dst_addr: SmoltcpEthernetAddress::from_bytes(&destination_mac),
        ethertype: EthernetProtocol::Ipv4,
    };
    let ip_offset = ethernet.buffer_len();
    let icmp_offset = ip_offset + ipv4.buffer_len();
    let mut bytes = vec![0; icmp_offset + icmp.buffer_len()];

    ethernet.emit(&mut EthernetFrame::new_unchecked(&mut bytes[..]));
    ipv4.emit(
        &mut Ipv4Packet::new_unchecked(&mut bytes[ip_offset..]),
        &ChecksumCapabilities::default(),
    );
    icmp.emit(
        &mut Icmpv4Packet::new_unchecked(&mut bytes[icmp_offset..]),
        &ChecksumCapabilities::default(),
    );
    bytes
}

pub(crate) fn build_arp_request(
    source_mac: [u8; 6],
    source_ip: [u8; 4],
    destination_ip: [u8; 4],
) -> Vec<u8> {
    let source_mac = SmoltcpEthernetAddress::from_bytes(&source_mac);
    let ethernet = EthernetRepr {
        src_addr: source_mac,
        dst_addr: SmoltcpEthernetAddress::BROADCAST,
        ethertype: EthernetProtocol::Arp,
    };
    let arp = ArpRepr::EthernetIpv4 {
        operation: ArpOperation::Request,
        source_hardware_addr: source_mac,
        source_protocol_addr: Ipv4Address::from_octets(source_ip),
        target_hardware_addr: SmoltcpEthernetAddress::from_bytes(&[0; 6]),
        target_protocol_addr: Ipv4Address::from_octets(destination_ip),
    };
    let mut bytes = vec![0; ethernet.buffer_len() + arp.buffer_len()];
    ethernet.emit(&mut EthernetFrame::new_unchecked(&mut bytes[..]));
    arp.emit(&mut ArpPacket::new_unchecked(
        &mut bytes[ethernet.buffer_len()..],
    ));
    bytes
}

pub(crate) fn build_raw_ipv4_packet(
    source_ip: [u8; 4],
    destination_ip: [u8; 4],
    marker: u8,
) -> Vec<u8> {
    let ipv4 = Ipv4Repr {
        src_addr: Ipv4Address::from_octets(source_ip),
        dst_addr: Ipv4Address::from_octets(destination_ip),
        next_header: IpProtocol::Unknown(253),
        payload_len: 1,
        hop_limit: 64,
    };
    let mut bytes = vec![0; ipv4.buffer_len() + 1];
    ipv4.emit(
        &mut Ipv4Packet::new_unchecked(&mut bytes[..]),
        &ChecksumCapabilities::default(),
    );
    bytes[ipv4.buffer_len()] = marker;
    bytes
}

pub(crate) fn raw_ipv4_marker(frame: &[u8]) -> u8 {
    let ethernet = EthernetFrame::new_checked(frame).unwrap();
    let ipv4 = Ipv4Packet::new_checked(ethernet.payload()).unwrap();
    ipv4.payload()[0]
}

pub(crate) fn prime_neighbor(
    stack: &mut Stack,
    interface: anemone_net_api::InterfaceId,
    provider: &mut DeterministicProvider,
    peer_mac: [u8; 6],
    peer_ip: [u8; 4],
    local_ip: [u8; 4],
) {
    provider
        .rx
        .inject(&build_arp_request(peer_mac, peer_ip, local_ip));
    stack
        .pump(interface, provider, Instant::ZERO, PumpBudget::new(2, 1))
        .unwrap();
    assert_eq!(provider.tx.slot, TxSlot::Submitted);
    provider.tx.complete();
    provider.receive_calls = 0;
}

pub(crate) fn prime_bounded_neighbor(
    stack: &mut Stack,
    interface: anemone_net_api::InterfaceId,
    provider: &mut BoundedProvider,
    peer_mac: [u8; 6],
    peer_ip: [u8; 4],
    local_ip: [u8; 4],
) {
    provider.inject(&build_arp_request(peer_mac, peer_ip, local_ip));
    stack
        .pump(interface, provider, Instant::ZERO, PumpBudget::new(2, 1))
        .unwrap();
    assert_eq!(provider.live_tx(), 1);
    provider.complete_all();
}

pub(crate) fn assert_icmp_echo_reply(
    frame: &[u8],
    source_mac: [u8; 6],
    destination_mac: [u8; 6],
    source_ip: [u8; 4],
    destination_ip: [u8; 4],
) {
    let ethernet = EthernetFrame::new_checked(frame).unwrap();
    assert_eq!(
        ethernet.src_addr(),
        SmoltcpEthernetAddress::from_bytes(&source_mac)
    );
    assert_eq!(
        ethernet.dst_addr(),
        SmoltcpEthernetAddress::from_bytes(&destination_mac)
    );

    let ipv4 = Ipv4Packet::new_checked(ethernet.payload()).unwrap();
    assert_eq!(ipv4.src_addr(), Ipv4Address::from_octets(source_ip));
    assert_eq!(ipv4.dst_addr(), Ipv4Address::from_octets(destination_ip));
    let icmp = Icmpv4Packet::new_checked(ipv4.payload()).unwrap();
    assert!(matches!(
        Icmpv4Repr::parse(&icmp, &ChecksumCapabilities::default()).unwrap(),
        Icmpv4Repr::EchoReply {
            ident: 0x1234,
            seq_no: 7,
            data: [0xaa, 0xbb, 0xcc, 0xdd]
        }
    ));
}
