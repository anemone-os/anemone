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
    fn new(payload: IpiPayload) -> IpiMsg {
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

/// Send an IPI to the target CPU.
pub fn send_ipi(cpu_id: usize, payload: IpiPayload, asynk: bool) {
    if asynk {
        unimplemented!("asynchronous IPIs are not supported yet.");
    } else {
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
}

/// Broadcast an IPI to all other CPUs.
pub fn broadcast_ipi(payload: IpiPayload, asynk: bool) {
    let cur_cpuid = CpuArch::cur_cpu_id();
    let ncpus = CpuArch::ncpus();
    for id in 0..ncpus {
        if id != cur_cpuid {
            send_ipi(id, payload, asynk);
        }
    }
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
            }
        }
    })
}
