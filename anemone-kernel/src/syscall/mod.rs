//! TODO: refactor user_access.

use anemone_abi::syscall::ANEMONE_SYSNO_MAX;

use crate::{
    prelude::{handler::SyscallRegs, *},
    syscall::handler::SyscallHandler,
};

pub mod handler;
pub mod user_access;

const INVALID_SYSCALL: SyscallHandler = SyscallHandler {
    sysno: 0,
    nargs: 0,
    name: "invalid",
    handler: |_, _| Err(SysError::NoSys),
};

/// Syscall handler table. Singleton instance.
#[derive(Debug)]
struct SyscallTable {
    handlers: [SyscallHandler; ANEMONE_SYSNO_MAX as usize],
}

static SYSCALL_TABLE: MonoOnce<SyscallTable> = unsafe {
    MonoOnce::from_partial_initialized(SyscallTable {
        handlers: [INVALID_SYSCALL; ANEMONE_SYSNO_MAX as usize],
    })
};

pub fn register_syscall_handlers() {
    SYSCALL_TABLE.init(|s| {
        use link_symbols::{__esyscall, __ssyscall};

        let handlers = unsafe {
            let (start, end) = (
                __ssyscall as *const () as usize,
                __esyscall as *const () as usize,
            );
            assert!(start.is_multiple_of(align_of::<SyscallHandler>()));
            assert!((end - start).is_multiple_of(size_of::<SyscallHandler>()));
            let handler_count = (end - start) / size_of::<SyscallHandler>();
            core::slice::from_raw_parts(start as *const SyscallHandler, handler_count)
        };

        let handler_ptr = s.as_mut_ptr().cast::<SyscallHandler>();

        for handler in handlers {
            unsafe {
                assert!(handler.sysno < ANEMONE_SYSNO_MAX as usize);

                let h = &*handler_ptr.add(handler.sysno);

                if h.sysno != 0 {
                    panic!(
                        "duplicate syscall number {} for handlers {} and {}",
                        handler.sysno, h.name, handler.name,
                    );
                }

                knoticeln!(
                    "registering syscall handler {} for syscall number {}",
                    handler.name,
                    handler.sysno
                );

                handler_ptr.add(handler.sysno).write(*handler);
            }
        }
    });
}

/// System call handler.
///
/// For syscall occurring in kernel space, arch-specific code should just panic
/// immediately, and this function should never be called.
pub fn handle_syscall(trapframe: &mut TrapFrame) -> Option<RestartSyscall> {
    let sysno = trapframe.syscall_no();

    let handler = SYSCALL_TABLE
        .get()
        .handlers
        .get(sysno)
        .copied()
        .unwrap_or(INVALID_SYSCALL);

    if sysno < ANEMONE_SYSNO_MAX as usize && handler.sysno == 0 {
        knoticeln!(
            "unknown syscall number {} from {}",
            sysno,
            current_task_id()
        );
    }

    let regs = SyscallRegs {
        sysno,
        args: [
            trapframe.syscall_arg::<0>(),
            trapframe.syscall_arg::<1>(),
            trapframe.syscall_arg::<2>(),
            trapframe.syscall_arg::<3>(),
            trapframe.syscall_arg::<4>(),
            trapframe.syscall_arg::<5>(),
        ],
    };

    trapframe.advance_syscall_pc();

    let (retval, restart) = match (handler.handler)(&regs, trapframe) {
        Ok(retval) => (retval, None),
        Err(err) => {
            let retval = -(i64::from(err.as_errno())) as u64;
            let restart = if let SysError::RestartSyscall(restart) = err {
                Some(restart)
            } else {
                None
            };
            (retval, restart)
        },
    };

    trapframe.set_syscall_retval(retval);

    restart
}
