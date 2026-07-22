mod port;

pub(crate) use port::{TtyPort, TtyPortId};

use crate::{
    prelude::*,
    task::kthread::{KThreadBuilder, KThreadCtx, KThreadHandle},
    utils::any_opaque::AnyOpaque,
};

const STAGE1_DRAIN_BATCH_BYTES: usize = 64;

static UNPUBLISHED_PORTS: Lazy<SpinLock<BTreeMap<TtyPortId, Weak<TtyRxEndpoint>>>> =
    Lazy::new(|| SpinLock::new(BTreeMap::new()));

struct TtyRxEndpoint {
    port: Arc<dyn TtyPort>,
    /// Stage 1 diagnostic only. This counter never decides work, wake, or
    /// lifecycle state and disappears when Stage 2 installs the discipline.
    discarded_rx_bytes: AtomicUsize,
    /// Stage 1 diagnostic only. This counter is test/review evidence and never
    /// participates in the worker predicate or ordering.
    discarded_rx_batches: AtomicUsize,
}

impl TtyRxEndpoint {
    fn new(port: Arc<dyn TtyPort>) -> Self {
        Self {
            port,
            discarded_rx_bytes: AtomicUsize::new(0),
            discarded_rx_batches: AtomicUsize::new(0),
        }
    }
}

#[derive(Opaque)]
struct TtyRxWorker {
    endpoint: Arc<TtyRxEndpoint>,
}

/// Owns one unpublished endpoint and its worker until a later publication gate.
///
/// Dropping the attachment is the pre-publication abort path. It first removes
/// registry visibility, then requests worker stop and joins without holding the
/// registry or a port-owned guard.
pub(crate) struct TtyPortAttachment {
    endpoint: Arc<TtyRxEndpoint>,
    worker: Option<KThreadHandle>,
}

impl TtyPortAttachment {
    pub(crate) fn abort(mut self) {
        self.detach();
    }

    fn detach(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };

        let removed = remove_unpublished_endpoint(&self.endpoint);
        worker.request_stop();
        let exit_code = worker.wait_exited();

        assert!(
            removed,
            "TTY unpublished attachment lost its registry entry"
        );
        assert_eq!(exit_code, 0, "TTY RX worker exited with an error");
    }
}

impl Drop for TtyPortAttachment {
    fn drop(&mut self) {
        self.detach();
    }
}

/// Pure wake capability for a port IRQ path.
///
/// It carries no byte, count, or request truth. `TtyPort::rx_pending()` remains
/// the durable work predicate.
#[derive(Clone)]
pub(crate) struct TtyRxNotifier {
    worker: KThreadHandle,
}

impl TtyRxNotifier {
    pub(crate) fn wake(&self) {
        self.worker.wake();
    }
}

pub(crate) fn attach_unpublished_port(
    port: Arc<dyn TtyPort>,
) -> Result<(TtyPortAttachment, TtyRxNotifier), SysError> {
    let endpoint = Arc::try_new(TtyRxEndpoint::new(port)).map_err(|_| SysError::OutOfMemory)?;
    let id = endpoint.port.id().clone();

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

    let worker = match KThreadBuilder::new(format!("tty-rx:{}", endpoint.port.id())).spawn(
        tty_rx_worker_entry,
        AnyOpaque::new(TtyRxWorker {
            endpoint: endpoint.clone(),
        }),
    ) {
        Ok(worker) => worker,
        Err(error) => {
            let removed = remove_unpublished_endpoint(&endpoint);
            assert!(removed, "failed TTY attach lost its registry reservation");
            return Err(error);
        },
    };

    let notifier = TtyRxNotifier {
        worker: worker.clone(),
    };
    Ok((
        TtyPortAttachment {
            endpoint,
            worker: Some(worker),
        },
        notifier,
    ))
}

fn remove_unpublished_endpoint(endpoint: &Arc<TtyRxEndpoint>) -> bool {
    let id = endpoint.port.id();
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

fn tty_rx_worker_entry(ctx: KThreadCtx, arg: AnyOpaque) -> i32 {
    let endpoint = &arg
        .cast::<TtyRxWorker>()
        .expect("TTY RX worker received invalid private data")
        .endpoint;
    let mut batch = [0_u8; STAGE1_DRAIN_BATCH_BYTES];

    loop {
        ctx.wait_until(|| endpoint.port.rx_pending());
        if ctx.should_stop() {
            break;
        }

        loop {
            let drained = endpoint.port.dequeue_rx(&mut batch);
            assert!(
                drained <= batch.len(),
                "TTY port returned more RX bytes than the supplied batch"
            );
            if drained == 0 {
                assert!(
                    !endpoint.port.rx_pending(),
                    "TTY port reported pending RX without dequeue progress"
                );
                break;
            }

            endpoint
                .discarded_rx_bytes
                .fetch_add(drained, Ordering::Relaxed);
            endpoint
                .discarded_rx_batches
                .fetch_add(1, Ordering::Relaxed);

            if ctx.should_stop() {
                break;
            }
            yield_now();
        }
    }

    0
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::utils::ring_buffer::RingBuffer;

    const FAKE_PORT_CAPACITY: usize = 512;

    struct FakePort {
        id: TtyPortId,
        input: SpinLock<RingBuffer<u8, FAKE_PORT_CAPACITY>>,
        observed: SpinLock<RingBuffer<u8, FAKE_PORT_CAPACITY>>,
        predicate_checks: AtomicUsize,
        activity: Event,
    }

    impl FakePort {
        fn new(id: &str) -> Arc<Self> {
            Arc::new(Self {
                id: TtyPortId::try_from(id).unwrap(),
                input: SpinLock::new(RingBuffer::new()),
                observed: SpinLock::new(RingBuffer::new()),
                predicate_checks: AtomicUsize::new(0),
                activity: Event::new(),
            })
        }

        fn enqueue(&self, bytes: &[u8]) {
            assert_eq!(self.input.lock().try_push_slice(bytes), bytes.len());
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

        fn observed_len(&self) -> usize {
            self.observed.lock().len()
        }

        fn assert_observed(&self, expected: &[u8]) {
            let observed = self.observed.lock();
            assert_eq!(observed.len(), expected.len());
            assert!(observed.iter().eq(expected.iter().copied()));
        }

        fn wait_for_predicate_check(&self) {
            for _ in 0..1024 {
                if self.predicate_checks.load(Ordering::Relaxed) != 0 {
                    return;
                }
                yield_now();
            }
            assert!(false, "TTY worker did not check the fake port predicate");
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

        fn dequeue_rx(&self, dst: &mut [u8]) -> usize {
            let count = self.input.lock().try_pop_slice(dst);

            assert_eq!(self.observed.lock().try_push_slice(&dst[..count]), count);
            self.activity.publish(usize::MAX, true);
            count
        }

        fn submit_tx(&self, src: &[u8]) -> usize {
            src.len()
        }
    }

    fn attach(port: &Arc<FakePort>) -> (TtyPortAttachment, TtyRxNotifier) {
        attach_unpublished_port(port.clone()).unwrap()
    }

    #[kunit]
    fn duplicate_identity_is_rejected_until_abort() {
        let first = FakePort::new("/kunit/tty/duplicate");
        let duplicate = FakePort::new("/kunit/tty/duplicate");
        let (attachment, _) = attach(&first);

        assert_eq!(
            attach_unpublished_port(duplicate.clone()).err(),
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
        port.enqueue(b"before-wait");
        notifier.wake();

        port.wait_for(|| port.observed_len() == b"before-wait".len());
        port.assert_observed(b"before-wait");
        attachment.abort();
    }

    #[kunit]
    fn notification_after_worker_wait_keeps_rx_progress() {
        let port = FakePort::new("/kunit/tty/wake-after-wait");
        let (attachment, notifier) = attach(&port);
        port.wait_for_predicate_check();

        port.enqueue(b"after-wait");
        notifier.wake();
        port.wait_for(|| port.observed_len() == b"after-wait".len());

        port.assert_observed(b"after-wait");
        attachment.abort();
    }

    #[kunit]
    fn concurrent_rx_is_drained_without_replay() {
        let port = FakePort::new("/kunit/tty/concurrent-drain");
        let (attachment, notifier) = attach(&port);
        let first = [b'a'; STAGE1_DRAIN_BATCH_BYTES];
        let second = [b'b'; STAGE1_DRAIN_BATCH_BYTES];

        port.enqueue(&first);
        notifier.wake();
        port.wait_for(|| port.observed_len() >= first.len());
        port.enqueue(&second);
        notifier.wake();
        port.wait_for(|| port.observed_len() == first.len() + second.len());

        let mut expected = first.to_vec();
        expected.extend_from_slice(&second);
        port.assert_observed(&expected);
        attachment.abort();
    }

    #[kunit]
    fn persistent_predicate_drains_fifo_batches() {
        let port = FakePort::new("/kunit/tty/persistent-predicate");
        let (attachment, notifier) = attach(&port);
        let input: Vec<u8> = (0..(STAGE1_DRAIN_BATCH_BYTES * 3 + 7))
            .map(|index| index as u8)
            .collect();

        port.enqueue(&input);
        notifier.wake();
        port.wait_for(|| port.observed_len() == input.len());

        port.assert_observed(&input);
        assert!(!port.rx_pending());
        attachment.abort();
    }

    #[kunit]
    fn prepublish_abort_removes_registry_and_joins_worker() {
        let port = FakePort::new("/kunit/tty/abort");
        let (attachment, notifier) = attach(&port);

        attachment.abort();
        assert!(notifier.worker.has_exited());

        let (replacement, _) = attach(&port);
        replacement.abort();
    }
}
