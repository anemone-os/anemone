use crate::{
    prelude::*,
    task::kthread::{KThreadBuilder, KThreadCtx, KThreadHandle},
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

use super::{TimerHandle, TimerLane, deadline_after, push_realtime_timer_event, push_timer_event};

const READY_BACKLOG_LOG_THRESHOLD: usize = 1024;

#[derive(Debug)]
struct WorkerSlot {
    /// Diagnostic/locality proof owned by timer core. It comes from the
    /// `KThreadBuilder::cpu()` creation request and is used only to assert that
    /// IRQ delivery wakes the local per-CPU timer worker.
    cpu: CpuId,
    handle: KThreadHandle,
}

/// Per-CPU handoff from timer dequeue to process-context completion.
///
/// The queue owns each callback after timer-core dispatch. Producers publish
/// under the no-IRQ lock, then wake the pinned worker; the worker removes one
/// callback under the lock and executes it only after unlocking.
struct ThreadedReadyQueue {
    queue: VecDeque<Box<dyn FnOnce() + Send + 'static>>,
}

impl ThreadedReadyQueue {
    const fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    fn push_back(&mut self, callback: Box<dyn FnOnce() + Send + 'static>) -> usize {
        self.queue.push_back(callback);
        self.queue.len()
    }

    fn pop_front(&mut self) -> Option<Box<dyn FnOnce() + Send + 'static>> {
        self.queue.pop_front()
    }
}

#[derive(Debug)]
struct ThreadedStats {
    // Diagnostic only: none of these counters decides queue ownership,
    // callback eligibility, or worker scheduling. `ready_high_water` drives
    // only the bounded-backlog warning below.
    submitted: AtomicUsize,
    dispatched: AtomicUsize,
    worker_wakes: AtomicUsize,
    worker_drains: AtomicUsize,
    callbacks_executed: AtomicUsize,
    ready_high_water: AtomicUsize,
    workers_spawned: AtomicUsize,
}

impl ThreadedStats {
    const fn new() -> Self {
        Self {
            submitted: AtomicUsize::new(0),
            dispatched: AtomicUsize::new(0),
            worker_wakes: AtomicUsize::new(0),
            worker_drains: AtomicUsize::new(0),
            callbacks_executed: AtomicUsize::new(0),
            ready_high_water: AtomicUsize::new(0),
            workers_spawned: AtomicUsize::new(0),
        }
    }
}

#[percpu]
static THREADED_READY_QUEUE: NoIrqSpinLock<ThreadedReadyQueue> =
    NoIrqSpinLock::new(ThreadedReadyQueue::new());

#[percpu]
static THREADED_WORKER: NoIrqSpinLock<Option<WorkerSlot>> = NoIrqSpinLock::new(None);

static THREADED_STATS: ThreadedStats = ThreadedStats::new();

/// Schedule a timer event whose completion runs on the local CPU's threaded
/// timer worker.
///
/// This is still a bounded timer completion lane, not a workqueue. The
/// callback may acquire ordinary locks in process context, but it must not
/// perform blocking I/O, long reclaim, user waits, or depend on timer-core
/// cancellation. Object owners must use their own generation / validness checks
/// for stale callbacks.
pub fn schedule_threaded_timer_event(
    expire: Duration,
    callback: Box<dyn FnOnce() + Send + 'static>,
) -> TimerHandle {
    assert!(
        threaded_worker_ready(),
        "threaded timer event scheduled before local worker initialization"
    );
    THREADED_STATS.submitted.fetch_add(1, Ordering::Relaxed);
    let deadline = deadline_after(expire);
    push_timer_event(deadline, TimerLane::Threaded(callback))
}

/// Schedule an absolute realtime request on the local CPU's timer queue.
///
/// `cancel_on_change_seq` is the timekeeper snapshot taken before registration.
/// When present, any later sequence consumes the request through
/// `clock_changed`; otherwise calendar steps only re-evaluate `deadline_ns`.
pub(crate) fn schedule_realtime_threaded_timer_event(
    deadline_ns: u64,
    cancel_on_change_seq: Option<u64>,
    expired: Box<dyn FnOnce() + Send + 'static>,
    clock_changed: Option<Box<dyn FnOnce() + Send + 'static>>,
) -> TimerHandle {
    assert!(
        threaded_worker_ready(),
        "realtime timer event scheduled before local worker initialization"
    );
    assert_eq!(
        cancel_on_change_seq.is_some(),
        clock_changed.is_some(),
        "cancel-on-set identity and callback must be installed together"
    );
    THREADED_STATS.submitted.fetch_add(1, Ordering::Relaxed);
    push_realtime_timer_event(
        deadline_ns,
        cancel_on_change_seq,
        TimerLane::RealtimeThreaded {
            expired,
            clock_changed,
        },
    )
}

fn threaded_worker_ready() -> bool {
    THREADED_WORKER.with(|slot| slot.lock().is_some())
}

pub(super) fn enqueue_expired_threaded_on(
    cpu: CpuId,
    callback: Box<dyn FnOnce() + Send + 'static>,
) {
    // Publish the callback before looking up and waking the worker. A worker
    // that observes the wake must therefore also be able to observe its work.
    let ready_len = if cpu == cur_cpu_id() {
        THREADED_READY_QUEUE.with(|queue| queue.lock().push_back(callback))
    } else {
        unsafe { THREADED_READY_QUEUE.with_remote(cpu, |queue| queue.lock().push_back(callback)) }
    };

    THREADED_STATS.dispatched.fetch_add(1, Ordering::Relaxed);
    update_ready_high_water(cpu, ready_len);

    let handle = if cpu == cur_cpu_id() {
        THREADED_WORKER.with(|slot| timer_worker_handle(cpu, slot))
    } else {
        unsafe { THREADED_WORKER.with_remote(cpu, |slot| timer_worker_handle(cpu, slot)) }
    };
    THREADED_STATS.worker_wakes.fetch_add(1, Ordering::Relaxed);
    handle.wake();
}

fn timer_worker_handle(cpu: CpuId, slot: &NoIrqSpinLock<Option<WorkerSlot>>) -> KThreadHandle {
    let slot = slot.lock();
    let slot = slot
        .as_ref()
        .expect("threaded timer event dispatched before worker initialization");
    assert_eq!(
        slot.cpu, cpu,
        "threaded timer worker slot does not belong to target CPU"
    );
    slot.handle.clone()
}

fn update_ready_high_water(cpu: CpuId, ready_len: usize) {
    let mut current = THREADED_STATS.ready_high_water.load(Ordering::Relaxed);
    while ready_len > current {
        match THREADED_STATS.ready_high_water.compare_exchange_weak(
            current,
            ready_len,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => {
                if ready_len >= READY_BACKLOG_LOG_THRESHOLD {
                    kwarningln!(
                        "threaded timer: ready backlog high-water {} on {}",
                        ready_len,
                        cpu
                    );
                }
                break;
            },
            Err(observed) => current = observed,
        }
    }
}

#[initcall(late)]
fn init_threaded_timer_workers() {
    for cpu in 0..ncpus() {
        let cpu_id = CpuId::new(cpu);
        let name = format!("timer-thread/{}", cpu);
        let handle = KThreadBuilder::new(name)
            .cpu(cpu_id)
            .spawn(threaded_timer_worker_entry, NilOpaque::new())
            .unwrap_or_else(|err| {
                panic!(
                    "failed to spawn threaded timer worker for {}: {:?}",
                    cpu_id, err
                )
            });

        publish_worker_slot(cpu_id, handle);
        THREADED_STATS
            .workers_spawned
            .fetch_add(1, Ordering::Relaxed);
    }
}

fn publish_worker_slot(cpu: CpuId, handle: KThreadHandle) {
    if cpu == cur_cpu_id() {
        THREADED_WORKER.with(|slot| {
            let mut slot = slot.lock();
            assert!(slot.is_none(), "threaded timer worker initialized twice");
            *slot = Some(WorkerSlot { cpu, handle });
        });
        return;
    }

    unsafe {
        THREADED_WORKER.with_remote(cpu, |slot| {
            let mut slot = slot.lock();
            assert!(slot.is_none(), "threaded timer worker initialized twice");
            *slot = Some(WorkerSlot { cpu, handle });
        });
    }
}

fn threaded_timer_worker_entry(ctx: KThreadCtx, _: AnyOpaque) -> i32 {
    loop {
        if ctx.should_stop() {
            break;
        }

        ctx.wait_until(ready_queue_not_empty);

        if ctx.should_stop() {
            break;
        }

        drain_ready_queue(&ctx);
    }

    0
}

fn ready_queue_not_empty() -> bool {
    THREADED_READY_QUEUE.with(|queue| !queue.lock().is_empty())
}

fn drain_ready_queue(ctx: &KThreadCtx) {
    loop {
        // Transfer ownership out of the no-IRQ queue before invoking arbitrary
        // owner code. Timerfd/itimer callbacks may acquire their own locks.
        let Some(callback) = THREADED_READY_QUEUE.with(|queue| queue.lock().pop_front()) else {
            break;
        };

        THREADED_STATS.worker_drains.fetch_add(1, Ordering::Relaxed);
        (callback)();
        THREADED_STATS
            .callbacks_executed
            .fetch_add(1, Ordering::Relaxed);
        // Timer callbacks are bounded, but a callback burst can still keep this
        // kthread on-CPU indefinitely when kernel preemption is disabled.
        if ctx.should_stop() {
            break;
        }
        yield_now();
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn threaded_timer_callback_runs_outside_hwirq() {
        let completed = Arc::new(Event::new());
        let done = Arc::new(AtomicBool::new(false));
        let callback_completed = completed.clone();
        let callback_done = done.clone();

        let _request = schedule_threaded_timer_event(
            Duration::from_millis(1),
            Box::new(move || {
                assert!(
                    !crate::percpu::in_hwirq(),
                    "threaded timer callback ran in hwirq context"
                );
                assert!(
                    IntrArch::local_intr_enabled(),
                    "threaded timer callback ran with interrupts disabled"
                );
                assert!(
                    allow_preempt(),
                    "threaded timer callback ran with preemption disabled"
                );
                callback_done.store(true, Ordering::Release);
                callback_completed.publish(usize::MAX, true);
            }),
        );

        let timeout = completed.listen_with_timeout(
            false,
            || done.load(Ordering::Acquire),
            Duration::from_secs(1),
        );
        assert!(
            !matches!(timeout, Some(TimeoutListenException::Timeout)),
            "threaded timer callback did not complete before timeout"
        );
    }
}
