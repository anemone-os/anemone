//! LoongArch user-ALE adapter for software unaligned access.
//!
//! Owns the ERA/BADV handoff, first-use notice, and synchronous fault delivery.

mod access;
mod decode;

use crate::{
    arch::loongarch64::exception::trap::LA64TrapFrame,
    prelude::*,
    sched::current_task_id,
    syscall::user_access::UserReadPtr,
    task::sig::{
        SigNo, Signal,
        info::{SiCode, SigFault, SigInfoFields},
    },
};

fn send_fault_signal(signal: SigNo, address: VirtAddr) {
    get_current_task().recv_signal(Signal::new(
        signal,
        SiCode::Kernel,
        SigInfoFields::Fault(SigFault { addr: address }),
    ));
}

fn send_alignment_error(address: VirtAddr) {
    get_current_task().recv_signal(Signal::new(
        SigNo::SIGBUS,
        SiCode::BusAdraln,
        SigInfoFields::Fault(SigFault { addr: address }),
    ));
}

/// Handle one user ALE using the instruction at ERA and address in BADV.
pub(super) fn handle_user_unaligned_access(
    trapframe: &mut LA64TrapFrame,
    address: VirtAddr,
) {
    let pc = VirtAddr::new(trapframe.era());
    let uspace = get_current_task().clone_uspace_handle();

    // Follow Linux and fetch from the faulting user PC. BADI is not saved by
    // trap entry, so a nested synchronous exception can overwrite it before
    // this Rust handler runs.
    let instruction = {
        let mut guard = uspace.lock();
        UserReadPtr::<u32>::try_new(pc, &mut guard)
            .map(|instruction| with_intr_disabled(|| instruction.read()))
    };
    let instruction = match instruction {
        Ok(instruction) => instruction,
        Err(error) => {
            kerrln!(
                "({}) user {} failed to fetch unaligned instruction at {:#x}: {:?}",
                cur_cpu_id(),
                current_task_id(),
                pc.get(),
                error,
            );
            send_fault_signal(SigNo::SIGSEGV, pc);
            return;
        },
    };

    let access = match decode::decode(instruction, address) {
        Ok(access) => access,
        Err(_) => {
            kerrln!(
                "({}) unsupported unaligned access for task {}: instruction={:#010x}, pc={:#x}, address={:#x}",
                cur_cpu_id(),
                current_task_id(),
                instruction,
                trapframe.era(),
                address.get(),
            );
            send_alignment_error(address);
            return;
        },
    };

    // knoticeln!(
    //     "({}) software unaligned access task={} pc={:#x} instruction={:#010x} address={:#x} length={:#x} type={} register={} register_value={:#x}",
    //     cur_cpu_id(),
    //     current_task_id(),
    //     pc.get(),
    //     instruction,
    //     access.address().get(),
    //     access.length(),
    //     access.kind().label(),
    //     access.register(),
    //     trapframe.read_gpr(access.register()),
    // );

    let task = get_current_task();
    if task.enable_soft_unaligned_access() {
        kinfoln!(
            "({}) enabled software unaligned access for task {}; execution may be slower; first access address={:#x}, length={:#x}, type={}",
            cur_cpu_id(),
            task.tid(),
            access.address().get(),
            access.length(),
            access.kind().label(),
        );
    }

    // Do not retain task ownership across user-memory access or the subsequent
    // signal/exit arbitration. Signal occurrences never carry an Arc<Task>.
    drop(task);
    let result = {
        let mut guard = uspace.lock();
        access::emulate_user(access, trapframe, &mut guard)
    };
    drop(uspace);
    if let Err(error) = result {
        kerrln!(
            "({}) user {} failed software unaligned access at {:#x}: {:?}",
            cur_cpu_id(),
            current_task_id(),
            address.get(),
            error,
        );
        send_fault_signal(SigNo::SIGSEGV, address);
    }
}
