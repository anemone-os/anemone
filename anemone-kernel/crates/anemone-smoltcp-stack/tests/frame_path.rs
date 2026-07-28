mod support;

use anemone_net_api::{
    Duration, EthernetAddress, FrameCapabilities, FrameProvider, Instant, LinkState,
    ReceiveOutcome, RxToken, TransmitOutcome, TxToken,
};
use anemone_smoltcp_stack::{PumpBudget, Stack};
use std::sync::{Arc, Mutex, TryLockError, mpsc};

use support::*;

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
