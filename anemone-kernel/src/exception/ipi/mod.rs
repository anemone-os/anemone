//! Inter-processor interrupt handling.
//!
//! Both synchronous and asynchronous IPIs are supported.
//!
//! Currently, dymanic allocation caused by allocating buffer for IPI messages
//! may incur heap oom followed by frame allocation.
//!
//! TODO: We should finaly implement another IPI mechanism that doesn't require
//! dynamic allocation.

use core::hint::spin_loop;

use alloc::{alloc::AllocError, collections::LinkedList};

use crate::prelude::*;

pub(crate) mod user_tlb;
use user_tlb::handle_user_tlb_shootdowns;

#[derive(Debug)]
pub enum IpiPayload {
    MemoryBarrier,
    /// `None` invalidates the entire local TLB on the receiving CPU.
    TlbShootdown {
        range: Option<VirtPageRange>,
    },
    EnqueueNewTask {
        tid: Tid,
    },
    WakeUpTaskStaleSafe {
        task: Arc<Task>,
        park: ParkState,
    },
    SchedulerRequest(Box<SchedRequest>),
    StopExecution,
}

impl IpiPayload {
    fn copy_for_broadcast(&self) -> Self {
        match self {
            Self::MemoryBarrier => Self::MemoryBarrier,
            Self::TlbShootdown { range } => Self::TlbShootdown { range: *range },
            Self::EnqueueNewTask { tid } => Self::EnqueueNewTask { tid: *tid },
            Self::WakeUpTaskStaleSafe { .. } => {
                panic!("wake placement cannot be copied for IPI broadcast")
            },
            Self::SchedulerRequest(_) => {
                panic!("scheduler request cannot be copied for IPI broadcast")
            },
            Self::StopExecution => Self::StopExecution,
        }
    }
}

#[derive(Debug)]
struct IpiMsg {
    payload: IpiPayload,
    is_accomplished: AtomicBool,
}

impl IpiMsg {
    fn new(payload: IpiPayload) -> IpiMsg {
        Self {
            payload,
            is_accomplished: AtomicBool::new(false),
        }
    }
}

/// This queue's lock will be acquired in hwirq context(ipi handler), so we must
/// use `lock_irqsave` all the time instead of `lock`, otherwise deadlock can
/// occur.
#[percpu]
static IPI_QUEUE: SpinLock<LinkedList<Arc<IpiMsg>>> = SpinLock::new(LinkedList::new());

#[inline(always)]
fn alloc_ipi_msg(payload: IpiPayload) -> Result<Arc<IpiMsg>, IpiError> {
    Arc::try_new(IpiMsg::new(payload)).map_err(IpiError::Alloc)
}

fn wait_ipi_accomplished(msg: &Arc<IpiMsg>) {
    loop {
        if msg.is_accomplished.load(Ordering::Acquire) {
            return;
        }
        spin_loop();
    }
}

#[inline(always)]
fn enqueue_ipi(cpu_id: CpuId, msg: Arc<IpiMsg>) {
    unsafe {
        IPI_QUEUE.with_remote(cpu_id, move |queue| {
            let mut queue = queue.lock_irqsave();
            queue.push_back(msg);
        })
    }
    IntrArch::send_ipi(cpu_id.physical_id());
}

/// Send an IPI to the target CPU, synchronously waiting for the IPI to be
/// handled before returning.
pub fn send_ipi(cpu_id: CpuId, payload: IpiPayload) -> Result<(), IpiError> {
    if cpu_id == cur_cpu_id() {
        panic!("cannot send ipi to self");
    }
    if !target_online(cpu_id) {
        return Err(IpiError::TargetOffline);
    }

    let msg = alloc_ipi_msg(payload)?;
    enqueue_ipi(cpu_id, Arc::clone(&msg));
    wait_ipi_accomplished(&msg);

    Ok(())
}

/// Broadcast an IPI to all other CPUs, synchronously waiting for all of them to
/// handle the IPI before returning.
pub fn broadcast_ipi(payload: IpiPayload) -> Result<(), IpiError> {
    assert!(
        !matches!(
            &payload,
            IpiPayload::SchedulerRequest(_) | IpiPayload::WakeUpTaskStaleSafe { .. }
        ),
        "single-target IPI payload cannot be broadcast"
    );
    let cur_cpuid = cur_cpu_id();
    let ncpus = ncpus();
    for logical_id in 0..ncpus {
        let id = CpuId::new(logical_id);
        if !target_online(id) {
            return Err(IpiError::TargetOffline);
        }
    }
    let mut pending = LinkedList::new();
    for logical_id in 0..ncpus {
        let id = CpuId::new(logical_id);
        if id != cur_cpuid {
            let msg = alloc_ipi_msg(payload.copy_for_broadcast())?;
            pending.push_back(Arc::clone(&msg));
            enqueue_ipi(id, msg);
        }
    }

    for msg in pending {
        wait_ipi_accomplished(&msg);
    }
    Ok(())
}

#[derive(Debug)]
pub enum IpiError {
    TargetOffline,
    Alloc(AllocError),
}

/// Send an IPI to the target CPU asynchronously.
pub fn send_ipi_async(cpu_id: CpuId, payload: IpiPayload) -> Result<(), IpiError> {
    if cpu_id == cur_cpu_id() {
        panic!("cannot send ipi to self");
    }

    if !target_online(cpu_id) {
        return Err(IpiError::TargetOffline);
    }

    enqueue_ipi(cpu_id, alloc_ipi_msg(payload)?);
    Ok(())
}

/// Broadcast an IPI to all other CPUs asynchronously.
pub fn broadcast_ipi_async(payload: IpiPayload) -> Result<(), IpiError> {
    assert!(
        !matches!(
            &payload,
            IpiPayload::SchedulerRequest(_) | IpiPayload::WakeUpTaskStaleSafe { .. }
        ),
        "single-target IPI payload cannot be broadcast"
    );
    let cur_cpuid = cur_cpu_id();
    let ncpus = ncpus();
    for logical_id in 0..ncpus {
        let id = CpuId::new(logical_id);
        if id != cur_cpuid && !target_online(id) {
            return Err(IpiError::TargetOffline);
        }
    }

    let mut pending = LinkedList::new();
    for logical_id in 0..ncpus {
        let id = CpuId::new(logical_id);
        if id != cur_cpuid {
            pending.push_back((id, alloc_ipi_msg(payload.copy_for_broadcast())?));
        }
    }

    for (cpu_id, msg) in pending {
        enqueue_ipi(cpu_id, msg);
    }
    Ok(())
}

/// IPI handler.
pub fn handle_ipi() {
    use IpiPayload::*;

    handle_user_tlb_shootdowns();
    IPI_QUEUE.with(|queue| {
        loop {
            // The queue lock protects transport ownership only. Business
            // handlers, including scheduler transactions, run after this
            // guard is unambiguously released.
            let msg = {
                let mut queue = queue.lock_irqsave();
                queue.pop_front()
            };
            let Some(msg) = msg else {
                break;
            };
            match &msg.payload {
                MemoryBarrier => {
                    // The completion store below only acknowledges transport;
                    // membarrier requires this explicit full data fence.
                    full_memory_barrier();
                    msg.is_accomplished.store(true, Ordering::Release);
                },
                TlbShootdown { range } => {
                    if let Some(range) = *range {
                        PagingArch::tlb_shootdown_range(range);
                    } else {
                        PagingArch::tlb_shootdown_all();
                    }
                    msg.is_accomplished.store(true, Ordering::Release);
                },
                EnqueueNewTask { tid } => {
                    let tid = *tid;
                    let task = get_task(&tid).expect("internal error: no such task to wake up");

                    // SAFETY: all accesses to local runqueue already disabled interrupts, so we are
                    // safe to do this in hwirq context.
                    local_enqueue_new_task(task);
                    msg.is_accomplished.store(true, Ordering::Release);
                },
                WakeUpTaskStaleSafe { task, park } => {
                    handle_remote_wake_placement(task.clone(), *park);
                    msg.is_accomplished.store(true, Ordering::Release);
                },
                SchedulerRequest(request) => {
                    request.execute();
                    msg.is_accomplished.store(true, Ordering::Release);
                },
                StopExecution => {
                    msg.is_accomplished.store(true, Ordering::Release);
                    loop {
                        core::hint::spin_loop();
                    }
                },
            }
        }
    })
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn test_broadcast_copy_reconstructs_only_eligible_payloads() {
        let tid = Tid::new(7);
        let range = VirtPageRange::new(VirtPageNum::new(11), 3);
        let copies = [
            IpiPayload::MemoryBarrier.copy_for_broadcast(),
            IpiPayload::TlbShootdown { range: Some(range) }.copy_for_broadcast(),
            IpiPayload::TlbShootdown { range: None }.copy_for_broadcast(),
            IpiPayload::EnqueueNewTask { tid }.copy_for_broadcast(),
            IpiPayload::StopExecution.copy_for_broadcast(),
        ];

        let mut copies = copies.into_iter();
        assert!(matches!(copies.next().unwrap(), IpiPayload::MemoryBarrier));
        assert!(matches!(
            copies.next().unwrap(),
            IpiPayload::TlbShootdown {
                range: Some(copied)
            } if copied == range
        ));
        assert!(matches!(
            copies.next().unwrap(),
            IpiPayload::TlbShootdown { range: None }
        ));
        assert!(matches!(
            copies.next().unwrap(),
            IpiPayload::EnqueueNewTask { tid: copied } if copied == tid
        ));
        assert!(matches!(copies.next().unwrap(), IpiPayload::StopExecution));
        assert!(copies.next().is_none());
    }
}

pub struct TlbShootdownGuard {
    /// `None` preserves the existing full-flush request.
    range: Option<VirtPageRange>,
}

impl TlbShootdownGuard {
    pub fn new(range: Option<VirtPageRange>) -> Self {
        Self { range }
    }
}

impl Drop for TlbShootdownGuard {
    fn drop(&mut self) {
        if let Err(e) = broadcast_ipi(IpiPayload::TlbShootdown { range: self.range }) {
            kwarningln!("failed to send TLB shootdown IPI in TlbShootdownGuard: {e:?}");
        }
    }
}
