//! Inter-processor interrupt handling.
//!
//! **Dynamic memory allocation in IPI sendings are prohibited.**
//!
//! Currently only synchronous IPIs are supported.

use core::ptr::NonNull;

use intrusive_collections::UnsafeRef;

use crate::prelude::*;

#[derive(Debug, Clone, Copy)]
pub enum IpiPayload {
    TlbShootdown { vaddr: Option<VirtAddr> },
    StopExecution,
}

#[derive(Debug)]
struct IpiMsg {
    payload: IpiPayload,
    is_accomplished: AtomicBool,
    link: LinkedListLink,
}

impl IpiMsg {
    const fn new(payload: IpiPayload) -> IpiMsg {
        Self {
            payload,
            is_accomplished: AtomicBool::new(false),
            link: LinkedListLink::new(),
        }
    }
}

intrusive_adapter!(
    IpiMsgAdapter = UnsafeRef<IpiMsg>: IpiMsg { link => LinkedListLink }
);

#[percpu]
static IPI_QUEUE: SpinLock<LinkedList<IpiMsgAdapter>> =
    SpinLock::new(LinkedList::new(IpiMsgAdapter::NEW));

/// Send an IPI to the target CPU, synchronously waiting for the IPI to be
/// handled before returning.
pub fn send_ipi(cpu_id: usize, payload: IpiPayload) -> Result<(), IpiError> {
    if unsafe { with_core_local_remote(cpu_id, |core_local| !core_local.online()) } {
        return Err(IpiError::TargetOffline);
    }

    let msg = IpiMsg::new(payload);
    let msg_ptr = unsafe { UnsafeRef::from_raw(&msg) };
    unsafe {
        IPI_QUEUE.with_remote(cpu_id, |queue| {
            let mut queue = queue.lock_irqsave();
            queue.push_back(msg_ptr);
        })
    }
    IntrArch::send_ipi(cpu_id);
    loop {
        if msg.is_accomplished.load(Ordering::Acquire) {
            break;
        }
        core::hint::spin_loop();
    }

    Ok(())
}

/// Broadcast an IPI to all other CPUs, synchronously waiting for all of them to
/// handle the IPI before returning.
pub fn broadcast_ipi(payload: IpiPayload) -> Result<(), IpiError> {
    let cur_cpuid = CpuArch::cur_cpu_id().get();
    let ncpus = CpuArch::ncpus();
    for id in 0..ncpus {
        if id != cur_cpuid {
            if unsafe { with_core_local_remote(id, |core_local| !core_local.online()) } {
                return Err(IpiError::TargetOffline);
            }
        }
    }
    for id in 0..ncpus {
        if id != cur_cpuid {
            send_ipi(id, payload)?;
        }
    }
    Ok(())
}

#[derive(Debug)]
pub enum IpiError {
    TargetOffline,
    /// This error can only happen when sending an asynchronous IPI.
    NoAvailableBuffer,
}

mod msg_buffers {
    use core::mem::MaybeUninit;

    use super::*;

    #[percpu]
    pub static MSG_BUFFERS: [IpiMsg; MAX_CPUS] = {
        // `IpiMsg` should not implement `Copy`, so we have to do this manually.

        let mut arr = MaybeUninit::<[IpiMsg; MAX_CPUS]>::uninit();
        let mut i = 0;
        while i < MAX_CPUS {
            unsafe {
                arr.as_mut_ptr().cast::<IpiMsg>().add(i).write(IpiMsg {
                    // temporary payload, it will be overwritten before being sent
                    payload: IpiPayload::StopExecution,
                    is_accomplished: AtomicBool::new(true),
                    link: LinkedListLink::new(),
                });
            }
            i += 1;
        }
        unsafe { arr.assume_init() }
    };
}
use msg_buffers::*;

/// Find an available IPI message buffer for current core, and mark it as
/// occupied.
///
/// The returned pointer points to a [None] value.
fn alloc_avail_buf() -> Option<NonNull<IpiMsg>> {
    MSG_BUFFERS.with_mut(|buffers| {
        for buf in buffers.iter_mut() {
            if buf.is_accomplished.load(Ordering::Acquire) {
                buf.is_accomplished.store(false, Ordering::Release);
                return Some(NonNull::from(buf));
            }
        }
        None
    })
}

/// Send an IPI to the target CPU asynchronously.
pub fn send_ipi_async(cpu_id: usize, payload: IpiPayload) -> Result<(), IpiError> {
    if unsafe { with_core_local_remote(cpu_id, |core_local| !core_local.online()) } {
        return Err(IpiError::TargetOffline);
    }

    let mut buf_ptr = alloc_avail_buf().ok_or(IpiError::NoAvailableBuffer)?;
    unsafe { buf_ptr.as_mut().payload = payload };
    let buf_ptr = unsafe { UnsafeRef::from_raw(buf_ptr.as_ptr()) };

    unsafe {
        IPI_QUEUE.with_remote(cpu_id, |queue| {
            let mut queue = queue.lock_irqsave();
            queue.push_back(buf_ptr);
        })
    }

    IntrArch::send_ipi(cpu_id);
    Ok(())
}

/// Broadcast an IPI to all other CPUs asynchronously.
pub fn broadcast_ipi_async(payload: IpiPayload) -> Result<(), IpiError> {
    // check whether empty buffers are enough
    let ncpus = CpuArch::ncpus();
    for id in 0..ncpus {
        if id != CpuArch::cur_cpu_id().get() {
            if unsafe { with_core_local_remote(id, |core_local| !core_local.online()) } {
                return Err(IpiError::TargetOffline);
            }
        }
    }

    let mut navail_bufs = 0;
    MSG_BUFFERS.with(|buffers| {
        for (id, buf) in buffers[..ncpus].iter().enumerate() {
            if id == CpuArch::cur_cpu_id().get() {
                continue;
            }
            if buf.is_accomplished.load(Ordering::Acquire) {
                navail_bufs += 1;
            }
        }
    });
    if navail_bufs < ncpus - 1 {
        return Err(IpiError::NoAvailableBuffer);
    }
    let mut sent_bufs = 0;
    MSG_BUFFERS.with_mut(|buffers| {
        for (id, buf) in buffers[..ncpus].iter_mut().enumerate() {
            if id == CpuArch::cur_cpu_id().get() {
                continue;
            }
            if buf.is_accomplished.load(Ordering::Acquire) {
                buf.is_accomplished.store(false, Ordering::Release);
                buf.payload = payload;
                let buf_ptr = unsafe { UnsafeRef::from_raw(buf) };
                unsafe {
                    IPI_QUEUE.with_remote(id, |queue| {
                        let mut queue = queue.lock_irqsave();
                        queue.push_back(buf_ptr);
                    });
                    IntrArch::send_ipi(id);
                }
                sent_bufs += 1;
            }
            if sent_bufs >= ncpus - 1 {
                break;
            }
        }
    });
    Ok(())
}

/// IPI handler.
pub fn handle_ipi() {
    use IpiPayload::*;

    IPI_QUEUE.with(|queue| {
        loop {
            let Some(msg_ptr) = queue.lock_irqsave().pop_front() else {
                break;
            };
            let msg = msg_ptr.as_ref();
            kdebugln!(
                "({}) handle ipi: payload={:?}",
                CpuArch::cur_cpu_id(),
                msg.payload
            );
            match msg.payload {
                TlbShootdown { vaddr } => {
                    if let Some(vaddr) = vaddr {
                        PagingArch::tlb_shootdown(vaddr);
                    } else {
                        PagingArch::tlb_shootdown_all();
                    }
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
