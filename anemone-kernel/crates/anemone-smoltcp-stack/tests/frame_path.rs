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
use std::sync::{Arc, Mutex, TryLockError, mpsc};

const FRAME_CAPACITY: usize = 128;

// Each lane is the only owner of its slot state. Tokens borrow one lane
// exclusively: RX Drop restores Ready, while TX Drop restores Available.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RxSlot {
    Available,
    Ready,
    Reserved,
}

struct RxLane {
    backing: Vec<u8>,
    len: usize,
    slot: RxSlot,
    cancellations: usize,
    recycles: usize,
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

    fn inject(&mut self, frame: &[u8]) {
        assert_eq!(self.slot, RxSlot::Available);
        assert!(frame.len() <= self.backing.len());
        self.backing[..frame.len()].copy_from_slice(frame);
        self.len = frame.len();
        self.slot = RxSlot::Ready;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TxSlot {
    Available,
    Reserved,
    Submitted,
}

struct TxLane {
    backing: Vec<u8>,
    len: usize,
    slot: TxSlot,
    cancellations: usize,
    rejections: usize,
    submissions: usize,
    completions: usize,
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

    fn complete(&mut self) {
        assert_eq!(self.slot, TxSlot::Submitted);
        self.slot = TxSlot::Available;
        self.completions += 1;
    }
}

struct DeterministicProvider {
    rx: RxLane,
    tx: TxLane,
    facts: InterfaceFacts,
    observed_at: Instant,
    receive_calls: usize,
}

impl DeterministicProvider {
    fn new() -> Self {
        Self::with_mac([0x02, 0, 0, 0, 0, 1])
    }

    fn with_mac(mac: [u8; 6]) -> Self {
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

struct DeterministicRxToken<'a> {
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

struct DeterministicTxToken<'a> {
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

struct ManualClock {
    now: Instant,
}

impl ManualClock {
    fn new() -> Self {
        Self { now: Instant::ZERO }
    }

    fn advance(&mut self, duration: Duration) {
        self.now = Instant::from_micros(
            self.now.total_micros() + i64::try_from(duration.total_micros()).unwrap(),
        );
    }
}

#[test]
fn rx_consume_recycles_owner_backing() {
    let mut provider = DeterministicProvider::new();
    provider.rx.inject(&[1, 2, 3, 4]);

    let ReceiveOutcome::Ready { rx, tx } = provider.receive(Instant::ZERO) else {
        panic!("injected frame must be available");
    };
    assert_eq!(rx.consume(|frame| frame.iter().copied().sum::<u8>()), 10);
    drop(tx);

    assert_eq!(provider.rx.slot, RxSlot::Available);
    assert_eq!(provider.rx.recycles, 1);
    assert_eq!(provider.tx.slot, TxSlot::Available);
}

#[test]
fn tx_fill_submits_until_owner_completes_it() {
    let mut provider = DeterministicProvider::new();

    let TransmitOutcome::Ready(tx) = provider.transmit(Instant::ZERO) else {
        panic!("initial transmit credit must be available");
    };
    tx.consume(4, |frame| frame.copy_from_slice(&[4, 3, 2, 1]))
        .unwrap();

    assert_eq!(provider.tx.slot, TxSlot::Submitted);
    assert_eq!(&provider.tx.backing[..provider.tx.len], &[4, 3, 2, 1]);
    assert!(matches!(
        provider.transmit(Instant::ZERO),
        TransmitOutcome::Exhausted
    ));
    provider.tx.complete();
    assert!(matches!(
        provider.transmit(Instant::ZERO),
        TransmitOutcome::Ready(_)
    ));
}

#[test]
fn unconsumed_rx_drop_restores_ready_frame() {
    let mut provider = DeterministicProvider::new();
    provider.rx.inject(&[9, 8]);

    let ReceiveOutcome::Ready { rx, tx } = provider.receive(Instant::ZERO) else {
        panic!("injected frame must be available");
    };
    drop(rx);
    drop(tx);

    assert_eq!(provider.rx.slot, RxSlot::Ready);
    assert_eq!(provider.rx.cancellations, 1);
    assert!(matches!(
        provider.receive(Instant::ZERO),
        ReceiveOutcome::Ready { .. }
    ));
}

#[test]
fn unconsumed_tx_drop_restores_credit() {
    let mut provider = DeterministicProvider::new();

    let TransmitOutcome::Ready(tx) = provider.transmit(Instant::ZERO) else {
        panic!("initial transmit credit must be available");
    };
    drop(tx);

    assert_eq!(provider.tx.slot, TxSlot::Available);
    assert_eq!(provider.tx.cancellations, 1);
    assert!(matches!(
        provider.transmit(Instant::ZERO),
        TransmitOutcome::Ready(_)
    ));
}

#[test]
fn oversized_tx_is_rejected_before_callback_and_restores_credit() {
    let mut provider = DeterministicProvider::new();
    let TransmitOutcome::Ready(tx) = provider.transmit(Instant::ZERO) else {
        panic!("initial transmit credit must be available");
    };
    let mut callback_ran = false;

    let error = tx
        .consume(FRAME_CAPACITY + 1, |_| callback_ran = true)
        .unwrap_err();

    assert!(!callback_ran);
    assert_eq!(error.requested(), FRAME_CAPACITY + 1);
    assert_eq!(error.capacity(), FRAME_CAPACITY);
    assert_eq!(provider.tx.slot, TxSlot::Available);
    assert_eq!(provider.tx.rejections, 1);
    assert_eq!(provider.tx.submissions, 0);
}

#[test]
fn link_and_manual_clock_remain_provider_owned() {
    let mut provider = DeterministicProvider::new();
    let mut clock = ManualClock::new();
    clock.advance(Duration::from_micros(25));
    provider.facts.link_state = LinkState::Down;

    assert!(matches!(
        provider.transmit(clock.now),
        TransmitOutcome::LinkUnavailable
    ));
    assert_eq!(provider.observed_at, clock.now);
    assert_eq!(provider.link_state(), LinkState::Down);
    assert_eq!(provider.capabilities().max_frame_len, FRAME_CAPACITY);
}

fn build_icmp_echo_request(
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

fn build_arp_request(source_mac: [u8; 6], source_ip: [u8; 4], destination_ip: [u8; 4]) -> Vec<u8> {
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

fn prime_neighbor(
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

fn assert_icmp_echo_reply(
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

#[test]
fn real_stack_echo_obeys_finite_ingress_budget() {
    const LOCAL_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 1];
    const PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 0, 2];
    const LOCAL_IP: [u8; 4] = [10, 0, 0, 2];
    const PEER_IP: [u8; 4] = [10, 0, 0, 1];

    let mut stack = Stack::new();
    let mut provider = DeterministicProvider::with_mac(LOCAL_MAC);
    let interface = stack.add_interface(
        &mut provider,
        EthernetAddress::new(LOCAL_MAC),
        Instant::ZERO,
    );
    stack
        .configure_ipv4_for_host_validation(interface, LOCAL_IP, 24)
        .unwrap();
    prime_neighbor(
        &mut stack,
        interface,
        &mut provider,
        PEER_MAC,
        PEER_IP,
        LOCAL_IP,
    );
    provider.rx.inject(&build_icmp_echo_request(
        PEER_MAC, LOCAL_MAC, PEER_IP, LOCAL_IP,
    ));

    let outcome = stack
        .pump(
            interface,
            &mut provider,
            Instant::from_micros(25),
            PumpBudget::new(1, 1),
        )
        .unwrap();

    assert_eq!(provider.receive_calls, 1);
    assert!(outcome.work_remaining);
    assert_eq!(outcome.recheck, anemone_net_api::Recheck::Immediate);
    assert_eq!(outcome.next_deadline, None);
    assert_eq!(provider.tx.slot, TxSlot::Submitted);
    assert_icmp_echo_reply(
        &provider.tx.backing[..provider.tx.len],
        LOCAL_MAC,
        PEER_MAC,
        LOCAL_IP,
        PEER_IP,
    );

    provider.tx.complete();
    let idle = stack
        .pump(
            interface,
            &mut provider,
            Instant::from_micros(26),
            PumpBudget::new(1, 1),
        )
        .unwrap();
    assert!(!idle.work_remaining);
    assert_eq!(idle.recheck, anemone_net_api::Recheck::Idle);
    assert_eq!(idle.next_deadline, None);
}

#[test]
fn two_stack_provider_pairs_keep_identity_credit_output_and_time_isolated() {
    const FIRST_MAC: [u8; 6] = [0x02, 0, 0, 0, 1, 1];
    const SECOND_MAC: [u8; 6] = [0x02, 0, 0, 0, 2, 1];
    const PEER_MAC: [u8; 6] = [0x02, 0, 0, 0, 3, 1];
    const FIRST_IP: [u8; 4] = [10, 0, 1, 2];
    const SECOND_IP: [u8; 4] = [10, 0, 2, 2];
    const PEER_IP: [u8; 4] = [10, 0, 1, 1];

    let mut first_stack = Stack::new();
    let mut second_stack = Stack::new();
    let mut first_provider = DeterministicProvider::with_mac(FIRST_MAC);
    let mut second_provider = DeterministicProvider::with_mac(SECOND_MAC);
    let first_id = first_stack.add_interface(
        &mut first_provider,
        EthernetAddress::new(FIRST_MAC),
        Instant::ZERO,
    );
    let second_id = second_stack.add_interface(
        &mut second_provider,
        EthernetAddress::new(SECOND_MAC),
        Instant::ZERO,
    );
    first_stack
        .configure_ipv4_for_host_validation(first_id, FIRST_IP, 24)
        .unwrap();
    second_stack
        .configure_ipv4_for_host_validation(second_id, SECOND_IP, 24)
        .unwrap();

    first_provider.rx.inject(&build_icmp_echo_request(
        PEER_MAC, FIRST_MAC, PEER_IP, FIRST_IP,
    ));
    first_stack
        .pump(
            first_id,
            &mut first_provider,
            Instant::from_micros(5),
            PumpBudget::new(2, 1),
        )
        .unwrap();
    second_stack
        .pump(
            second_id,
            &mut second_provider,
            Instant::from_micros(99),
            PumpBudget::new(2, 1),
        )
        .unwrap();

    assert_eq!(first_provider.tx.slot, TxSlot::Submitted);
    assert_eq!(second_provider.tx.slot, TxSlot::Available);
    assert_eq!(first_provider.observed_at, Instant::from_micros(5));
    assert_eq!(second_provider.observed_at, Instant::from_micros(99));
    assert_eq!(first_provider.tx.submissions, 1);
    assert_eq!(second_provider.tx.submissions, 0);
}

struct BlockingProvider {
    inner: DeterministicProvider,
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}

impl FrameProvider for BlockingProvider {
    type RxToken<'a> = DeterministicRxToken<'a>;
    type TxToken<'a> = DeterministicTxToken<'a>;

    fn receive(&mut self, now: Instant) -> ReceiveOutcome<Self::RxToken<'_>, Self::TxToken<'_>> {
        self.entered.send(()).unwrap();
        self.release.recv().unwrap();
        self.inner.receive(now)
    }

    fn transmit(&mut self, now: Instant) -> TransmitOutcome<Self::TxToken<'_>> {
        self.inner.transmit(now)
    }

    fn capabilities(&self) -> FrameCapabilities {
        self.inner.capabilities()
    }

    fn link_state(&self) -> LinkState {
        self.inner.link_state()
    }
}

#[test]
fn outer_runtime_mutex_controls_competing_pump_admission() {
    let mut stack = Stack::new();
    let mut setup_provider = DeterministicProvider::new();
    let interface = stack.add_interface(
        &mut setup_provider,
        EthernetAddress::new([0x02, 0, 0, 0, 0, 1]),
        Instant::ZERO,
    );
    let stack = Arc::new(Mutex::new(stack));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mut blocking_provider = BlockingProvider {
        inner: DeterministicProvider::new(),
        entered: entered_tx,
        release: release_rx,
    };
    let first_stack = Arc::clone(&stack);
    let first = std::thread::spawn(move || {
        first_stack.lock().unwrap().pump(
            interface,
            &mut blocking_provider,
            Instant::ZERO,
            PumpBudget::new(1, 1),
        )
    });

    entered_rx.recv().unwrap();
    assert!(matches!(stack.try_lock(), Err(TryLockError::WouldBlock)));
    release_tx.send(()).unwrap();
    assert!(first.join().unwrap().is_ok());
}
