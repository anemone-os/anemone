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
    TlbShootdown {
        vaddr: Option<VirtAddr>,
    },
    StopExecution,

    /// Placeholder for empty payload.
    ///
    /// **IPI system internal usage only. Should never be sent by external
    /// code.**
    Empty,
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
pub fn send_ipi(cpu_id: usize, payload: IpiPayload) {
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
}

/// Broadcast an IPI to all other CPUs, synchronously waiting for all of them to
/// handle the IPI before returning.
pub fn broadcast_ipi(payload: IpiPayload) {
    let cur_cpuid = CpuArch::cur_cpu_id();
    let ncpus = CpuArch::ncpus();
    for id in 0..ncpus {
        if id != cur_cpuid {
            send_ipi(id, payload);
        }
    }
}

#[derive(Debug)]
pub enum AsyncIpiError {
    NoAvailableBuffer,
}

mod msg_buffers {
    use core::mem::MaybeUninit;

    use super::*;

    #[percpu]
    pub static MSG_BUFFERS: [IpiMsg; MAX_CPUS] = {
        // `IpiMsg` cannot implement `Copy`, so we have to do this manually.

        let mut arr = MaybeUninit::<[IpiMsg; MAX_CPUS]>::uninit();
        let mut i = 0;
        while i < MAX_CPUS {
            unsafe {
                arr.as_mut_ptr()
                    .cast::<IpiMsg>()
                    .add(i)
                    .write(IpiMsg::new(IpiPayload::Empty));
            }
            i += 1;
        }
        unsafe { arr.assume_init() }
    };
}
use msg_buffers::*;

/// Find an available IPI message buffer for current core.
///
/// The returned pointer points to a [None] value.
fn find_avail_buf() -> Option<NonNull<IpiMsg>> {
    MSG_BUFFERS.with_mut(|buffers| {
        for buf in buffers.iter_mut() {
            if let IpiPayload::Empty = buf.payload {
                return Some(NonNull::from(buf));
            }
        }
        None
    })
}

/// Send an IPI to the target CPU asynchronously.
pub fn send_ipi_async(cpu_id: usize, payload: IpiPayload) -> Result<(), AsyncIpiError> {
    let mut buf_ptr = find_avail_buf().ok_or(AsyncIpiError::NoAvailableBuffer)?;
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
pub fn broadcast_ipi_async(payload: IpiPayload) -> Result<(), AsyncIpiError> {
    // check whether empty buffers are enough
    let mut navail_bufs = 0;
    MSG_BUFFERS.with(|buffers| {
        for buf in buffers.iter() {
            if let IpiPayload::Empty = buf.payload {
                navail_bufs += 1;
            }
        }
    });
    if navail_bufs < CpuArch::ncpus() - 1 {
        return Err(AsyncIpiError::NoAvailableBuffer);
    }
    MSG_BUFFERS.with_mut(|buffers| {
        for (id, buf) in buffers.iter_mut().enumerate().skip(1) {
            if let IpiPayload::Empty = buf.payload {
                buf.payload = payload;

                let buf_ptr = unsafe { UnsafeRef::from_raw(buf) };
                unsafe {
                    IPI_QUEUE.with_remote(id, |queue| {
                        let mut queue = queue.lock_irqsave();
                        queue.push_back(buf_ptr);
                    });
                    IntrArch::send_ipi(id);
                }
            } else {
                panic!("concurrent modification of IPI message buffers detected");
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
            let msg = unsafe { msg_ptr.as_ref() };
            kdebugln!("handle ipi: payload={:?}", msg.payload);
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
                Empty => {
                    panic!("ipi with empty payload should never be sent");
                },
            }
        }
    })
}
