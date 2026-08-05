//! Boot-fixed per-CPU system workers for short, asynchronous kernel work.
//!
//! This is deliberately smaller than a workqueue framework: topology is
//! created once during boot, submission is infallible, and workers only drain
//! their owner CPU's FIFO queue. Owners that need blocking work, independent
//! lifecycle, or queue isolation continue to use an owner-local kthread.

use crate::{
    prelude::*,
    task::kthread::{KThreadBuilder, KThreadCtx, KThreadHandle},
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

struct WorkQueue {
    entries: VecDeque<Box<dyn FnOnce() + Send + 'static>>,
}

impl WorkQueue {
    const fn new() -> Self {
        Self {
            entries: VecDeque::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn push_back(&mut self, work: Box<dyn FnOnce() + Send + 'static>) {
        self.entries.push_back(work);
    }

    fn pop_front(&mut self) -> Option<Box<dyn FnOnce() + Send + 'static>> {
        self.entries.pop_front()
    }
}

struct SystemWorkerSlot {
    /// Protocol identity for selecting the matching per-CPU queue. It is fixed
    /// by boot activation and does not project scheduler placement state.
    cpu: CpuId,
    handle: KThreadHandle,
}

/// A submission capability for one boot-fixed system worker.
///
/// The capability contains only a fixed queue target and wake ability. Queue
/// state, kthread lifecycle, and scheduler placement remain owner-private.
#[derive(Debug, Clone)]
pub(crate) struct SystemWorker {
    cpu: CpuId,
    handle: KThreadHandle,
}

#[percpu]
static SYSTEM_WORK_QUEUE: NoIrqSpinLock<WorkQueue> = NoIrqSpinLock::new(WorkQueue::new());

#[percpu]
static SYSTEM_WORKER_SLOT: NoIrqSpinLock<Option<SystemWorkerSlot>> = NoIrqSpinLock::new(None);

static ACTIVATED: AtomicBool = AtomicBool::new(false);

/// Publish one system worker for every boot-online CPU and seal the topology.
///
/// BSP calls this after the all-CPU-online barrier and before the unordered
/// `Late` initcall window. There is no runtime create, destroy, resize, or
/// hotplug path in this phase.
pub fn activate_system_workers() {
    assert_eq!(
        cur_cpu_id(),
        bsp_cpu_id(),
        "kworker activation must run on BSP"
    );
    assert!(
        !ACTIVATED.load(Ordering::Acquire),
        "system worker topology activated more than once"
    );

    for cpu in 0..ncpus() {
        let cpu = CpuId::new(cpu);
        let handle = KThreadBuilder::new(format!("kworker/{}", cpu.logical_id()))
            .cpu(cpu)
            .spawn(system_worker_entry, NilOpaque::new())
            .unwrap_or_else(|err| panic!("failed to spawn system worker for {}: {:?}", cpu, err));
        publish_slot(cpu, handle);
    }

    assert!(
        ACTIVATED
            .compare_exchange(false, true, Ordering::Release, Ordering::Acquire)
            .is_ok(),
        "system worker topology activated more than once"
    );
}

/// Return the already-published worker bound to the current CPU.
pub(crate) fn local_system_worker() -> SystemWorker {
    assert!(
        ACTIVATED.load(Ordering::Acquire),
        "system worker requested before boot activation"
    );
    SYSTEM_WORKER_SLOT.with(|slot| {
        let slot = slot.lock();
        let slot = slot
            .as_ref()
            .expect("local system worker slot was not published");
        assert_eq!(
            slot.cpu,
            cur_cpu_id(),
            "system worker slot has wrong CPU identity"
        );
        SystemWorker {
            cpu: slot.cpu,
            handle: slot.handle.clone(),
        }
    })
}

impl SystemWorker {
    /// Transfer one anonymous closure into this worker's FIFO queue.
    pub(crate) fn submit<F>(&self, work: F)
    where
        F: FnOnce() + Send + 'static,
    {
        self.submit_boxed(Box::new(work));
    }

    pub(crate) fn submit_boxed(&self, work: Box<dyn FnOnce() + Send + 'static>) {
        assert!(
            ACTIVATED.load(Ordering::Acquire),
            "system worker submission before boot activation"
        );
        assert!(
            target_online(self.cpu),
            "system worker target CPU is offline"
        );

        if self.cpu == cur_cpu_id() {
            SYSTEM_WORK_QUEUE.with(|queue| queue.lock().push_back(work));
        } else {
            unsafe {
                SYSTEM_WORK_QUEUE.with_remote(self.cpu, |queue| queue.lock().push_back(work));
            }
        }
        // Queue ownership is transferred before this no-result wake. The
        // worker predicate recheck closes wake-before-wait and drain races.
        self.handle.wake();
    }
}

fn publish_slot(cpu: CpuId, handle: KThreadHandle) {
    if cpu == cur_cpu_id() {
        SYSTEM_WORKER_SLOT.with(|slot| {
            let mut slot = slot.lock();
            assert!(slot.is_none(), "system worker slot initialized twice");
            *slot = Some(SystemWorkerSlot { cpu, handle });
        });
        return;
    }

    unsafe {
        SYSTEM_WORKER_SLOT.with_remote(cpu, |slot| {
            let mut slot = slot.lock();
            assert!(slot.is_none(), "system worker slot initialized twice");
            *slot = Some(SystemWorkerSlot { cpu, handle });
        });
    }
}

fn system_worker_entry(ctx: KThreadCtx, _: AnyOpaque) -> i32 {
    let cpu = cur_cpu_id();
    loop {
        if ctx.should_stop() {
            break;
        }

        ctx.wait_until(work_queue_not_empty);

        if ctx.should_stop() {
            break;
        }

        drain_work_queue(&ctx, cpu);
    }

    0
}

fn work_queue_not_empty() -> bool {
    SYSTEM_WORK_QUEUE.with(|queue| !queue.lock().is_empty())
}

fn drain_work_queue(ctx: &KThreadCtx, cpu: CpuId) {
    loop {
        let Some(work) = SYSTEM_WORK_QUEUE.with(|queue| queue.lock().pop_front()) else {
            break;
        };

        assert_eq!(cpu, cur_cpu_id(), "system worker migrated off its boot CPU");
        (work)();

        // A callback may submit a new entry. It is a later FIFO entry and is
        // therefore observed on the next iteration, never as the current work.
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
    fn system_workers_cover_boot_online_topology() {
        assert!(ACTIVATED.load(Ordering::Acquire));
        for cpu in 0..ncpus() {
            let cpu = CpuId::new(cpu);
            assert!(target_online(cpu));
            if cpu == cur_cpu_id() {
                SYSTEM_WORKER_SLOT.with(|slot| assert_published_slot(cpu, slot));
            } else {
                unsafe {
                    SYSTEM_WORKER_SLOT.with_remote(cpu, |slot| assert_published_slot(cpu, slot));
                }
            }
        }
    }

    fn assert_published_slot(cpu: CpuId, slot: &NoIrqSpinLock<Option<SystemWorkerSlot>>) {
        let slot = slot.lock();
        let slot = slot.as_ref().expect("online CPU is missing system worker");
        assert_eq!(slot.cpu, cpu);
        assert!(!slot.handle.has_exited());
    }

    #[kunit]
    fn system_worker_preserves_fifo_and_executes_once() {
        let worker = local_system_worker();
        let order = Arc::new(SpinLock::new(Vec::new()));
        let completed = Arc::new(Event::new());

        for value in 0..3 {
            let order = order.clone();
            let completed = completed.clone();
            worker.submit(move || {
                order.lock().push(value);
                if value == 2 {
                    completed.publish(usize::MAX, true);
                }
            });
        }

        completed.listen_uninterruptible(false, || order.lock().len() == 3);
        assert_eq!(&*order.lock(), &[0, 1, 2]);
    }

    #[kunit]
    fn system_worker_callback_resubmission_is_a_new_fifo_entry() {
        let worker = local_system_worker();
        let order = Arc::new(SpinLock::new(Vec::new()));
        let completed = Arc::new(Event::new());
        let nested_worker = worker.clone();
        let nested_order = order.clone();
        let nested_completed = completed.clone();

        worker.submit(move || {
            nested_order.lock().push(0);
            nested_worker.submit(move || {
                nested_order.lock().push(1);
                nested_completed.publish(usize::MAX, true);
            });
        });

        completed.listen_uninterruptible(false, || order.lock().len() == 2);
        assert_eq!(&*order.lock(), &[0, 1]);
    }
}
