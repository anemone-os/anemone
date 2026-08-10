mod discipline;
mod endpoint;
mod file;
mod port;
mod relation;
mod terminal;

pub(crate) use endpoint::prepare_system_boot;
pub(crate) use port::{TtyLineSnapshot, TtyParity, TtyPort, TtyPortId, TtyRxUnit};
pub(crate) use relation::detach_exiting_session;

use crate::{
    prelude::*,
    task::kthread::{KThreadBuilder, KThreadCtx, KThreadHandle},
    utils::any_opaque::AnyOpaque,
};

use discipline::TtySignalControl;
use terminal::Terminal;

static_assert!(
    TTY_CANONICAL_LINE_CAPACITY_BYTES > 0,
    "TTY canonical line capacity must be non-zero"
);
static_assert!(
    TTY_INPUT_CAPACITY_BYTES > 0,
    "TTY committed input capacity must be non-zero"
);
static_assert!(
    TTY_OUTPUT_CAPACITY_BYTES >= 8,
    "TTY output capacity must hold one maximum TAB3 transform token"
);
static_assert!(
    TTY_WORKER_BATCH_BYTES > 0,
    "TTY worker batch must be non-zero"
);

static UNPUBLISHED_PORTS: Lazy<SpinLock<BTreeMap<TtyPortId, Weak<TtyEndpoint>>>> =
    Lazy::new(|| SpinLock::new(BTreeMap::new()));

/// Stable semantic terminal capability shared by FileOps and relation owners.
///
/// The allocation identity of this object is the exact terminal identity used
/// by the relation owner. It deliberately contains no physical-port identity,
/// transport state, worker handle, or readiness cache. The weak wake edge only
/// lets an open operation request that its attachment recheck owner predicates.
struct TtyEndpoint {
    terminal: Arc<Terminal>,
    /// Weak projection only; the driver attachment remains the sole
    /// long-lived strong owner of the worker wake source.
    wake_source: Weak<TtyWakeSource>,
}

#[derive(Opaque)]
struct TtyWorker {
    port: Arc<dyn TtyPort>,
    endpoint: Arc<TtyEndpoint>,
}

struct TtyWakeSource {
    /// Installed exactly once before any weak notifier becomes reachable and
    /// taken exactly once by pre-publication abort. This is lifecycle state,
    /// not work truth; RX/output/drain predicates remain authoritative.
    worker: SpinLock<Option<KThreadHandle>>,
}

#[derive(Clone)]
pub(super) struct TtyWakeHandle {
    source: Arc<TtyWakeSource>,
}

impl TtyWakeHandle {
    pub(super) fn wake(&self) {
        let worker = self.source.worker.lock().as_ref().cloned();
        if let Some(worker) = worker {
            worker.wake();
        }
    }
}

/// Owns one unpublished endpoint and its worker until the publication stage.
///
/// Dropping the attachment is the pre-publication abort path. It first removes
/// registry visibility, then requests worker stop and joins without holding the
/// registry, Terminal, or port-owned guard.
pub(crate) struct TtyPortAttachment {
    port: Arc<dyn TtyPort>,
    endpoint: Arc<TtyEndpoint>,
    wake_source: Option<Arc<TtyWakeSource>>,
}

impl TtyPortAttachment {
    pub(crate) fn abort(mut self) {
        self.detach();
    }

    fn detach(&mut self) {
        let Some(wake_source) = self.wake_source.take() else {
            return;
        };

        let removed = remove_unpublished_endpoint(self.port.id(), &self.endpoint);
        let worker = wake_source
            .worker
            .lock()
            .take()
            .expect("TTY wake source lost its worker before detach");
        worker.request_stop();
        let exit_code = worker.wait_exited();

        assert!(
            removed,
            "TTY unpublished attachment lost its registry entry"
        );
        assert_eq!(exit_code, 0, "TTY endpoint worker exited with an error");
    }
}

impl Drop for TtyPortAttachment {
    fn drop(&mut self) {
        self.detach();
    }
}

/// Weak, pure wake projection for a port IRQ path.
///
/// It carries no byte, count, or request truth. Raw RX, Terminal output, and
/// drain predicates remain the durable work sources. The weak projection also
/// cannot keep an aborted unpublished worker alive.
#[derive(Clone)]
pub(crate) struct TtyRxNotifier {
    wake_source: Weak<TtyWakeSource>,
}

impl TtyRxNotifier {
    pub(crate) fn wake(&self) {
        if let Some(wake_source) = self.wake_source.upgrade() {
            let worker = wake_source.worker.lock().as_ref().cloned();
            if let Some(worker) = worker {
                worker.wake();
            }
        }
    }

    #[cfg(feature = "kunit")]
    fn is_live(&self) -> bool {
        self.wake_source.strong_count() != 0
    }
}

pub(crate) fn attach_unpublished_port(
    port: Arc<dyn TtyPort>,
    line_snapshot: TtyLineSnapshot,
) -> Result<(TtyPortAttachment, TtyRxNotifier), SysError> {
    let terminal = Terminal::try_new(line_snapshot)?;
    let wake_source = Arc::try_new(TtyWakeSource {
        worker: SpinLock::new(None),
    })
    .map_err(|_| SysError::OutOfMemory)?;
    let endpoint = Arc::try_new(TtyEndpoint {
        terminal,
        wake_source: Arc::downgrade(&wake_source),
    })
    .map_err(|_| SysError::OutOfMemory)?;
    let id = port.id().clone();

    {
        let mut ports = UNPUBLISHED_PORTS.lock();
        ports.retain(|_, endpoint| endpoint.upgrade().is_some());
        if ports.contains_key(&id) {
            return Err(SysError::DevAlreadyRegistered);
        }
        let old = ports.insert(id, Arc::downgrade(&endpoint));
        assert!(
            old.is_none(),
            "duplicate TTY port passed registry validation"
        );
    }

    let worker = KThreadBuilder::new(format!("tty:{}", port.id())).spawn(
        tty_worker_entry,
        AnyOpaque::new(TtyWorker {
            port: port.clone(),
            endpoint: endpoint.clone(),
        }),
    );
    finish_unpublished_port_attach(port, endpoint, wake_source, worker)
}

fn finish_unpublished_port_attach(
    port: Arc<dyn TtyPort>,
    endpoint: Arc<TtyEndpoint>,
    wake_source: Arc<TtyWakeSource>,
    worker: Result<KThreadHandle, SysError>,
) -> Result<(TtyPortAttachment, TtyRxNotifier), SysError> {
    let worker = match worker {
        Ok(worker) => worker,
        Err(error) => {
            let removed = remove_unpublished_endpoint(port.id(), &endpoint);
            assert!(removed, "failed TTY attach lost its registry reservation");
            return Err(error);
        },
    };

    let old = wake_source.worker.lock().replace(worker);
    assert!(old.is_none(), "TTY wake source worker installed twice");
    let notifier = TtyRxNotifier {
        wake_source: Arc::downgrade(&wake_source),
    };
    Ok((
        TtyPortAttachment {
            port,
            endpoint,
            wake_source: Some(wake_source),
        },
        notifier,
    ))
}

fn remove_unpublished_endpoint(id: &TtyPortId, endpoint: &Arc<TtyEndpoint>) -> bool {
    let mut ports = UNPUBLISHED_PORTS.lock();
    let Some(registered) = ports.get(id) else {
        return false;
    };
    if !registered
        .upgrade()
        .is_some_and(|registered| Arc::ptr_eq(&registered, endpoint))
    {
        return false;
    }
    ports.remove(id);
    true
}

fn tty_worker_entry(ctx: KThreadCtx, arg: AnyOpaque) -> i32 {
    let worker = arg
        .cast::<TtyWorker>()
        .expect("TTY worker received invalid private data");
    let port = &worker.port;
    let endpoint = &worker.endpoint;
    let mut rx_batch = [TtyRxUnit::Byte(0); TTY_WORKER_BATCH_BYTES];
    let mut rx_cursor = 0;
    let mut rx_len = 0;
    let mut tx_batch = [0_u8; TTY_WORKER_BATCH_BYTES];

    loop {
        ctx.wait_until(|| {
            rx_cursor != rx_len
                || port.rx_pending()
                || endpoint.terminal.output_pending()
                || endpoint.terminal.drain_check_pending()
        });
        if ctx.should_stop() {
            break;
        }

        if rx_cursor == rx_len && port.rx_pending() {
            rx_len = port.dequeue_rx(&mut rx_batch);
            rx_cursor = 0;
            assert!(
                rx_len <= rx_batch.len(),
                "TTY port returned more RX units than the supplied batch"
            );
            if rx_len == 0 {
                assert!(
                    !port.rx_pending(),
                    "TTY port reported pending RX without dequeue progress"
                );
            }
        }

        while rx_cursor < rx_len {
            let effect = endpoint
                .terminal
                .receive_rx_unit_effect(rx_batch[rx_cursor]);
            if !effect.consumed() {
                break;
            }
            rx_cursor += 1;
            if let Some(signal) = effect.signal() {
                let signal = match signal {
                    TtySignalControl::Interrupt => {
                        crate::task::jobctl::TtyTerminalSignal::Interrupt
                    },
                    TtySignalControl::Quit => crate::task::jobctl::TtyTerminalSignal::Quit,
                    TtySignalControl::Suspend => crate::task::jobctl::TtyTerminalSignal::Suspend,
                };
                if !relation::signal_foreground(endpoint, signal) {
                    endpoint.terminal.record_no_foreground_input_signal();
                }
            }
        }

        let prepared = endpoint.terminal.peek_output(&mut tx_batch);
        if prepared != 0 {
            let accepted = port.submit_tx(&tx_batch[..prepared]);
            assert!(
                accepted <= prepared,
                "TTY port accepted more TX bytes than supplied"
            );
            if accepted != 0 {
                endpoint.terminal.consume_output(&tx_batch[..accepted]);
            }
            if accepted != prepared {
                endpoint.terminal.record_partial_port_progress();
            }
        }

        if endpoint.terminal.drain_check_pending() {
            let port_idle = !endpoint.terminal.output_pending() && port.tx_idle();
            endpoint.terminal.complete_drain_if(port_idle);
        }

        if ctx.should_stop() {
            break;
        }
        // Work is bounded by the configured RX/TX batch. If any predicate is
        // still true, yield before the next round instead of monopolizing the
        // CPU; otherwise the next loop registers and rechecks the wake event.
        yield_now();
    }

    0
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::utils::ring_buffer::RingBuffer;

    const FAKE_PORT_CAPACITY: usize = 2048;

    struct FakePort {
        id: TtyPortId,
        input: SpinLock<RingBuffer<TtyRxUnit, FAKE_PORT_CAPACITY>>,
        dequeued: SpinLock<RingBuffer<TtyRxUnit, FAKE_PORT_CAPACITY>>,
        output: SpinLock<RingBuffer<u8, FAKE_PORT_CAPACITY>>,
        tx_limit: AtomicUsize,
        tx_idle: AtomicBool,
        predicate_checks: AtomicUsize,
        activity: Event,
    }

    impl FakePort {
        fn new(id: &str) -> Arc<Self> {
            Arc::new(Self {
                id: TtyPortId::try_from(id).unwrap(),
                input: SpinLock::new(RingBuffer::new()),
                dequeued: SpinLock::new(RingBuffer::new()),
                output: SpinLock::new(RingBuffer::new()),
                tx_limit: AtomicUsize::new(usize::MAX),
                tx_idle: AtomicBool::new(true),
                predicate_checks: AtomicUsize::new(0),
                activity: Event::new(),
            })
        }

        fn enqueue(&self, bytes: &[u8]) {
            let mut input = self.input.lock();
            for &byte in bytes {
                assert_eq!(input.try_push(TtyRxUnit::Byte(byte)), Ok(()));
            }
            drop(input);
            self.activity.publish(usize::MAX, true);
        }

        fn wait_for<P>(&self, predicate: P)
        where
            P: Fn() -> bool,
        {
            let result =
                self.activity
                    .listen_with_timeout(false, predicate, Duration::from_secs(1));
            assert!(
                !matches!(result, Some(TimeoutListenException::Timeout)),
                "fake TTY port did not reach the expected state"
            );
            assert!(
                !matches!(result, Some(TimeoutListenException::Signaled)),
                "fake TTY port wait was interrupted"
            );
        }

        fn wait_for_predicate_check(&self) {
            self.wait_for(|| self.predicate_checks.load(Ordering::Relaxed) != 0);
        }

        fn assert_dequeued(&self, expected: &[u8]) {
            let dequeued = self.dequeued.lock();
            assert_eq!(dequeued.len(), expected.len());
            assert!(
                dequeued
                    .iter()
                    .eq(expected.iter().copied().map(TtyRxUnit::Byte))
            );
        }

        fn output_len(&self) -> usize {
            self.output.lock().len()
        }

        fn assert_output(&self, expected: &[u8]) {
            let output = self.output.lock();
            assert_eq!(output.len(), expected.len());
            assert!(output.iter().eq(expected.iter().copied()));
        }
    }

    impl TtyPort for FakePort {
        fn id(&self) -> &TtyPortId {
            &self.id
        }

        fn rx_pending(&self) -> bool {
            self.predicate_checks.fetch_add(1, Ordering::Relaxed);
            !self.input.lock().is_empty()
        }

        fn dequeue_rx(&self, dst: &mut [TtyRxUnit]) -> usize {
            let count = self.input.lock().try_pop_slice(dst);
            assert_eq!(self.dequeued.lock().try_push_slice(&dst[..count]), count);
            self.activity.publish(usize::MAX, true);
            count
        }

        fn submit_tx(&self, src: &[u8]) -> usize {
            let accepted = src
                .len()
                .min(self.tx_limit.load(Ordering::Relaxed))
                .min(self.output.lock().available());
            assert_eq!(
                self.output.lock().try_push_slice(&src[..accepted]),
                accepted
            );
            self.activity.publish(usize::MAX, true);
            accepted
        }

        fn tx_idle(&self) -> bool {
            let idle = self.tx_idle.load(Ordering::Relaxed);
            // Test-only observation edge: production drain truth remains the
            // Terminal queue plus the port snapshot, never this Event.
            self.activity.publish(usize::MAX, true);
            idle
        }
    }

    fn attach(port: &Arc<FakePort>) -> (TtyPortAttachment, TtyRxNotifier) {
        attach_unpublished_port(port.clone(), line()).unwrap()
    }

    fn line() -> TtyLineSnapshot {
        TtyLineSnapshot {
            baud: 115200,
            parity: TtyParity::None,
            data_bits: 8,
        }
    }

    #[kunit]
    fn duplicate_identity_is_rejected_until_abort() {
        let first = FakePort::new("/kunit/tty/duplicate");
        let duplicate = FakePort::new("/kunit/tty/duplicate");
        let (attachment, _) = attach(&first);

        assert_eq!(
            attach_unpublished_port(duplicate.clone(), line()).err(),
            Some(SysError::DevAlreadyRegistered)
        );
        attachment.abort();

        let (replacement, _) = attach(&duplicate);
        replacement.abort();
    }

    #[kunit]
    fn notification_before_worker_wait_keeps_rx_progress() {
        let port = FakePort::new("/kunit/tty/wake-before-wait");
        let (attachment, notifier) = attach(&port);
        let terminal = attachment.endpoint.terminal.clone();
        port.enqueue(b"before-wait\n");
        notifier.wake();

        port.wait_for(|| terminal.readable());
        port.assert_dequeued(b"before-wait\n");
        attachment.abort();
    }

    #[kunit]
    fn notification_after_worker_wait_keeps_rx_progress() {
        let port = FakePort::new("/kunit/tty/wake-after-wait");
        let (attachment, notifier) = attach(&port);
        let terminal = attachment.endpoint.terminal.clone();
        port.wait_for_predicate_check();

        port.enqueue(b"after-wait\n");
        notifier.wake();
        port.wait_for(|| terminal.readable());

        port.assert_dequeued(b"after-wait\n");
        attachment.abort();
    }

    #[kunit]
    fn persistent_raw_predicate_transfers_fifo_batches_once() {
        let port = FakePort::new("/kunit/tty/persistent-predicate");
        let (attachment, notifier) = attach(&port);
        let terminal = attachment.endpoint.terminal.clone();
        let mut input = vec![b'a'; TTY_WORKER_BATCH_BYTES * 3 + 7];
        input.push(b'\n');

        port.enqueue(&input);
        notifier.wake();
        port.wait_for(|| terminal.readable());

        port.assert_dequeued(&input);
        let mut observed = vec![0_u8; input.len()];
        assert_eq!(
            terminal.read_input(&mut observed),
            discipline::InputRead::Bytes(input.len())
        );
        assert_eq!(observed, input);
        attachment.abort();
    }

    #[kunit]
    fn worker_retries_partial_tx_without_replaying_input() {
        let port = FakePort::new("/kunit/tty/partial-tx");
        port.tx_limit.store(1, Ordering::Relaxed);
        let (attachment, notifier) = attach(&port);
        let terminal = attachment.endpoint.terminal.clone();

        port.enqueue(b"x\n");
        notifier.wake();
        port.wait_for(|| port.output_len() == 3 && terminal.readable());

        port.assert_dequeued(b"x\n");
        port.assert_output(b"x\r\n");
        attachment.abort();
    }

    #[kunit]
    fn prepublish_abort_drops_weak_notifier_and_joins_worker() {
        let port = FakePort::new("/kunit/tty/abort");
        let (attachment, notifier) = attach(&port);
        let endpoint = Arc::downgrade(&attachment.endpoint);
        assert!(notifier.is_live());

        attachment.abort();
        assert!(!notifier.is_live());
        assert!(endpoint.upgrade().is_none());
        notifier.wake();

        let (replacement, _) = attach(&port);
        replacement.abort();
    }

    #[kunit]
    fn worker_spawn_failure_releases_identity_reservation() {
        let port = FakePort::new("/kunit/tty/spawn-failure");
        let terminal = Terminal::try_new(line()).unwrap();
        let wake_source = Arc::new(TtyWakeSource {
            worker: SpinLock::new(None),
        });
        let endpoint = Arc::new(TtyEndpoint {
            terminal,
            wake_source: Arc::downgrade(&wake_source),
        });
        let old = UNPUBLISHED_PORTS
            .lock()
            .insert(port.id().clone(), Arc::downgrade(&endpoint));
        assert!(old.is_none());

        let result = finish_unpublished_port_attach(
            port.clone(),
            endpoint,
            wake_source,
            Err(SysError::OutOfMemory),
        );
        assert_eq!(result.err(), Some(SysError::OutOfMemory));

        let (replacement, _) = attach(&port);
        replacement.abort();
    }
}
