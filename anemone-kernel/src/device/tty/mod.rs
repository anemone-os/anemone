mod discipline;
mod endpoint;
mod file;
mod port;
mod pty;
mod relation;
mod terminal;

pub(crate) use endpoint::prepare_system_boot;
pub(crate) use port::{TtyLineSnapshot, TtyParity, TtyPort, TtyPortId, TtyRxUnit};
pub(crate) use pty::{
    LivePtyPair, PreparedPtyPair, PreparedPtySlaveDescription, PtyBindingCapability, PtyBindingOps,
    PtyImplicitAcquire, prepare_pair,
};
pub(crate) use relation::{detach_exiting_session, proc_snapshot};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TtyFlushQueues {
    Input,
    Output,
    Both,
}

impl TtyFlushQueues {
    fn includes_input(self) -> bool {
        matches!(self, Self::Input | Self::Both)
    }

    fn includes_output(self) -> bool {
        matches!(self, Self::Output | Self::Both)
    }

    fn reverse(self) -> Self {
        match self {
            Self::Input => Self::Output,
            Self::Output => Self::Input,
            Self::Both => Self::Both,
        }
    }
}

struct UnpublishedEndpoint {
    endpoint: Weak<TtyEndpoint>,
    /// One-shot relation commit authority. It is transferred to the boot
    /// publication transaction only after every other fallible prepare step.
    enrollment: Option<relation::RelationEnrollment>,
}

static UNPUBLISHED_PORTS: Lazy<SpinLock<BTreeMap<TtyPortId, UnpublishedEndpoint>>> =
    Lazy::new(|| SpinLock::new(BTreeMap::new()));

/// Stable semantic terminal capability shared by FileOps and relation owners.
///
/// The allocation identity of this object is the exact terminal identity used
/// by the relation owner. It deliberately contains no physical-port identity,
/// transport state, worker handle, or readiness cache. The weak backend edge
/// only exposes bounded wake and queue-flush capabilities owned by the serial
/// attachment or PTY pair.
struct TtyEndpoint {
    terminal: Arc<Terminal>,
    /// Weak projection only; attachment/opened-file lifecycle owns backend
    /// reachability without making the endpoint a second lifecycle owner.
    backend: Weak<dyn TtyBackend>,
}

/// Narrow endpoint backend capability.
///
/// It carries no byte, capacity, readiness, peer-presence, or lifecycle
/// snapshot. Queue mutation stays with Terminal/port owners, while this
/// capability supplies the endpoint-specific arbitration needed to reach them.
trait TtyBackend: Send + Sync {
    fn wake(&self);

    /// The caller has already admitted any endpoint-specific lifecycle
    /// operation. Serial implements worker/port arbitration here; PTY callers
    /// hold the pair operation guard through `TtyOperation`.
    fn flush_queues(&self, queues: TtyFlushQueues);
}

#[derive(Opaque)]
struct TtyWorker {
    backend: Arc<SerialTtyBackend>,
    endpoint: Arc<TtyEndpoint>,
}

struct SerialTransfer {
    /// Protocol state only: a new generation retires every worker-local RX
    /// unit captured before the corresponding input flush. It never describes
    /// raw-port or Terminal queue contents.
    input_flush_generation: usize,
}

struct SerialTtyBackend {
    /// Installed exactly once before any weak notifier becomes reachable and
    /// taken exactly once by pre-publication abort. This is lifecycle state,
    /// not work truth; RX/output/drain predicates remain authoritative.
    worker: SpinLock<Option<KThreadHandle>>,
    /// Serializes worker RX/TX transfer with TCFLSH. It is never held by IRQ
    /// publication and never substitutes for a Terminal or port queue guard.
    transfer: Mutex<SerialTransfer>,
    port: Arc<dyn TtyPort>,
    terminal: Arc<Terminal>,
}

#[derive(Clone)]
pub(super) struct TtyBackendHandle {
    backend: Arc<dyn TtyBackend>,
}

impl TtyBackendHandle {
    pub(super) fn wake(&self) {
        self.backend.wake();
    }

    pub(super) fn flush_queues(&self, queues: TtyFlushQueues) {
        self.backend.flush_queues(queues);
    }
}

impl SerialTtyBackend {
    fn wake_worker(&self) {
        let worker = self.worker.lock().as_ref().cloned();
        if let Some(worker) = worker {
            worker.wake();
        }
    }
}

fn retire_rx_batch_after_flush(
    current_generation: usize,
    observed_generation: &mut usize,
    cursor: &mut usize,
    len: &mut usize,
) {
    if *observed_generation == current_generation {
        return;
    }
    *cursor = 0;
    *len = 0;
    *observed_generation = current_generation;
}

impl TtyBackend for SerialTtyBackend {
    fn wake(&self) {
        self.wake_worker();
    }

    fn flush_queues(&self, queues: TtyFlushQueues) {
        {
            let mut transfer = self.transfer.lock();
            if queues.includes_input() {
                self.port.discard_rx();
            }
            self.terminal.flush_queues(queues);
            if queues.includes_input() {
                transfer.input_flush_generation = transfer
                    .input_flush_generation
                    .checked_add(1)
                    .expect("TTY input flush generation overflow");
            }
        }
        self.terminal.publish_progress();
        self.wake_worker();
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
    backend: Option<Arc<SerialTtyBackend>>,
}

impl TtyPortAttachment {
    pub(crate) fn abort(mut self) {
        self.detach();
    }

    fn detach(&mut self) {
        let Some(backend) = self.backend.take() else {
            return;
        };

        let removed = remove_unpublished_endpoint(self.port.id(), &self.endpoint);
        let worker = backend
            .worker
            .lock()
            .take()
            .expect("TTY backend lost its worker before detach");
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
    backend: Weak<SerialTtyBackend>,
}

impl TtyRxNotifier {
    pub(crate) fn wake(&self) {
        if let Some(backend) = self.backend.upgrade() {
            backend.wake_worker();
        }
    }
}

pub(crate) fn attach_unpublished_port(
    port: Arc<dyn TtyPort>,
    line_snapshot: TtyLineSnapshot,
) -> Result<(TtyPortAttachment, TtyRxNotifier), SysError> {
    let terminal = Terminal::try_new(line_snapshot)?;
    let backend = Arc::try_new(SerialTtyBackend {
        worker: SpinLock::new(None),
        transfer: Mutex::new(SerialTransfer {
            input_flush_generation: 0,
        }),
        port: port.clone(),
        terminal: terminal.clone(),
    })
    .map_err(|_| SysError::OutOfMemory)?;
    let endpoint = Arc::try_new(TtyEndpoint {
        terminal,
        backend: {
            let backend: Arc<dyn TtyBackend> = backend.clone();
            Arc::downgrade(&backend)
        },
    })
    .map_err(|_| SysError::OutOfMemory)?;
    let enrollment = relation::RelationEnrollment::new(endpoint.clone());
    let id = port.id().clone();

    {
        let mut ports = UNPUBLISHED_PORTS.lock();
        ports.retain(|_, pending| pending.endpoint.upgrade().is_some());
        if ports.contains_key(&id) {
            return Err(SysError::DevAlreadyRegistered);
        }
        let old = ports.insert(
            id,
            UnpublishedEndpoint {
                endpoint: Arc::downgrade(&endpoint),
                enrollment: Some(enrollment),
            },
        );
        assert!(
            old.is_none(),
            "duplicate TTY port passed registry validation"
        );
    }

    let worker = KThreadBuilder::new(format!("tty:{}", port.id())).spawn(
        tty_worker_entry,
        AnyOpaque::new(TtyWorker {
            backend: backend.clone(),
            endpoint: endpoint.clone(),
        }),
    );
    finish_unpublished_port_attach(port, endpoint, backend, worker)
}

fn finish_unpublished_port_attach(
    port: Arc<dyn TtyPort>,
    endpoint: Arc<TtyEndpoint>,
    backend: Arc<SerialTtyBackend>,
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

    let old = backend.worker.lock().replace(worker);
    assert!(old.is_none(), "TTY backend worker installed twice");
    let notifier = TtyRxNotifier {
        backend: Arc::downgrade(&backend),
    };
    Ok((
        TtyPortAttachment {
            port,
            endpoint,
            backend: Some(backend),
        },
        notifier,
    ))
}

fn remove_unpublished_endpoint(id: &TtyPortId, endpoint: &Arc<TtyEndpoint>) -> bool {
    let removed = {
        let mut ports = UNPUBLISHED_PORTS.lock();
        let Some(registered) = ports.get(id) else {
            return false;
        };
        if !registered
            .endpoint
            .upgrade()
            .is_some_and(|registered| Arc::ptr_eq(&registered, endpoint))
        {
            return false;
        }
        ports.remove(id)
    };
    // The pending enrollment may hold the last endpoint reference. Release it
    // only after registry visibility has been withdrawn.
    drop(removed);
    true
}

fn tty_worker_entry(ctx: KThreadCtx, arg: AnyOpaque) -> i32 {
    let worker = arg
        .cast::<TtyWorker>()
        .expect("TTY worker received invalid private data");
    let backend = &worker.backend;
    let port = &backend.port;
    let endpoint = &worker.endpoint;
    let mut rx_batch = [TtyRxUnit::Byte(0); TTY_WORKER_BATCH_BYTES];
    let mut rx_cursor = 0;
    let mut rx_len = 0;
    let mut observed_input_flush_generation = 0;
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

        {
            // TCFLSH owns the same transfer guard. An input-generation change
            // retires the worker-local batch captured before that flush, while
            // output flush cannot interleave between peek, port submit and
            // Terminal consume.
            let transfer = backend.transfer.lock();
            retire_rx_batch_after_flush(
                transfer.input_flush_generation,
                &mut observed_input_flush_generation,
                &mut rx_cursor,
                &mut rx_len,
            );

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
                        TtySignalControl::Suspend => {
                            crate::task::jobctl::TtyTerminalSignal::Suspend
                        },
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

    #[kunit]
    fn input_flush_generation_retires_only_pre_flush_worker_batch() {
        let mut observed_generation = 4;
        let mut cursor = 2;
        let mut len = 7;
        retire_rx_batch_after_flush(4, &mut observed_generation, &mut cursor, &mut len);
        assert_eq!((observed_generation, cursor, len), (4, 2, 7));

        retire_rx_batch_after_flush(5, &mut observed_generation, &mut cursor, &mut len);
        assert_eq!((observed_generation, cursor, len), (5, 0, 0));

        cursor = 0;
        len = 3;
        retire_rx_batch_after_flush(5, &mut observed_generation, &mut cursor, &mut len);
        assert_eq!((observed_generation, cursor, len), (5, 0, 3));
    }
}
