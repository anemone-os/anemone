use crate::{
    device::{
        console::Console,
        discovery::fwnode::InterruptSelector,
        tty::{
            TtyLineSnapshot, TtyParity, TtyPort, TtyPortAttachment, TtyPortId, TtyRxNotifier,
            TtyRxUnit, attach_unpublished_port,
        },
    },
    exception::intr::request_irq_selected,
    mm::remap::IoRemap,
    prelude::*,
    utils::{any_opaque::AnyOpaque, ring_buffer::RingBuffer},
};

use super::{
    UartLineConfig,
    regs::{InterruptReason, Ns16550ARegisters, RxSample, UartVariant},
};

static_assert!(
    TTY_RAW_RX_CAPACITY_BYTES > 0,
    "TTY_RAW_RX_CAPACITY_BYTES must be non-zero"
);
static_assert!(
    NS16550A_IRQ_RX_BUDGET_BYTES > 0,
    "NS16550A_IRQ_RX_BUDGET_BYTES must be non-zero"
);
static_assert!(
    NS16550A_TX_BATCH_BYTES > 0,
    "NS16550A_TX_BATCH_BYTES must be non-zero"
);
static_assert!(
    NS16550A_TX_POLL_ITERATIONS > 0,
    "NS16550A_TX_POLL_ITERATIONS must be non-zero"
);

#[derive(Debug, Clone, Copy)]
pub(super) struct AppliedLine {
    /// Immutable boot-applied line truth. Stage 1 does not expose it yet, but a
    /// later termios snapshot must use this value rather than reread registers.
    config: UartLineConfig,
    divisor: u16,
}

impl AppliedLine {
    pub(super) fn new(config: UartLineConfig, divisor: u16) -> Self {
        Self { config, divisor }
    }

    fn tty_snapshot(self) -> TtyLineSnapshot {
        TtyLineSnapshot {
            baud: self.config.baud,
            parity: match self.config.parity {
                super::UartParity::None => TtyParity::None,
                super::UartParity::Odd => TtyParity::Odd,
                super::UartParity::Even => TtyParity::Even,
            },
            data_bits: self.config.data_bits,
        }
    }
}

struct RawRx {
    fifo: RingBuffer<TtyRxUnit, TTY_RAW_RX_CAPACITY_BYTES>,
}

impl RawRx {
    fn new() -> Self {
        Self {
            fifo: RingBuffer::new(),
        }
    }

    fn publish(&mut self, units: &[TtyRxUnit]) -> RawPublication {
        let was_empty = self.fifo.is_empty();
        let accepted = self.fifo.try_push_slice(units);
        RawPublication {
            accepted,
            dropped: units.len() - accepted,
            became_nonempty: was_empty && accepted != 0,
        }
    }

    fn dequeue(&mut self, dst: &mut [TtyRxUnit]) -> usize {
        self.fifo.try_pop_slice(dst)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RawPublication {
    accepted: usize,
    dropped: usize,
    became_nonempty: bool,
}

struct PortCounters {
    /// Diagnostic only; these atomics never drive predicates, ordering, or a
    /// lifecycle transition.
    rx_accepted: AtomicUsize,
    rx_dropped: AtomicUsize,
    rx_breaks: AtomicUsize,
    rx_faults: AtomicUsize,
    rx_overruns: AtomicUsize,
    irq_budget_exhaustions: AtomicUsize,
    notifications: AtomicUsize,
    tx_accepted: AtomicUsize,
    tx_timeouts: AtomicUsize,
    console_unsubmitted: AtomicUsize,
}

impl PortCounters {
    fn new() -> Self {
        Self {
            rx_accepted: AtomicUsize::new(0),
            rx_dropped: AtomicUsize::new(0),
            rx_breaks: AtomicUsize::new(0),
            rx_faults: AtomicUsize::new(0),
            rx_overruns: AtomicUsize::new(0),
            irq_budget_exhaustions: AtomicUsize::new(0),
            notifications: AtomicUsize::new(0),
            tx_accepted: AtomicUsize::new(0),
            tx_timeouts: AtomicUsize::new(0),
            console_unsubmitted: AtomicUsize::new(0),
        }
    }
}

pub(super) struct Uart16550Port {
    id: TtyPortId,
    /// Stable diagnostic identity for logs and review. It does not decide port
    /// behavior or replace the OF-path `TtyPortId`.
    base: PhysAddr,
    reg_shift: usize,
    reg_io_width: usize,
    variant: UartVariant,
    remap: IoRemap,
    /// Authoritative boot-applied configuration snapshot, not a diagnostic
    /// cache. Runtime register reads must not replace it.
    applied_line: AppliedLine,
    raw_rx: SpinLock<Box<RawRx>>,
    tx: SpinLock<()>,
    counters: PortCounters,
}

impl Uart16550Port {
    fn new(
        id: TtyPortId,
        base: PhysAddr,
        reg_shift: usize,
        reg_io_width: usize,
        variant: UartVariant,
        remap: IoRemap,
        applied_line: AppliedLine,
    ) -> Result<Arc<Self>, SysError> {
        let raw_rx = Box::try_new(RawRx::new()).map_err(|_| SysError::OutOfMemory)?;
        Arc::try_new(Self {
            id,
            base,
            reg_shift,
            reg_io_width,
            variant,
            remap,
            applied_line,
            raw_rx: SpinLock::new(raw_rx),
            tx: SpinLock::new(()),
            counters: PortCounters::new(),
        })
        .map_err(|_| SysError::OutOfMemory)
    }

    fn regs(&self) -> Ns16550ARegisters {
        unsafe {
            Ns16550ARegisters::from_raw_variant(
                self.remap.as_ptr().as_ptr().cast(),
                self.reg_shift,
                self.reg_io_width,
                self.variant,
            )
        }
    }

    pub(super) fn id(&self) -> &TtyPortId {
        &self.id
    }

    pub(super) fn base(&self) -> PhysAddr {
        self.base
    }

    fn submit_tx_bytes(&self, src: &[u8]) -> usize {
        let regs = self.regs();
        let mut accepted = 0;

        for batch in src.chunks(NS16550A_TX_BATCH_BYTES) {
            let progress = {
                let _guard = self.tx.lock_irqsave();
                submit_tx_batch(batch, NS16550A_TX_POLL_ITERATIONS, |byte| {
                    regs.write_byte(byte).is_some()
                })
            };
            accepted += progress.accepted;
            if progress.timed_out {
                self.counters.tx_timeouts.fetch_add(1, Ordering::Relaxed);
                break;
            }
        }

        self.counters
            .tx_accepted
            .fetch_add(accepted, Ordering::Relaxed);
        accepted
    }

    fn handle_irq(&self, notifier: &TtyRxNotifier) {
        let regs = self.regs();
        let mut batch = [TtyRxUnit::Byte(0); NS16550A_IRQ_RX_BUDGET_BYTES];
        let mut units = 0;
        let mut breaks = 0;
        let mut faults = 0;
        let mut overruns = 0;
        let mut causes = 0;
        let mut budget_exhausted = false;

        while causes < NS16550A_IRQ_RX_BUDGET_BYTES && units < batch.len() {
            match regs.interrupt_reason() {
                InterruptReason::None => break,
                InterruptReason::RxAvailable | InterruptReason::RxLineStatus => {
                    causes += 1;
                    let drained = drain_samples(&mut batch[units..], || regs.read_rx_sample());
                    units += drained.units;
                    breaks += drained.breaks;
                    faults += drained.faults;
                    overruns += drained.overruns;
                    if units == batch.len() {
                        // LSR error/condition bits are read-clear and belong to
                        // the next FIFO sample. A full software batch already
                        // proves more IRQ work may remain, so do not inspect and
                        // destroy the next sample's classification here.
                        budget_exhausted = true;
                        break;
                    }
                },
                InterruptReason::RxTimeout => {
                    causes += 1;
                    let drained = drain_samples(&mut batch[units..], || regs.read_rx_sample());
                    units += drained.units;
                    breaks += drained.breaks;
                    faults += drained.faults;
                    overruns += drained.overruns;
                    if drained.units == 0 {
                        let _ = regs.clear_spurious_rx_timeout();
                    }
                    if units == batch.len() {
                        // As above, leave the next sample's read-clear LSR bits
                        // for the drain that will also consume its payload.
                        budget_exhausted = true;
                        break;
                    }
                },
                InterruptReason::ModemStatus => {
                    causes += 1;
                    regs.clear_modem_status();
                },
                InterruptReason::BusyDetect => {
                    causes += 1;
                    regs.clear_busy_detect();
                },
                // TX interrupts are disabled and unknown causes have no bounded
                // owner-local acknowledgement. Stop rather than spin in IRQ.
                InterruptReason::TxHoldingEmpty | InterruptReason::Unknown => break,
            }
        }

        if causes == NS16550A_IRQ_RX_BUDGET_BYTES && regs.interrupt_pending() {
            budget_exhausted = true;
        }

        self.counters.rx_breaks.fetch_add(breaks, Ordering::Relaxed);
        self.counters.rx_faults.fetch_add(faults, Ordering::Relaxed);
        self.counters
            .rx_overruns
            .fetch_add(overruns, Ordering::Relaxed);
        if budget_exhausted {
            self.counters
                .irq_budget_exhaustions
                .fetch_add(1, Ordering::Relaxed);
        }

        let publication = {
            let mut raw_rx = self.raw_rx.lock_irqsave();
            raw_rx.publish(&batch[..units])
        };
        self.counters
            .rx_accepted
            .fetch_add(publication.accepted, Ordering::Relaxed);
        self.counters
            .rx_dropped
            .fetch_add(publication.dropped, Ordering::Relaxed);

        if publication.became_nonempty {
            self.counters.notifications.fetch_add(1, Ordering::Relaxed);
            notifier.wake();
        }
    }
}

struct Uart16550TtyPort {
    port: Arc<Uart16550Port>,
}

impl TtyPort for Uart16550TtyPort {
    fn id(&self) -> &TtyPortId {
        self.port.id()
    }

    fn rx_pending(&self) -> bool {
        !self.port.raw_rx.lock_irqsave().fifo.is_empty()
    }

    fn dequeue_rx(&self, dst: &mut [TtyRxUnit]) -> usize {
        self.port.raw_rx.lock_irqsave().dequeue(dst)
    }

    fn submit_tx(&self, src: &[u8]) -> usize {
        self.port.submit_tx_bytes(src)
    }

    fn tx_idle(&self) -> bool {
        self.port.regs().tx_idle()
    }
}

struct Uart16550Console {
    port: Arc<Uart16550Port>,
}

impl Console for Uart16550Console {
    fn output(&self, s: &str) {
        let accepted = self.port.submit_tx_bytes(s.as_bytes());
        self.port
            .counters
            .console_unsubmitted
            .fetch_add(s.len() - accepted, Ordering::Relaxed);
    }
}

/// Driver-local state installed by the early synchronous probe.
///
/// `attachment == None` is the sole Quiescent truth; `Some` is the sole Active
/// truth. The attachment lives here rather than in `Uart16550Port`, avoiding a
/// `port -> attachment -> endpoint -> port` strong-reference cycle.
#[derive(Opaque)]
pub(super) struct Uart16550Device {
    port: Arc<Uart16550Port>,
    tty_port: Arc<Uart16550TtyPort>,
    attachment: SpinLock<Option<TtyPortAttachment>>,
}

impl Uart16550Device {
    pub(super) fn new(
        id: TtyPortId,
        base: PhysAddr,
        reg_shift: usize,
        reg_io_width: usize,
        variant: UartVariant,
        remap: IoRemap,
        applied_line: AppliedLine,
    ) -> Result<(Self, Arc<dyn Console>), SysError> {
        let port = Uart16550Port::new(
            id,
            base,
            reg_shift,
            reg_io_width,
            variant,
            remap,
            applied_line,
        )?;
        let tty_port = Arc::try_new(Uart16550TtyPort { port: port.clone() })
            .map_err(|_| SysError::OutOfMemory)?;
        let console: Arc<dyn Console> = Arc::try_new(Uart16550Console { port: port.clone() })
            .map_err(|_| SysError::OutOfMemory)?;
        Ok((
            Self {
                port,
                tty_port,
                attachment: SpinLock::new(None),
            },
            console,
        ))
    }

    pub(super) fn port(&self) -> &Arc<Uart16550Port> {
        &self.port
    }

    pub(super) fn activate(&self, device: &dyn Device) -> Result<(), SysError> {
        assert!(
            self.attachment.lock_irqsave().is_none(),
            "UART16550 TTY transport activated twice"
        );

        let tty_port: Arc<dyn TtyPort> = self.tty_port.clone();
        let (attachment, notifier) =
            attach_unpublished_port(tty_port, self.port.applied_line.tty_snapshot())?;
        let irq_context = AnyOpaque::new(Uart16550IrqContext {
            port: self.port.clone(),
            notifier,
        });

        if let Err(error) = request_irq_selected(
            device,
            InterruptSelector::Index(0),
            None,
            &IRQ_HANDLER,
            Some(irq_context),
        ) {
            attachment.abort();
            return Err(error);
        }

        {
            let mut slot = self.attachment.lock_irqsave();
            assert!(
                slot.is_none(),
                "UART16550 activation slot changed during commit"
            );
            *slot = Some(attachment);
        }
        self.port.regs().enable_rx_irq();
        Ok(())
    }
}

#[derive(Opaque)]
struct Uart16550IrqContext {
    port: Arc<Uart16550Port>,
    notifier: TtyRxNotifier,
}

pub(super) static IRQ_HANDLER: IrqHandler = IrqHandler::new(handle_irq);

fn handle_irq(private: &AnyOpaque) {
    let context = private
        .cast::<Uart16550IrqContext>()
        .expect("UART16550 IRQ received invalid private data");
    context.port.handle_irq(&context.notifier);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SampleDrain {
    units: usize,
    breaks: usize,
    faults: usize,
    overruns: usize,
}

fn drain_samples(dst: &mut [TtyRxUnit], mut read_sample: impl FnMut() -> RxSample) -> SampleDrain {
    let mut units = 0;
    let mut breaks = 0;
    let mut faults = 0;
    let mut overruns = 0;
    while units < dst.len() {
        let sample = read_sample();
        let stop_after_sample = sample.byte.is_none();
        overruns += usize::from(sample.overrun);
        breaks += usize::from(sample.break_received);
        faults += usize::from(!sample.break_received && sample.parity_or_framing);
        let unit = if sample.break_received {
            Some(TtyRxUnit::Break)
        } else {
            sample.byte.map(|byte| {
                if sample.parity_or_framing {
                    TtyRxUnit::FaultedByte(byte)
                } else {
                    TtyRxUnit::Byte(byte)
                }
            })
        };
        let Some(unit) = unit else {
            break;
        };
        dst[units] = unit;
        units += 1;
        if stop_after_sample {
            // A synthetic/no-payload break is one ordered condition. Stop the
            // drain so a source that cannot advance without payload cannot
            // duplicate that condition in the same IRQ batch.
            break;
        }
    }
    SampleDrain {
        units,
        breaks,
        faults,
        overruns,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TxBatchProgress {
    accepted: usize,
    timed_out: bool,
}

fn submit_tx_batch(
    src: &[u8],
    poll_iterations: usize,
    mut try_write: impl FnMut(u8) -> bool,
) -> TxBatchProgress {
    assert!(poll_iterations != 0, "TX poll bound must be non-zero");
    let mut accepted = 0;
    for &byte in src {
        let mut written = false;
        for _ in 0..poll_iterations {
            if try_write(byte) {
                written = true;
                break;
            }
        }
        if !written {
            return TxBatchProgress {
                accepted,
                timed_out: true,
            };
        }
        accepted += 1;
    }
    TxBatchProgress {
        accepted,
        timed_out: false,
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn raw_rx_preserves_fifo_and_notifies_only_on_empty_transition() {
        let mut raw = RawRx::new();
        assert_eq!(
            raw.publish(&[
                TtyRxUnit::Byte(1),
                TtyRxUnit::Break,
                TtyRxUnit::FaultedByte(3),
            ]),
            RawPublication {
                accepted: 3,
                dropped: 0,
                became_nonempty: true,
            }
        );
        assert_eq!(
            raw.publish(&[TtyRxUnit::Byte(4), TtyRxUnit::Byte(5)]),
            RawPublication {
                accepted: 2,
                dropped: 0,
                became_nonempty: false,
            }
        );

        let mut observed = [TtyRxUnit::Byte(0); 5];
        assert_eq!(raw.dequeue(&mut observed), observed.len());
        assert_eq!(
            observed,
            [
                TtyRxUnit::Byte(1),
                TtyRxUnit::Break,
                TtyRxUnit::FaultedByte(3),
                TtyRxUnit::Byte(4),
                TtyRxUnit::Byte(5),
            ]
        );
        assert!(raw.fifo.is_empty());
    }

    #[kunit]
    fn raw_rx_full_queue_drops_new_units() {
        let mut raw = RawRx::new();
        let chunk = [TtyRxUnit::Byte(0x5a); 64];
        while !raw.fifo.is_full() {
            let available = raw.fifo.available().min(chunk.len());
            let publication = raw.publish(&chunk[..available]);
            assert_eq!(publication.dropped, 0);
        }
        assert_eq!(
            raw.publish(&[TtyRxUnit::Break]),
            RawPublication {
                accepted: 0,
                dropped: 1,
                became_nonempty: false,
            }
        );
    }

    #[kunit]
    fn rx_sample_drain_classifies_ordered_conditions_and_counts_diagnostics() {
        let samples = [
            RxSample {
                byte: Some(0x11),
                break_received: false,
                parity_or_framing: false,
                overrun: false,
            },
            RxSample {
                byte: Some(0x22),
                break_received: false,
                parity_or_framing: true,
                overrun: true,
            },
            RxSample {
                byte: Some(0x33),
                break_received: true,
                parity_or_framing: true,
                overrun: true,
            },
        ];
        let mut next = 0;
        let mut dst = [TtyRxUnit::Byte(0); 2];
        let drained = drain_samples(&mut dst, || {
            let sample = samples[next];
            next += 1;
            sample
        });
        assert_eq!(drained.units, 2);
        assert_eq!(drained.breaks, 0);
        assert_eq!(drained.faults, 1);
        assert_eq!(drained.overruns, 1);
        assert_eq!(dst, [TtyRxUnit::Byte(0x11), TtyRxUnit::FaultedByte(0x22)]);
        assert_eq!(next, 2, "the third sample must remain for later IRQ work");

        let mut break_without_data = false;
        let mut units = [TtyRxUnit::Byte(0); 2];
        let drained = drain_samples(&mut units, || {
            assert!(!break_without_data);
            break_without_data = true;
            RxSample {
                byte: None,
                break_received: true,
                parity_or_framing: true,
                overrun: true,
            }
        });
        assert_eq!(drained.units, 1);
        assert_eq!(drained.breaks, 1);
        assert_eq!(drained.faults, 0, "break classification has priority");
        assert_eq!(drained.overruns, 1);
        assert_eq!(units, [TtyRxUnit::Break, TtyRxUnit::Byte(0)]);

        let drained = drain_samples(&mut [TtyRxUnit::Byte(0); 1], || RxSample {
            byte: None,
            break_received: false,
            parity_or_framing: false,
            overrun: true,
        });
        assert_eq!(drained.units, 0);
        assert_eq!(drained.overruns, 1);
    }

    #[kunit]
    fn tx_batch_returns_partial_progress_at_poll_timeout() {
        let mut attempts = 0;
        let progress = submit_tx_batch(&[0x41, 0x42, 0x43], 3, |byte| {
            attempts += 1;
            byte == 0x41
        });
        assert_eq!(
            progress,
            TxBatchProgress {
                accepted: 1,
                timed_out: true,
            }
        );
        assert_eq!(attempts, 4, "one success plus three bounded retries");
    }
}
