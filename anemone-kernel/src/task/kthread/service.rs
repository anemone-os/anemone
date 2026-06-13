use crate::prelude::*;

use super::{KThreadBuilder, KThreadContext, KThreadRef};

/// Mutable pending backend used by `KThreadService`.
///
/// This trait intentionally models pending state, not a fixed queue shape. A
/// backend may merge into one slot, pop from a FIFO, or keep extra state after
/// `take()`; the worker loop only observes `is_empty`, `submit`, `take`, and
/// `discard`.
pub trait KThreadPending: Send + 'static {
    type Request: Send + 'static;
    type Work: Send + 'static;

    fn is_empty(&self) -> bool;
    fn submit(&mut self, request: Self::Request);
    fn take(&mut self) -> Option<Self::Work>;
    fn discard(&mut self);
}

/// Request type that can be merged into an existing pending slot.
pub trait KThreadMergeRequest: Send + 'static {
    fn merge(&mut self, other: Self);
}

/// Single pending slot backend. New submissions merge into the existing slot.
#[derive(Debug)]
pub struct KThreadPendingSlot<R> {
    pending: Option<R>,
}

impl<R> KThreadPendingSlot<R> {
    pub fn new() -> Self {
        Self { pending: None }
    }
}

impl<R> KThreadPending for KThreadPendingSlot<R>
where
    R: KThreadMergeRequest,
{
    type Request = R;
    type Work = R;

    fn is_empty(&self) -> bool {
        self.pending.is_none()
    }

    fn submit(&mut self, request: Self::Request) {
        if let Some(pending) = &mut self.pending {
            pending.merge(request);
        } else {
            self.pending = Some(request);
        }
    }

    fn take(&mut self) -> Option<Self::Work> {
        self.pending.take()
    }

    fn discard(&mut self) {
        self.pending = None;
    }
}

/// FIFO pending backend. New submissions are appended, and workers pop one
/// request at a time.
#[derive(Debug)]
pub struct KThreadPendingQueue<R> {
    pending: VecDeque<R>,
}

impl<R> KThreadPendingQueue<R> {
    pub fn new() -> Self {
        Self {
            pending: VecDeque::new(),
        }
    }
}

impl<R> KThreadPending for KThreadPendingQueue<R>
where
    R: Send + 'static,
{
    type Request = R;
    type Work = R;

    fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    fn submit(&mut self, request: Self::Request) {
        self.pending.push_back(request);
    }

    fn take(&mut self) -> Option<Self::Work> {
        self.pending.pop_front()
    }

    fn discard(&mut self) {
        self.pending.clear();
    }
}

pub trait KThreadRequestHandler<W>: Send + Sync + 'static {
    fn handle(&self, ctx: &KThreadContext, work: W);
}

pub struct KThreadService<A, H>
where
    A: KThreadPending,
    H: KThreadRequestHandler<A::Work>,
{
    state: Arc<KThreadServiceState<A>>,
    workers: Vec<KThreadRef>,
    handler: Arc<H>,
    options: KThreadServiceOptions,
}

struct KThreadServiceState<A>
where
    A: KThreadPending,
{
    inner: SpinLock<KThreadServiceInner<A>>,
    drained: Event,
}

struct KThreadServiceInner<A>
where
    A: KThreadPending,
{
    stopping: bool,
    pending: A,
    active_workers: usize,
}

impl<A> KThreadServiceState<A>
where
    A: KThreadPending,
{
    fn new(pending: A) -> Self {
        Self {
            inner: SpinLock::new(KThreadServiceInner {
                stopping: false,
                pending,
                active_workers: 0,
            }),
            drained: Event::new(),
        }
    }

    fn submit(&self, request: A::Request) -> Result<(), SubmitError> {
        let mut inner = self.inner.lock();
        if inner.stopping {
            return Err(SubmitError::Stopping);
        }
        inner.pending.submit(request);
        Ok(())
    }

    fn should_wake(&self) -> bool {
        let inner = self.inner.lock();
        !inner.pending.is_empty()
    }

    fn take_work(&self) -> Option<A::Work> {
        let mut inner = self.inner.lock();
        let work = inner.pending.take();
        if work.is_some() {
            inner.active_workers += 1;
        }
        work
    }

    fn complete_work(&self) {
        let drained = {
            let mut inner = self.inner.lock();
            assert!(inner.active_workers > 0, "kthread service active underflow");
            inner.active_workers -= 1;
            inner.pending.is_empty() && inner.active_workers == 0
        };
        if drained {
            self.drained.publish(usize::MAX, true);
        }
    }

    fn begin_stop(&self, mode: StopMode) {
        let drained = {
            let mut inner = self.inner.lock();
            inner.stopping = true;
            if matches!(mode, StopMode::DiscardPending) {
                inner.pending.discard();
            }
            inner.pending.is_empty() && inner.active_workers == 0
        };
        if drained {
            self.drained.publish(usize::MAX, true);
        }
    }

    fn drain(&self) {
        self.drained.listen_uninterruptible(false, || {
            let inner = self.inner.lock();
            inner.pending.is_empty() && inner.active_workers == 0
        });
    }
}

struct KThreadServiceWorkerArg<A, H>
where
    A: KThreadPending,
    H: KThreadRequestHandler<A::Work>,
{
    state: Arc<KThreadServiceState<A>>,
    handler: Arc<H>,
}

impl<A, H> KThreadService<A, H>
where
    A: KThreadPending,
    H: KThreadRequestHandler<A::Work>,
{
    pub fn spawn(
        name: impl Into<Box<str>>,
        workers: usize,
        pending: A,
        handler: H,
        options: KThreadServiceOptions,
    ) -> Result<Self, SysError> {
        assert!(workers > 0, "kthread service must have at least one worker");

        let name = name.into();
        let state = Arc::new(KThreadServiceState::new(pending));
        let handler = Arc::new(handler);
        let mut worker_handles = Vec::new();

        for index in 0..workers {
            let mut builder = KThreadBuilder::new(format!("{}-{}", name, index));
            if let Some(cpu) = options.cpu {
                builder = builder.cpu(cpu);
            }

            let arg = KThreadServiceWorkerArg {
                state: state.clone(),
                handler: handler.clone(),
            };
            match builder.spawn(service_worker_entry::<A, H>, arg) {
                Ok(worker) => worker_handles.push(worker),
                Err(err) => {
                    state.begin_stop(StopMode::DiscardPending);
                    while let Some(worker) = worker_handles.pop() {
                        if let Some(worker) = worker.upgrade() {
                            worker.stop();
                        }
                    }
                    return Err(err);
                },
            }
        }

        Ok(Self {
            state,
            workers: worker_handles,
            handler,
            options,
        })
    }

    pub fn submit(&self, request: A::Request) -> Result<(), SubmitError> {
        self.state.submit(request)?;
        self.wake_workers();
        Ok(())
    }

    pub fn drain(&self) {
        self.state.drain();
    }

    pub fn stop(mut self) -> Vec<i32> {
        self.state.begin_stop(self.options.stop_mode);
        self.wake_workers();
        if matches!(self.options.stop_mode, StopMode::Drain) {
            self.state.drain();
        }

        let mut exit_codes = Vec::new();
        while let Some(worker) = self.workers.pop() {
            if let Some(worker) = worker.upgrade() {
                exit_codes.push(worker.stop());
            }
        }
        exit_codes
    }

    fn wake_workers(&self) {
        match self.options.wake_policy {
            WakePolicy::One => {
                if let Some(worker) = self.workers.first() {
                    if let Some(worker) = worker.upgrade() {
                        worker.wake();
                    }
                }
            },
            WakePolicy::All => {
                for worker in &self.workers {
                    if let Some(worker) = worker.upgrade() {
                        worker.wake();
                    }
                }
            },
        }
    }
}

impl<A, H> Drop for KThreadService<A, H>
where
    A: KThreadPending,
    H: KThreadRequestHandler<A::Work>,
{
    fn drop(&mut self) {
        if !self.workers.is_empty() {
            kwarningln!(
                "kthread service dropped without stop: workers={}",
                self.workers.len()
            );
        }
    }
}

fn service_worker_entry<A, H>(ctx: KThreadContext, arg: KThreadServiceWorkerArg<A, H>) -> i32
where
    A: KThreadPending,
    H: KThreadRequestHandler<A::Work>,
{
    loop {
        ctx.wait_until_woken(|| arg.state.should_wake());

        if ctx.should_stop() {
            break;
        }
        if ctx.should_park() {
            ctx.parkme();
            continue;
        }

        let Some(work) = arg.state.take_work() else {
            continue;
        };

        arg.handler.handle(&ctx, work);
        arg.state.complete_work();
    }

    0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KThreadServiceOptions {
    pub wake_policy: WakePolicy,
    pub stop_mode: StopMode,
    pub cpu: Option<CpuId>,
}

impl Default for KThreadServiceOptions {
    fn default() -> Self {
        Self {
            wake_policy: WakePolicy::One,
            stop_mode: StopMode::Drain,
            cpu: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakePolicy {
    One,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopMode {
    Drain,
    DiscardPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitError {
    Stopping,
}
