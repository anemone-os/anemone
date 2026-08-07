use crate::prelude::*;

use super::control::KThreadControl;

/// Narrow runtime capability passed to a kthread entry.
#[derive(Debug, Clone)]
pub struct KThreadCtx {
    pub(super) control: Arc<KThreadControl>,
}

impl KThreadCtx {
    pub(super) fn new(control: Arc<KThreadControl>) -> Self {
        Self { control }
    }

    pub fn should_stop(&self) -> bool {
        self.control.should_stop()
    }

    /// Wait for a pure wake notification until stop or the consumer predicate.
    pub fn wait_until<P>(&self, predicate: P)
    where
        P: Fn() -> bool,
    {
        self.control.wait_until(predicate);
    }

    /// Compatibility spelling for current consumers. This remains a pure
    /// wake-plus-predicate wait and does not encode request queue semantics.
    pub fn wait_until_woken<P>(&self, predicate: P)
    where
        P: Fn() -> bool,
    {
        self.wait_until(predicate);
    }

    /// Wait until the duration expires or cooperative stop is requested.
    ///
    /// Ordinary wake notifications only cause a stop/deadline recheck; they
    /// cannot complete this wait early.
    pub fn wait_for(&self, timeout: Duration) {
        self.control.wait_for(timeout);
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{task::kthread::KThreadBuilder, time::Instant, utils::any_opaque::AnyOpaque};

    #[derive(Opaque)]
    struct WaitForContext {
        timeout: Duration,
        ready: Arc<AtomicBool>,
        elapsed_nanos: Arc<AtomicU64>,
    }

    fn wait_for_entry(ctx: KThreadCtx, opaque: AnyOpaque) -> i32 {
        let context = opaque
            .cast::<WaitForContext>()
            .expect("invalid kthread wait_for context");
        let start = Instant::now();
        context.ready.store(true, Ordering::Release);
        ctx.wait_for(context.timeout);
        context
            .elapsed_nanos
            .store(start.elapsed().as_nanos() as u64, Ordering::Release);
        i32::from(ctx.should_stop())
    }

    #[kunit]
    fn wait_for_completes_on_timeout() {
        let timeout = Duration::from_millis(5);
        let ready = Arc::new(AtomicBool::new(false));
        let elapsed_nanos = Arc::new(AtomicU64::new(0));
        let worker = spawn_waiter(timeout, ready, elapsed_nanos.clone());

        assert_eq!(worker.wait_exited(), 0);
        assert!(
            elapsed_nanos.load(Ordering::Acquire) >= timeout.as_nanos() as u64,
            "kthread wait_for completed before its timeout"
        );
    }

    #[kunit]
    fn request_stop_interrupts_wait_for() {
        let ready = Arc::new(AtomicBool::new(false));
        let worker = spawn_waiter(
            Duration::from_secs(60),
            ready.clone(),
            Arc::new(AtomicU64::new(0)),
        );

        while !ready.load(Ordering::Acquire) {
            yield_now();
        }
        worker.request_stop();
        assert_eq!(worker.wait_exited(), 1);
    }

    fn spawn_waiter(
        timeout: Duration,
        ready: Arc<AtomicBool>,
        elapsed_nanos: Arc<AtomicU64>,
    ) -> super::super::KThreadHandle {
        let worker = KThreadBuilder::new("kunit:kthread-wait-for")
            .spawn(
                wait_for_entry,
                AnyOpaque::new(WaitForContext {
                    timeout,
                    ready,
                    elapsed_nanos,
                }),
            )
            .expect("failed to spawn kthread wait_for worker");
        worker
    }
}
