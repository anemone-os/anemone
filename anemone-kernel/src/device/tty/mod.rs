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
/// transport state, worker handle, or readiness cache. The weak wake edge only
/// lets an open operation request that its attachment recheck owner predicates.
struct TtyEndpoint {
    terminal: Arc<Terminal>,
    /// Weak projection only; the driver attachment remains the sole
    /// long-lived strong owner of the worker wake source.
    wake_source: Weak<dyn TtyProgress>,
}

/// Narrow backend progress capability. It carries no byte, capacity,
/// readiness, peer-presence, or lifecycle truth; every consumer rechecks the
/// corresponding owner after a notification.
trait TtyProgress: Send + Sync {
    fn wake(&self);
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
    source: Arc<dyn TtyProgress>,
}

impl TtyWakeHandle {
    pub(super) fn wake(&self) {
        self.source.wake();
    }
}

impl TtyProgress for TtyWakeSource {
    fn wake(&self) {
        let worker = self.worker.lock().as_ref().cloned();
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
        wake_source: {
            let progress: Arc<dyn TtyProgress> = wake_source.clone();
            Arc::downgrade(&progress)
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
