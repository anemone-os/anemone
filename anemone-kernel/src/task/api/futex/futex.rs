use super::*;
use crate::{
    prelude::*,
    syscall::{
        handler::TryFromSyscallArg,
        user_access::{SyscallArgValidatorExt as _, UserReadPtr, user_addr},
    },
};
use anemone_abi::{process::linux::futex::*, time::linux::TimeSpec};

fn futex_waiter_id(waiter: &Arc<FutexWaiter>) -> usize {
    Arc::as_ptr(waiter) as usize
}

fn futex_event_id(waiter: &Arc<FutexWaiter>) -> usize {
    &waiter.futex_available as *const Event as usize
}

#[derive(Debug)]
struct FutexOp {
    cmd: FutexCmd,
    flags: FutexCmdFlags,
}

#[derive(Debug)]
enum FutexCmd {
    Wait,
    Wake,
    Requeue,
    CmpRequeue,
    WakeOp,
    WaitBitset,
    WakeBitset,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FutexCmdFlags: i32 {
        const PRIVATE = FUTEX_PRIVATE_FLAG;
        const CLOCK_REALTIME = FUTEX_CLOCK_REALTIME;
    }
}

impl TryFromSyscallArg for FutexOp {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        let raw = raw as i32;

        let cmd = (raw & FUTEX_CMD_MASK) & !(FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME);

        let cmd = match cmd {
            FUTEX_WAIT => FutexCmd::Wait,
            FUTEX_WAKE => FutexCmd::Wake,
            FUTEX_REQUEUE => FutexCmd::Requeue,
            FUTEX_CMP_REQUEUE => FutexCmd::CmpRequeue,
            FUTEX_WAKE_OP => FutexCmd::WakeOp,
            FUTEX_WAIT_BITSET => FutexCmd::WaitBitset,
            FUTEX_WAKE_BITSET => FutexCmd::WakeBitset,

            FUTEX_FD => {
                knoticeln!("futex: FUTEX_FD is deprecated and not supported");
                return Err(SysError::NoSys);
            },

            FUTEX_LOCK_PI
            | FUTEX_UNLOCK_PI
            | FUTEX_TRYLOCK_PI
            | FUTEX_WAIT_REQUEUE_PI
            | FUTEX_CMP_REQUEUE_PI => {
                knoticeln!("futex: PI futexes are not yet implemented");
                return Err(SysError::NotYetImplemented);
            },

            _ => {
                knoticeln!("futex: unrecognized futex cmd: raw={:#x}", raw);
                return Err(SysError::InvalidArgument);
            },
        };

        let mut flags = FutexCmdFlags::empty();
        if raw & FUTEX_PRIVATE_FLAG != 0 {
            flags |= FutexCmdFlags::PRIVATE;
        }
        if raw & FUTEX_CLOCK_REALTIME != 0 {
            flags |= FutexCmdFlags::CLOCK_REALTIME;
            kwarningln!("futex: FUTEX_CLOCK_REALTIME is not yet implemented");
            // return Err(SysError::NotYetImplemented);
        }

        Ok(Self { cmd, flags })
    }
}

#[syscall(SYS_FUTEX, preparse = |uaddr, op, val, val2, raw_uaddr2, val3| {
    kdebugln!(
        "{} sys_futex preparse: uaddr={:#x?}, op={:#x}, val={}, val2={}, uaddr2={:#x}, val3={}",
        current_task_id(),
        uaddr,
        op,
        val,
        val2,
        raw_uaddr2,
        val3,
    );
})]
fn sys_futex(
    #[validate_with(user_addr)] uaddr: VirtAddr,
    op: FutexOp,
    val: u32,
    val2: u64,
    // some cmds don't use this argument, we can't validate it unconditionally.
    raw_uaddr2: u64,
    val3: u32,
) -> Result<u64, SysError> {
    kdebugln!(
        "{} sys_futex: uaddr={:#x?}, op={:?}, val={}, val2={}, uaddr2={:#x}, val3={}",
        current_task_id(),
        uaddr,
        op,
        val,
        val2,
        raw_uaddr2,
        val3
    );

    let task = get_current_task();
    let usp_handle = task.clone_uspace_handle();

    let futex1_key = {
        if !uaddr.get().is_multiple_of(4) {
            knoticeln!("futex: uaddr {} is not 4-byte aligned", uaddr);
            return Err(SysError::InvalidArgument);
        }
        calc_futex_key(&usp_handle, uaddr)?
    };

    // for now we ignore flags.

    match op.cmd {
        FutexCmd::Wait => {
            let timeout_ptr = (user_addr.nullable())(val2)?;

            let timeout = if let Some(timeout_ptr) = timeout_ptr {
                let mut usp = usp_handle.lock();
                let TimeSpec { tv_sec, tv_nsec } =
                    UserReadPtr::<TimeSpec>::try_new(timeout_ptr, &mut usp)?.read();

                if tv_sec < 0 || tv_nsec < 0 || tv_nsec >= 1_000_000_000 {
                    knoticeln!(
                        "futex: invalid timeout value: tv_sec={}, tv_nsec={}",
                        tv_sec,
                        tv_nsec
                    );
                    return Err(SysError::InvalidArgument);
                }
                let duration = Duration::new(tv_sec as u64, tv_nsec as u32);
                Some(duration)
            } else {
                None
            };

            futex_wait(uaddr, futex1_key, val, timeout, None)?;
            Ok(0)
        },
        FutexCmd::WaitBitset => {
            let bitset = val3;
            if bitset == 0 {
                knoticeln!("futex: FUTEX_WAIT_BITSET with bitset=0 is not allowed");
                return Err(SysError::InvalidArgument);
            }

            let timeout_ptr = (user_addr.nullable())(val2)?;
            let timeout = if let Some(timeout_ptr) = timeout_ptr {
                let mut usp = usp_handle.lock();
                let TimeSpec { tv_sec, tv_nsec } =
                    UserReadPtr::<TimeSpec>::try_new(timeout_ptr, &mut usp)?.read();

                if tv_sec < 0 || tv_nsec < 0 || tv_nsec >= 1_000_000_000 {
                    knoticeln!(
                        "futex: invalid timeout value: tv_sec={}, tv_nsec={}",
                        tv_sec,
                        tv_nsec
                    );
                    return Err(SysError::InvalidArgument);
                }
                // for waitbitset, timeout is a absolute time.
                let duration = Duration::new(tv_sec as u64, tv_nsec as u32)
                    .saturating_sub(Instant::now().to_duration());
                Some(duration)
            } else {
                None
            };

            futex_wait(uaddr, futex1_key, val, timeout, Some(bitset))?;
            Ok(0)
        },
        FutexCmd::Wake => {
            let n_woken = futex_wake(futex1_key, val, None)?;
            Ok(n_woken as u64)
        },
        FutexCmd::WakeBitset => {
            let bitset = val3;
            if bitset == 0 {
                knoticeln!("futex: FUTEX_WAKE_BITSET with bitset=0 is not allowed");
                return Err(SysError::InvalidArgument);
            }
            let n_woken = futex_wake(futex1_key, val, Some(bitset))?;
            Ok(n_woken as u64)
        },
        FutexCmd::Requeue => {
            let uaddr2 = user_addr(raw_uaddr2)?;

            let futex2_key = {
                if !uaddr2.get().is_multiple_of(4) {
                    knoticeln!("futex: uaddr2 {} is not 4-byte aligned", uaddr2);
                    return Err(SysError::InvalidArgument);
                }
                calc_futex_key(&usp_handle, uaddr2)?
            };

            let n_handled =
                futex_cmp_requeue(uaddr, futex1_key, futex2_key, val, val2 as u32, None)?;
            Ok(n_handled as u64)
        },
        FutexCmd::CmpRequeue => {
            let uaddr2 = user_addr(raw_uaddr2)?;

            let futex2_key = {
                if !uaddr2.get().is_multiple_of(4) {
                    knoticeln!("futex: uaddr2 {} is not 4-byte aligned", uaddr2);
                    return Err(SysError::InvalidArgument);
                }
                calc_futex_key(&usp_handle, uaddr2)?
            };

            let n_handled =
                futex_cmp_requeue(uaddr, futex1_key, futex2_key, val, val2 as u32, Some(val3))?;
            Ok(n_handled as u64)
        },
        FutexCmd::WakeOp => {
            knoticeln!("futex: FUTEX_WAKE_OP is not yet implemented");
            Err(SysError::NotYetImplemented)
        },
    }
}

/// `timeout` is relative.
fn futex_wait(
    word_addr: VirtAddr,
    key: FutexKey,
    val: u32,
    timeout: Option<Duration>,
    bitset: Option<u32>,
) -> Result<(), SysError> {
    let task = get_current_task();
    let usp_handle = task.clone_uspace_handle();

    // note that usp is locked before FUTEX_SET is locked.
    let mut usp = usp_handle.lock();

    let mut val_mismatch = false;
    let waiter = with_futex(key, true, |futex| {
        // this operation ensures we can safely access the futex word directly through
        // user pointer.
        usp.inject_page_fault(word_addr, PageFaultType::Read)?;

        let atomic_view = unsafe { (word_addr.as_ptr_mut() as *mut AtomicU32).as_ref().unwrap() };
        if atomic_view.load(Ordering::SeqCst) != val {
            val_mismatch = true;
            return Ok(None);
        }

        let waiter = Arc::new(FutexWaiter {
            key: SpinLock::new(key),
            task: task.clone(),
            futex_available: Event::new(),
            woken: AtomicBool::new(false),
            bitset: bitset.unwrap_or(FUTEX_BITSET_MATCH_ANY),
        });
        futex.waiters.push_back(waiter.clone());

        kdebugln!(
            "futex: queued waiter={:#x} task={} word_addr={:#x} key={:?} event={:#x} bitset={:#x} waiters={}",
            futex_waiter_id(&waiter),
            task.tid(),
            word_addr.get(),
            key,
            futex_event_id(&waiter),
            waiter.bitset,
            futex.waiters.len(),
        );

        Ok(Some(waiter))
    })
    .unwrap()?;

    drop(usp);

    if val_mismatch {
        return Err(SysError::Again);
    }

    let Some(waiter) = waiter else { unreachable!() };

    /// [with_futex] isn't used since it will lead to a deadlock.
    fn handle_wait_exception(waiter: &Arc<FutexWaiter>) -> Result<(), ()> {
        let mut set = FUTEX_SET.lock();
        let key = *waiter.key.lock();
        let waiter_id = futex_waiter_id(waiter);
        let event_id = futex_event_id(waiter);

        let Some(futex) = set.get_futex(key) else {
            // futex already removed. this means the waiter is already woken up by some
            // task, so treat it as success.
            kdebugln!(
                "futex: handle_wait_exception waiter={:#x} task={} event={:#x} key={:?} found_no_futex, treating as wake",
                waiter_id,
                waiter.task.tid(),
                event_id,
                key,
            );
            return Ok(());
        };

        if waiter.woken.load(Ordering::SeqCst) {
            // we are woken up by some task as well in this window, so treat it as success.
            kdebugln!(
                "futex: handle_wait_exception waiter={:#x} task={} event={:#x} key={:?} already_marked_woken",
                waiter_id,
                waiter.task.tid(),
                event_id,
                key,
            );
            return Ok(());
        }

        // oops. we should remove the waiter from the futex waiters list. find the
        // waiter in the waiters list and remove it.
        if let Some(pos) = futex.waiters.iter().position(|w| Arc::ptr_eq(w, waiter)) {
            futex.waiters.remove(pos);
            let remaining_waiters = futex.waiters.len();

            if futex.waiters.is_empty() {
                // no more waiters, we can remove the futex to save memory.
                set.remove_futex(key);
            }

            kdebugln!(
                "futex: handle_wait_exception waiter={:#x} task={} event={:#x} key={:?} removed_from_queue remaining_waiters={} removed_futex={}",
                waiter_id,
                waiter.task.tid(),
                event_id,
                key,
                remaining_waiters,
                remaining_waiters == 0,
            );

            Err(())
        } else {
            panic!("futex: inconsistency between key and waiter list.");
        }
    }

    kdebugln!(
        "futex: task {} entering wait on waiter={:#x} event={:#x} key={:?} timeout={:?}",
        task.tid(),
        futex_waiter_id(&waiter),
        futex_event_id(&waiter),
        key,
        timeout,
    );
    if let Some(timeout) = timeout {
        match waiter.futex_available.listen_with_timeout(
            true,
            || waiter.woken.load(Ordering::SeqCst),
            timeout,
        ) {
            None => {
                kdebugln!(
                    "futex: wait completed waiter={:#x} task={} event={:#x} via wake",
                    futex_waiter_id(&waiter),
                    waiter.task.tid(),
                    futex_event_id(&waiter),
                );
                Ok(())
            },
            Some(TimeoutListenException::Signaled) => {
                kdebugln!("futex: wait interrupted by signal");
                handle_wait_exception(&waiter).map_err(|()| SysError::Interrupted)
            },
            Some(TimeoutListenException::Timeout) => {
                kdebugln!("futex: wait timed out");
                handle_wait_exception(&waiter).map_err(|()| SysError::Timeout)
            },
        }
    } else {
        match waiter
            .futex_available
            .listen(true, || waiter.woken.load(Ordering::SeqCst))
        {
            true => {
                kdebugln!(
                    "futex: wait completed waiter={:#x} task={} event={:#x} via wake",
                    futex_waiter_id(&waiter),
                    waiter.task.tid(),
                    futex_event_id(&waiter),
                );
                Ok(())
            },
            false => handle_wait_exception(&waiter).map_err(|()| SysError::Interrupted),
        }
    }
}

fn futex_wake(key: FutexKey, n_waiters: u32, bitset: Option<u32>) -> Result<u32, SysError> {
    let bitset = bitset.unwrap_or(FUTEX_BITSET_MATCH_ANY);

    match with_futex(key, false, |futex| {
        let len = futex.waiters.len() as u32;
        let mut tested = 0;

        let mut woken = 0;
        while woken < n_waiters {
            if tested >= len {
                // preventing infinite loop.
                break;
            }

            if let Some(waiter) = futex.waiters.pop_front() {
                tested += 1;
                if waiter.bitset & bitset == 0 {
                    // this waiter doesn't match the bitset, skip it.
                    //
                    // this is a bit unfair. we should refine this later.
                    futex.waiters.push_back(waiter);
                    continue;
                }

                waiter.woken.store(true, Ordering::SeqCst);
                kdebugln!(
                    "futex: waking waiter={:#x} task={} key={:?} event={:#x} bitset_mask={:#x}",
                    futex_waiter_id(&waiter),
                    waiter.task.tid(),
                    *waiter.key.lock(),
                    futex_event_id(&waiter),
                    bitset,
                );
                waiter.futex_available.publish(1, true);
                woken += 1;
            } else {
                break;
            }
        }
        woken
    }) {
        Some(n_woken) => Ok(n_woken),
        // no waiters. just return 0 to indicate a successful wake.
        None => Ok(0),
    }
}

fn futex_cmp_requeue(
    word1_addr: VirtAddr,
    key1: FutexKey,
    key2: FutexKey,
    n_wake: u32,
    n_requeue: u32,
    cmp_val: Option<u32>,
) -> Result<u32, SysError> {
    let task = get_current_task();
    let usp_handle = task.clone_uspace_handle();

    // again, usp is locked before FUTEX_SET is locked.
    let mut usp = usp_handle.lock();
    match with_2_futex(key1, key2, true, |futex1, futex2| {
        usp.inject_page_fault(word1_addr, PageFaultType::Read)?;

        let atomic_view = unsafe {
            (word1_addr.as_ptr_mut() as *mut AtomicU32)
                .as_ref()
                .unwrap()
        };
        if let Some(cmp_val) = cmp_val {
            if atomic_view.load(Ordering::SeqCst) != cmp_val {
                // value doesn't match, no requeue or wake.
                return Err(SysError::Again);
            }
        }

        let mut n_woken = 0;
        let mut n_requeued = 0;

        // 1. wake waiters on futex1.
        while n_woken < n_wake {
            if let Some(waiter) = futex1.waiters.pop_front() {
                waiter.woken.store(true, Ordering::SeqCst);
                kdebugln!(
                    "futex: cmp_requeue waking waiter={:#x} task={} old_key={:?} event={:#x}",
                    futex_waiter_id(&waiter),
                    waiter.task.tid(),
                    *waiter.key.lock(),
                    futex_event_id(&waiter),
                );
                waiter.futex_available.publish(1, true);
                n_woken += 1;
            } else {
                break;
            }
        }

        // 2. requeue waiters from futex1 to futex2.
        while let Some(remaining_waiter) = futex1.waiters.pop_front() {
            let old_key = *remaining_waiter.key.lock();
            *remaining_waiter.key.lock() = key2;
            futex2.waiters.push_back(remaining_waiter);
            let requeued_waiter = futex2.waiters.back().unwrap();
            kdebugln!(
                "futex: requeued waiter={:#x} task={} old_key={:?} new_key={:?} event={:#x}",
                futex_waiter_id(requeued_waiter),
                requeued_waiter.task.tid(),
                old_key,
                key2,
                futex_event_id(requeued_waiter),
            );
            n_requeued += 1;
            if n_requeued >= n_requeue {
                break;
            }
        }

        Ok(n_woken + n_requeued)
    }) {
        Some(ret) => ret,
        None => {
            // futex1 doesn't exist. this can be regarded as no waiters on futex1.
            Ok(0)
        },
    }
}
