//! From-kernel trap handling.

use crate::{
    exception::{
        ipi::handle_ipi,
        trap::{
            InterruptReason,
            hal::{ExceptionReason, TrapArchTrait, TrapReason},
        },
    },
    prelude::*,
};

pub unsafe fn ktrap_handler(
    trapframe: &mut <TrapArch as TrapArchTrait>::TrapFrame,
    reason: TrapReason,
) {
    kdebugln!("handle kernel trap: reason={:?}", reason);

    match reason {
        TrapReason::Exception(exception) => match exception {
            ExceptionReason::Syscall(sysno) => {
                // for some architectures, here is actually unreachable.
                panic!("syscall {:?} called in kernel", sysno);
            },
            ExceptionReason::Breakpoint => {
                panic!("breakpoint in kernel");
            },
            ExceptionReason::PageFault(info) => {
                panic!(
                    "page fault in kernel: addr={:?}, type={:?}",
                    info.fault_addr(),
                    info.fault_type()
                );
            },
            ExceptionReason::DivisionByZero => {
                panic!("division by zero in kernel");
            },
            ExceptionReason::InvalidOpcode => {
                panic!("invalid opcode in kernel");
            },
            ExceptionReason::ArchFatal => {
                panic!("fatal architecture-specific exception in kernel");
            },
        },
        TrapReason::Interrupt(interrupt) => match interrupt {
            InterruptReason::Ipi => {
                handle_ipi();
            },
            _ => unimplemented!(),
        },
    }
}
