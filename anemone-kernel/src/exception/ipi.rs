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

#[derive(Debug, Clone, Copy)]
pub enum IpiPayload {
    TlbShootdown {
        vpn: Option<VirtPageNum>,
    },
    #[cfg(feature = "kunit")]
    RunKUnitPerCpu {
        test_fn: fn(),
    },
    StopExecution,
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

#[percpu]
static IPI_QUEUE: SpinLock<LinkedList<Arc<IpiMsg>>> = SpinLock::new(LinkedList::new());

#[inline(always)]
fn alloc_ipi_msg(payload: IpiPayload) -> Result<Arc<IpiMsg>, IpiError> {
    Arc::try_new(IpiMsg::new(payload)).map_err(IpiError::Alloc)
}

#[inline(always)]
fn enqueue_ipi(cpu_id: usize, msg: Arc<IpiMsg>) {
    unsafe {
        IPI_QUEUE.with_remote(cpu_id, move |queue| {
            let mut queue = queue.lock_irqsave();
            queue.push_back(msg);
        })
    }
    IntrArch::send_ipi(cpu_id);
}

/// Send an IPI to the target CPU, synchronously waiting for the IPI to be
/// handled before returning.
pub fn send_ipi(cpu_id: usize, payload: IpiPayload) -> Result<(), IpiError> {
    if cpu_id == cur_cpu_id().get() {
        panic!("cannot send ipi to self");
    }
    if !target_online(cpu_id) {
        return Err(IpiError::TargetOffline);
    }

    let msg = alloc_ipi_msg(payload)?;
    enqueue_ipi(cpu_id, Arc::clone(&msg));
    loop {
        if msg.is_accomplished.load(Ordering::Acquire) {
            break;
        }
        spin_loop();
    }

    Ok(())
}

/// Broadcast an IPI to all other CPUs, synchronously waiting for all of them to
/// handle the IPI before returning.
pub fn broadcast_ipi(payload: IpiPayload) -> Result<(), IpiError> {
    let cur_cpuid = cur_cpu_id().get();
    let ncpus = ncpus();
    for id in 0..ncpus {
        if !target_online(id) {
            return Err(IpiError::TargetOffline);
        }
    }
    let mut pending = LinkedList::new();
    for id in 0..ncpus {
        if id != cur_cpuid {
            let msg = alloc_ipi_msg(payload)?;
            pending.push_back(Arc::clone(&msg));
            enqueue_ipi(id, msg);
        }
    }

    for msg in pending {
        while !msg.is_accomplished.load(Ordering::Acquire) {
            spin_loop();
        }
    }
    Ok(())
}

#[derive(Debug)]
pub enum IpiError {
    TargetOffline,
    Alloc(AllocError),
}

/// Send an IPI to the target CPU asynchronously.
pub fn send_ipi_async(cpu_id: usize, payload: IpiPayload) -> Result<(), IpiError> {
    if cpu_id == cur_cpu_id().get() {
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
    let cur_cpuid = cur_cpu_id().get();
    let ncpus = ncpus();
    for id in 0..ncpus {
        if id != cur_cpuid && !target_online(id) {
            return Err(IpiError::TargetOffline);
        }
    }

    let mut pending = LinkedList::new();
    for id in 0..ncpus {
        if id != cur_cpuid {
            pending.push_back((id, alloc_ipi_msg(payload)?));
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
            let Some(msg) = queue.lock_irqsave().pop_front() else {
                break;
            };
            match msg.payload {
                TlbShootdown { vpn } => {
                    if let Some(vpn) = vpn {
                        PagingArch::tlb_shootdown(vpn);
                    } else {
                        PagingArch::tlb_shootdown_all();
                    }
                    msg.is_accomplished.store(true, Ordering::Release);
                },
                #[cfg(feature = "kunit")]
                RunKUnitPerCpu { test_fn } => {
                    crate::debug::kunit::handle_percpu_ipi_test(test_fn);
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
