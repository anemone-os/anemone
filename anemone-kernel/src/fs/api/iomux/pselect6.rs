//! pselect6 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/pselect6.2.html
//! - https://elixir.bootlin.com/linux/v6.6.32/source/fs/select.c#L795
//!
//! Currently busy polling.

use anemone_abi::{
    fs::linux::select::{FD_SETSIZE, FdSet},
    process::linux::signal as linux_signal,
    time::linux::TimeSpec,
};

use crate::{
    prelude::*,
    syscall::{
        handler::TryFromSyscallArg,
        user_access::{SyscallArgValidatorExt as _, UserReadPtr, UserWritePtr, user_addr},
    },
    task::{
        files::Fd,
        sig::{SigNo, set::SigSet},
    },
    utils::bitmap::Bitmap,
};

type FdBitmap = Bitmap<{ FD_SETSIZE / 64 }>;

fn trim_fdset(bitmap: &mut FdBitmap, n: usize) {
    for fd_idx in n..FD_SETSIZE {
        bitmap.clear(fd_idx);
    }
}

// TODO: updating tsp.
#[syscall(SYS_PSELECT6)]
pub fn sys_pselect6(
    n: i32,
    #[validate_with(user_addr.nullable())] inp: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] outp: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] exp: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] tsp: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] sig: Option<VirtAddr>,
) -> Result<u64, SysError> {
    kdebugln!(
        "pselect6: n={}, inp={:?}, outp={:?}, exp={:?}, tsp={:?}, sig={:?}",
        n,
        inp,
        outp,
        exp,
        tsp,
        sig
    );

    if n < 0 {
        return Err(SysError::InvalidArgument);
    }
    if n as usize > FD_SETSIZE {
        return Err(SysError::InvalidArgument);
    }

    let n = n as usize;

    let task = get_current_task();
    let usp_handle = task.clone_uspace_handle();

    let (mut in_fds, mut out_fds, mut exp_fds, timeout, sigmask) = {
        let mut usp = usp_handle.lock();

        // this closure doesn't check if fds are indeed in task's fd table. it'll be
        // checked lazily when we actually try to access those fds.
        let mut collect_fds = |ptr: Option<VirtAddr>| {
            ptr.map(|ptr| {
                let fdset = UserReadPtr::<FdSet>::try_new(ptr, &mut usp)?.read();
                let mut bitmap = Bitmap::new_with(Box::new(fdset.fds_bits));
                trim_fdset(&mut bitmap, n);
                Ok(bitmap)
            })
            .transpose()
        };

        let in_fds = collect_fds(inp)?;
        let out_fds = collect_fds(outp)?;
        let exp_fds = collect_fds(exp)?;

        // TODO: we really should provide a common helper to convert timespec. currently
        // too much duplicated code.
        let timeout = tsp
            .map(|tsp| {
                let TimeSpec { tv_sec, tv_nsec } =
                    UserReadPtr::<TimeSpec>::try_new(tsp, &mut usp)?.read();
                if tv_sec < 0 || tv_nsec < 0 || tv_nsec >= 1_000_000_000 {
                    return Err(SysError::InvalidArgument);
                }
                Ok(Duration::new(tv_sec as u64, tv_nsec as u32))
            })
            .transpose()?;

        let sigmask = sig
            .map(|sig| {
                let linux_signal::SigSetArgPack { p, size } =
                    UserReadPtr::<linux_signal::SigSetArgPack>::try_new(sig, &mut usp)?.read();
                if size as usize != size_of::<linux_signal::SigSet>() {
                    return Err(SysError::InvalidArgument);
                }
                let linux_signal::SigSet { bits } =
                    UserReadPtr::<linux_signal::SigSet>::try_new(user_addr(p as u64)?, &mut usp)?
                        .read();
                Ok(SigSet::new_with_mask(bits))
            })
            .transpose()?
            .map(|sigmask| {
                sigmask.difference(&SigSet::new_with_signos(&[SigNo::SIGKILL, SigNo::SIGSTOP]))
            });

        (in_fds, out_fds, exp_fds, timeout, sigmask)
    };

    let prev_mask = sigmask.map(|mask| {
        let prev_mask = task.sig_mask();
        task.set_sig_mask(mask);
        prev_mask
    });

    let deadline = timeout.map(|timeout| Instant::now() + timeout);
    let wait_result = (|| -> Result<u64, SysError> {
        loop {
            let mut progressed = false;

            let mut scan_fds = |fds: &mut FdBitmap, interests: Option<PollEvent>| {
                let mut ready_fds = FdBitmap::new();

                for fd_idx in 0..n {
                    if !fds.test(fd_idx) {
                        continue;
                    }

                    let fd = Fd::try_from_syscall_arg(fd_idx as u64)?;
                    let fd = task.get_fd(fd)?;

                    if let Some(interests) = interests {
                        let revents = fd.poll(&PollRequest::snapshot(interests))?;
                        if !revents.is_empty() {
                            ready_fds.set(fd_idx);
                            progressed = true;
                        }
                    }
                }

                *fds = ready_fds;

                Ok(())
            };

            if let Some(in_fds) = in_fds.as_mut() {
                scan_fds(in_fds, Some(PollEvent::READABLE))?;
            }
            if let Some(out_fds) = out_fds.as_mut() {
                scan_fds(out_fds, Some(PollEvent::WRITABLE))?;
            }
            // stub implementation for POLLPRI.
            if let Some(exp_fds) = exp_fds.as_mut() {
                scan_fds(exp_fds, None)?;
            }

            if progressed {
                break;
            }

            if task.has_unmasked_signal() {
                kdebugln!("sys_pselect6: interrupted by signal");
                return Err(SysError::Interrupted);
            }

            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                break;
            }

            yield_now();
        }

        let ready = in_fds.as_ref().map(|fds| fds.count_ones()).unwrap_or(0)
            + out_fds.as_ref().map(|fds| fds.count_ones()).unwrap_or(0)
            + exp_fds.as_ref().map(|fds| fds.count_ones()).unwrap_or(0);

        Ok(ready as u64)
    })();

    if let Some(mask) = prev_mask {
        task.set_sig_mask(mask);
    }

    let retval = wait_result?;

    {
        // update user's fd sets.
        let mut usp = usp_handle.lock();

        let mut update_user_fds = |fds: &FdBitmap, ptr: VirtAddr| {
            let fdset = FdSet {
                fds_bits: *fds.dwords(),
            };
            UserWritePtr::<FdSet>::try_new(ptr, &mut usp)?.write(fdset);
            Ok(())
        };

        if let Some(in_fds) = in_fds.as_ref() {
            update_user_fds(in_fds, inp.unwrap())?;
        }
        if let Some(out_fds) = out_fds.as_ref() {
            update_user_fds(out_fds, outp.unwrap())?;
        }
        if let Some(exp_fds) = exp_fds.as_ref() {
            update_user_fds(exp_fds, exp.unwrap())?;
        }
    }

    Ok(retval)
}
