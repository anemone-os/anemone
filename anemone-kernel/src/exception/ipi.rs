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

#[derive(Debug)]
pub enum IpiPayload {
    TlbShootdown {
        vpn: Option<VirtPageNum>,
    },
    EnqueueNewTask {
        tid: Tid,
    },
    WakeUpTaskStaleSafe {
        tid: Tid,
        park: ParkState,
    },
    SchedulerRequest(Box<SchedRequest>),
    #[cfg(feature = "kunit")]
    RunKUnitPerCpu {
        test_fn: fn(),
    },
    StopExecution,
}

impl IpiPayload {
    fn copy_for_broadcast(&self) -> Self {
        match self {
            Self::TlbShootdown { vpn } => Self::TlbShootdown { vpn: *vpn },
            Self::EnqueueNewTask { tid } => Self::EnqueueNewTask { tid: *tid },
            Self::WakeUpTaskStaleSafe { tid, park } => Self::WakeUpTaskStaleSafe {
                tid: *tid,
                park: *park,
            },
            Self::SchedulerRequest(_) => {
                panic!("scheduler request cannot be copied for IPI broadcast")
            },
            #[cfg(feature = "kunit")]
            Self::RunKUnitPerCpu { test_fn } => Self::RunKUnitPerCpu { test_fn: *test_fn },
            Self::StopExecution => Self::StopExecution,
        }
    }
}

#[derive(Debug)]
struct IpiMsg {
    payload: IpiPayload,
    is_accomplished: AtomicBool,
    wake_result: NoIrqSpinLock<Option<WakeEnqueueResult>>,
}

impl IpiMsg {
    fn new(payload: IpiPayload) -> IpiMsg {
        Self {
            payload,
            is_accomplished: AtomicBool::new(false),
            wake_result: NoIrqSpinLock::new(None),
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

fn wait_ipi_accomplished(msg: &Arc<IpiMsg>) -> Option<WakeEnqueueResult> {
    loop {
        if msg.is_accomplished.load(Ordering::Acquire) {
            return *msg.wake_result.lock();
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
    let _ = wait_ipi_accomplished(&msg);

    Ok(())
}

pub fn send_ipi_wait_result(
    cpu_id: CpuId,
    payload: IpiPayload,
) -> Result<WakeEnqueueResult, IpiError> {
    if cpu_id == cur_cpu_id() {
        panic!("cannot send ipi to self");
    }
    if !target_online(cpu_id) {
        return Err(IpiError::TargetOffline);
    }

    let msg = alloc_ipi_msg(payload)?;
    enqueue_ipi(cpu_id, Arc::clone(&msg));
    Ok(wait_ipi_accomplished(&msg).unwrap_or(WakeEnqueueResult::Stale))
}

/// Broadcast an IPI to all other CPUs, synchronously waiting for all of them to
/// handle the IPI before returning.
pub fn broadcast_ipi(payload: IpiPayload) -> Result<(), IpiError> {
    assert!(
        !matches!(&payload, IpiPayload::SchedulerRequest(_)),
        "scheduler request cannot be broadcast"
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
        let _ = wait_ipi_accomplished(&msg);
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
        !matches!(&payload, IpiPayload::SchedulerRequest(_)),
        "scheduler request cannot be broadcast"
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
                TlbShootdown { vpn } => {
                    if let Some(vpn) = *vpn {
                        PagingArch::tlb_shootdown(vpn);
                    } else {
                        PagingArch::tlb_shootdown_all();
                    }
                    msg.is_accomplished.store(true, Ordering::Release);
                },
                #[cfg(feature = "kunit")]
                RunKUnitPerCpu { test_fn } => {
                    crate::debug::kunit::handle_percpu_ipi_test(*test_fn);
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
                WakeUpTaskStaleSafe { tid, park } => {
                    let tid = *tid;
                    let task = get_task(&tid).expect("internal error: no such task to wake up");
                    let placement = wake_enqueue(task, *park);
                    *msg.wake_result.lock() = Some(placement);
                    kdebugln!(
                        "ipi wake placement: tid={} park={:?} placement={:?}",
                        tid,
                        park,
                        placement
                    );
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

    fn unused_test() {}

    #[kunit]
    fn test_broadcast_copy_reconstructs_only_eligible_payloads() {
        let tid = Tid::new(7);
        let copies = [
            IpiPayload::TlbShootdown { vpn: None }.copy_for_broadcast(),
            IpiPayload::EnqueueNewTask { tid }.copy_for_broadcast(),
            IpiPayload::WakeUpTaskStaleSafe {
                tid,
                park: ParkState::Parked,
            }
            .copy_for_broadcast(),
            IpiPayload::RunKUnitPerCpu {
                test_fn: unused_test,
            }
            .copy_for_broadcast(),
            IpiPayload::StopExecution.copy_for_broadcast(),
        ];

        let mut copies = copies.into_iter();
        assert!(matches!(
            copies.next().unwrap(),
            IpiPayload::TlbShootdown { vpn: None }
        ));
        assert!(matches!(
            copies.next().unwrap(),
            IpiPayload::EnqueueNewTask { tid: copied } if copied == tid
        ));
        assert!(matches!(
            copies.next().unwrap(),
            IpiPayload::WakeUpTaskStaleSafe {
                tid: copied,
                park: ParkState::Parked,
            } if copied == tid
        ));
        assert!(matches!(
            copies.next().unwrap(),
            IpiPayload::RunKUnitPerCpu { .. }
        ));
        assert!(matches!(copies.next().unwrap(), IpiPayload::StopExecution));
        assert!(copies.next().is_none());
    }
}

pub struct TlbShootdownGuard {
    vpn: Option<VirtPageNum>,
}

impl TlbShootdownGuard {
    pub fn new(vpn: Option<VirtPageNum>) -> Self {
        Self { vpn }
    }
}

impl Drop for TlbShootdownGuard {
    fn drop(&mut self) {
        if let Err(e) = broadcast_ipi(IpiPayload::TlbShootdown { vpn: self.vpn }) {
            kwarningln!("failed to send TLB shootdown IPI in TlbShootdownGuard: {e:?}");
        }
    }
}
