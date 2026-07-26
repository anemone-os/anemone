use anemone_net_api::{
    Duration, EthernetAddress, FrameCapabilities, FrameProvider, FrameSizeError, Instant,
    InterfaceFacts, LinkState, ReceiveOutcome, RxToken, TransmitOutcome, TxToken,
};

const FRAME_CAPACITY: usize = 64;

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
}

impl DeterministicProvider {
    fn new() -> Self {
        Self {
            rx: RxLane::new(),
            tx: TxLane::new(),
            facts: InterfaceFacts {
                ethernet_address: Some(EthernetAddress::new([0x02, 0, 0, 0, 0, 1])),
                max_frame_len: FRAME_CAPACITY,
                link_state: LinkState::Up,
            },
            observed_at: Instant::ZERO,
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
