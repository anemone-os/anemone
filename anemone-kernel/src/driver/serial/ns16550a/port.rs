use crate::{
    device::{
        console::Console,
        tty::{
            TtyLineSnapshot, TtyParity, TtyPort, TtyPortAttachment, TtyPortId, TtyRxNotifier,
            attach_unpublished_port,
        },
    },
    mm::remap::IoRemap,
    prelude::*,
    utils::{any_opaque::AnyOpaque, ring_buffer::RingBuffer},
};

use super::{
    UartLineConfig,
    regs::{InterruptReason, Ns16550ARegisters, RxSample},
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
    fifo: RingBuffer<u8, TTY_RAW_RX_CAPACITY_BYTES>,
}

impl RawRx {
    fn new() -> Self {
        Self {
            fifo: RingBuffer::new(),
        }
    }

    fn publish(&mut self, bytes: &[u8]) -> RawPublication {
        let was_empty = self.fifo.is_empty();
        let accepted = self.fifo.try_push_slice(bytes);
        RawPublication {
            accepted,
            dropped: bytes.len() - accepted,
            became_nonempty: was_empty && accepted != 0,
        }
    }

    fn dequeue(&mut self, dst: &mut [u8]) -> usize {
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
    line_errors: AtomicUsize,
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
            line_errors: AtomicUsize::new(0),
            irq_budget_exhaustions: AtomicUsize::new(0),
            notifications: AtomicUsize::new(0),
            tx_accepted: AtomicUsize::new(0),
            tx_timeouts: AtomicUsize::new(0),
            console_unsubmitted: AtomicUsize::new(0),
        }
    }
}

pub(super) struct Ns16550APort {
    id: TtyPortId,
    /// Stable diagnostic identity for logs and review. It does not decide port
    /// behavior or replace the OF-path `TtyPortId`.
    base: PhysAddr,
    reg_shift: usize,
    reg_io_width: usize,
    remap: IoRemap,
    /// Authoritative boot-applied configuration snapshot, not a diagnostic
    /// cache. Runtime register reads must not replace it.
    applied_line: AppliedLine,
    raw_rx: SpinLock<Box<RawRx>>,
    tx: SpinLock<()>,
    counters: PortCounters,
}

impl Ns16550APort {
    fn new(
        id: TtyPortId,
        base: PhysAddr,
        reg_shift: usize,
        reg_io_width: usize,
        remap: IoRemap,
        applied_line: AppliedLine,
    ) -> Result<Arc<Self>, SysError> {
        let raw_rx = Box::try_new(RawRx::new()).map_err(|_| SysError::OutOfMemory)?;
        Arc::try_new(Self {
            id,
            base,
            reg_shift,
            reg_io_width,
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
            Ns16550ARegisters::from_raw(
                self.remap.as_ptr().as_ptr().cast(),
                self.reg_shift,
                self.reg_io_width,
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
        let mut batch = [0_u8; NS16550A_IRQ_RX_BUDGET_BYTES];
        let mut bytes = 0;
        let mut line_errors = 0;
        let mut causes = 0;
        let mut budget_exhausted = false;

        while causes < NS16550A_IRQ_RX_BUDGET_BYTES && bytes < batch.len() {
            match regs.interrupt_reason() {
                InterruptReason::None => break,
                InterruptReason::RxAvailable
                | InterruptReason::RxLineStatus
                | InterruptReason::RxTimeout => {
                    causes += 1;
                    let drained = drain_samples(&mut batch[bytes..], || regs.read_rx_sample());
                    bytes += drained.bytes;
                    line_errors += drained.line_errors;
                    if bytes == batch.len() {
                        // The next FIFO entry's line status belongs to the next
                        // drain. Count it only when that byte is consumed.
                        budget_exhausted = regs.rx_status().data_ready;
                        break;
                    }
                },
                InterruptReason::ModemStatus => {
                    causes += 1;
                    regs.clear_modem_status();
                },
                // TX interrupts are disabled and unknown causes have no bounded
                // owner-local acknowledgement. Stop rather than spin in IRQ.
                InterruptReason::TxHoldingEmpty | InterruptReason::Unknown => break,
            }
        }

        if causes == NS16550A_IRQ_RX_BUDGET_BYTES && regs.interrupt_pending() {
            budget_exhausted = true;
        }

        self.counters
            .line_errors
            .fetch_add(line_errors, Ordering::Relaxed);
        if budget_exhausted {
            self.counters
                .irq_budget_exhaustions
                .fetch_add(1, Ordering::Relaxed);
        }

        let publication = {
            let mut raw_rx = self.raw_rx.lock_irqsave();
            raw_rx.publish(&batch[..bytes])
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

struct Ns16550ATtyPort {
    port: Arc<Ns16550APort>,
}

impl TtyPort for Ns16550ATtyPort {
    fn id(&self) -> &TtyPortId {
        self.port.id()
    }

    fn line_snapshot(&self) -> TtyLineSnapshot {
        self.port.applied_line.tty_snapshot()
    }

    fn rx_pending(&self) -> bool {
        !self.port.raw_rx.lock_irqsave().fifo.is_empty()
    }

    fn dequeue_rx(&self, dst: &mut [u8]) -> usize {
        self.port.raw_rx.lock_irqsave().dequeue(dst)
    }

    fn submit_tx(&self, src: &[u8]) -> usize {
        self.port.submit_tx_bytes(src)
    }

    fn tx_idle(&self) -> bool {
        self.port.regs().tx_idle()
    }
}

struct Ns16550AConsole {
    port: Arc<Ns16550APort>,
}

impl Console for Ns16550AConsole {
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
/// truth. The attachment lives here rather than in `Ns16550APort`, avoiding a
/// `port -> attachment -> endpoint -> port` strong-reference cycle.
#[derive(Opaque)]
pub(super) struct Ns16550ADevice {
    port: Arc<Ns16550APort>,
    tty_port: Arc<Ns16550ATtyPort>,
    attachment: SpinLock<Option<TtyPortAttachment>>,
}

impl Ns16550ADevice {
    pub(super) fn new(
        id: TtyPortId,
        base: PhysAddr,
        reg_shift: usize,
        reg_io_width: usize,
        remap: IoRemap,
        applied_line: AppliedLine,
    ) -> Result<(Self, Arc<dyn Console>), SysError> {
        let port = Ns16550APort::new(id, base, reg_shift, reg_io_width, remap, applied_line)?;
        let tty_port = Arc::try_new(Ns16550ATtyPort { port: port.clone() })
            .map_err(|_| SysError::OutOfMemory)?;
        let console: Arc<dyn Console> = Arc::try_new(Ns16550AConsole { port: port.clone() })
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

    pub(super) fn port(&self) -> &Arc<Ns16550APort> {
        &self.port
    }

    pub(super) fn activate(&self, device: &dyn Device) -> Result<(), SysError> {
        assert!(
            self.attachment.lock_irqsave().is_none(),
            "NS16550A TTY transport activated twice"
        );

        let tty_port: Arc<dyn TtyPort> = self.tty_port.clone();
        let (attachment, notifier) = attach_unpublished_port(tty_port)?;
        let irq_context = AnyOpaque::new(Ns16550AIrqContext {
            port: self.port.clone(),
            notifier,
        });

        if let Err(error) = request_irq(device, &IRQ_HANDLER, Some(irq_context)) {
            attachment.abort();
            return Err(error);
        }

        {
            let mut slot = self.attachment.lock_irqsave();
            assert!(
                slot.is_none(),
                "NS16550A activation slot changed during commit"
            );
            *slot = Some(attachment);
        }
        self.port.regs().enable_rx_irq();
        Ok(())
    }
}

#[derive(Opaque)]
struct Ns16550AIrqContext {
    port: Arc<Ns16550APort>,
    notifier: TtyRxNotifier,
}

pub(super) static IRQ_HANDLER: IrqHandler = IrqHandler::new(handle_irq);

fn handle_irq(private: &AnyOpaque) {
    let context = private
        .cast::<Ns16550AIrqContext>()
        .expect("NS16550A IRQ received invalid private data");
    context.port.handle_irq(&context.notifier);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SampleDrain {
    bytes: usize,
    line_errors: usize,
}

fn drain_samples(dst: &mut [u8], mut read_sample: impl FnMut() -> RxSample) -> SampleDrain {
    let mut bytes = 0;
    let mut line_errors = 0;
    while bytes < dst.len() {
        let sample = read_sample();
        line_errors += usize::from(sample.line_error);
        let Some(byte) = sample.byte else {
            break;
        };
        dst[bytes] = byte;
        bytes += 1;
    }
    SampleDrain { bytes, line_errors }
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
            raw.publish(&[1, 2, 3]),
            RawPublication {
                accepted: 3,
                dropped: 0,
                became_nonempty: true,
            }
        );
        assert_eq!(
            raw.publish(&[4, 5]),
            RawPublication {
                accepted: 2,
                dropped: 0,
                became_nonempty: false,
            }
        );

        let mut observed = [0_u8; 5];
        assert_eq!(raw.dequeue(&mut observed), observed.len());
        assert_eq!(observed, [1, 2, 3, 4, 5]);
        assert!(raw.fifo.is_empty());
    }

    #[kunit]
    fn raw_rx_full_queue_drops_new_bytes() {
        let mut raw = RawRx::new();
        let chunk = [0x5a_u8; 64];
        while !raw.fifo.is_full() {
            let available = raw.fifo.available().min(chunk.len());
            let publication = raw.publish(&chunk[..available]);
            assert_eq!(publication.dropped, 0);
        }
        assert_eq!(
            raw.publish(&[0xa5]),
            RawPublication {
                accepted: 0,
                dropped: 1,
                became_nonempty: false,
            }
        );
    }

    #[kunit]
    fn rx_sample_drain_obeys_budget_and_counts_line_errors() {
        let samples = [
            RxSample {
                byte: Some(0x11),
                line_error: true,
            },
            RxSample {
                byte: Some(0x22),
                line_error: false,
            },
            RxSample {
                byte: Some(0x33),
                line_error: true,
            },
        ];
        let mut next = 0;
        let mut dst = [0_u8; 2];
        let drained = drain_samples(&mut dst, || {
            let sample = samples[next];
            next += 1;
            sample
        });
        assert_eq!(drained.bytes, 2);
        assert_eq!(drained.line_errors, 1);
        assert_eq!(dst, [0x11, 0x22]);
        assert_eq!(next, 2, "the third sample must remain for later IRQ work");

        let mut no_data = false;
        let drained = drain_samples(&mut [0_u8; 1], || {
            assert!(!no_data);
            no_data = true;
            RxSample {
                byte: None,
                line_error: true,
            }
        });
        assert_eq!(drained.bytes, 0);
        assert_eq!(drained.line_errors, 1);
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
