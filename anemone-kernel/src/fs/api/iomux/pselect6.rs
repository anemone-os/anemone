//! pselect6 system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/pselect6.2.html
//! - https://elixir.bootlin.com/linux/v6.6.32/source/fs/select.c#L795

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
        sig::{SigNo, TemporaryMaskWaitContext, set::SigSet},
    },
    utils::bitmap::Bitmap,
};

use super::*;

type FdBitmap = Bitmap<{ FD_SETSIZE / 64 }>;

fn trim_fdset(bitmap: &mut FdBitmap, n: usize) {
    for fd_idx in n..FD_SETSIZE {
        bitmap.clear(fd_idx);
    }
}

fn clear_ready_outputs(
    in_ready: &mut Option<FdBitmap>,
    out_ready: &mut Option<FdBitmap>,
    exp_ready: &mut Option<FdBitmap>,
) {
    if let Some(fds) = in_ready.as_mut() {
        fds.clear_all();
    }
    if let Some(fds) = out_ready.as_mut() {
        fds.clear_all();
    }
    if let Some(fds) = exp_ready.as_mut() {
        fds.clear_all();
    }
}

fn scan_pselect_fdset(
    task: &Arc<Task>,
    n: usize,
    interest_fds: Option<&FdBitmap>,
    ready_fds: Option<&mut FdBitmap>,
    request: PollRequest<'_>,
    nready: &mut usize,
    unsupported: &mut bool,
) -> Result<(), SysError> {
    let Some(interest_fds) = interest_fds else {
        return Ok(());
    };
    let ready_fds = ready_fds.expect("pselect interest fdset without ready output fdset");

    for fd_idx in 0..n {
        if !interest_fds.test(fd_idx) {
            continue;
        }

        let fd = Fd::try_from_syscall_arg(fd_idx as u64)?;
        let fd = task.get_fd(fd)?;

        match fd.poll(&request) {
            Ok(PollRegisterResult::Ready(revents)) if !revents.is_empty() => {
                ready_fds.set(fd_idx);
                *nready += 1;
            },
            Ok(PollRegisterResult::Ready(_)) if request.is_register() => {
                kwarningln!(
                    "sys_pselect6: register scan returned empty ready for fd {} interests={:?}",
                    fd_idx,
                    request.interests(),
                );
                *unsupported = true;
            },
            Ok(PollRegisterResult::Ready(_)) => {},
            Ok(PollRegisterResult::Armed) if request.is_register() => {},
            Ok(PollRegisterResult::Armed) => {
                kwarningln!(
                    "sys_pselect6: snapshot scan unexpectedly armed fd {}",
                    fd_idx
                );
                return Err(SysError::IO);
            },
            Ok(PollRegisterResult::Unsupported) => {
                kdebugln!(
                    "sys_pselect6: unsupported register source fd {} interests={:?}",
                    fd_idx,
                    request.interests(),
                );
                *unsupported = true;
            },
            Err(err) => {
                knoticeln!("sys_pselect6: poll error for fd {}: {:?}", fd_idx, err);
                return Err(err);
            },
        }
    }

    Ok(())
}

fn validate_exception_fdset(
    task: &Arc<Task>,
    n: usize,
    exception_fds: Option<&FdBitmap>,
) -> Result<bool, SysError> {
    let Some(exception_fds) = exception_fds else {
        return Ok(false);
    };

    let mut has_interest = false;
    for fd_idx in 0..n {
        if !exception_fds.test(fd_idx) {
            continue;
        }
        has_interest = true;

        let fd = Fd::try_from_syscall_arg(fd_idx as u64)?;
        let _ = task.get_fd(fd)?;
    }

    Ok(has_interest)
}

fn scan_pselect_fds(
    task: &Arc<Task>,
    n: usize,
    in_interests: Option<&FdBitmap>,
    out_interests: Option<&FdBitmap>,
    exp_interests: Option<&FdBitmap>,
    in_ready: &mut Option<FdBitmap>,
    out_ready: &mut Option<FdBitmap>,
    exp_ready: &mut Option<FdBitmap>,
    mode: IomuxScanMode<'_>,
) -> Result<IomuxScanOutcome, SysError> {
    clear_ready_outputs(in_ready, out_ready, exp_ready);

    let mut nready = 0;
    let mut unsupported = false;

    scan_pselect_fdset(
        task,
        n,
        in_interests,
        in_ready.as_mut(),
        mode.poll_request(PollEvent::READABLE),
        &mut nready,
        &mut unsupported,
    )?;

    scan_pselect_fdset(
        task,
        n,
        out_interests,
        out_ready.as_mut(),
        mode.poll_request(PollEvent::WRITABLE),
        &mut nready,
        &mut unsupported,
    )?;

    // Exception readiness has no internal PollEvent yet; keep output empty and
    // do not register it as a latch source.
    let has_exception_interest = validate_exception_fdset(task, n, exp_interests)?;

    if nready > 0 {
        Ok(IomuxScanOutcome::Ready(nready))
    } else if unsupported || (mode.is_register() && has_exception_interest) {
        Ok(IomuxScanOutcome::Unsupported)
    } else {
        Ok(IomuxScanOutcome::NotReady)
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

    let (in_interests, out_interests, exp_interests, timeout, sigmask) = {
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
                if p.is_null() {
                    return Ok(None);
                }
                if size as usize != size_of::<linux_signal::SigSet>() {
                    return Err(SysError::InvalidArgument);
                }
                let linux_signal::SigSet { bits } =
                    UserReadPtr::<linux_signal::SigSet>::try_new(user_addr(p as u64)?, &mut usp)?
                        .read();
                Ok(Some(SigSet::new_with_mask(bits)))
            })
            .transpose()?
            .flatten()
            .map(|sigmask| {
                sigmask.difference(&SigSet::new_with_signos(&[SigNo::SIGKILL, SigNo::SIGSTOP]))
            });

        (in_fds, out_fds, exp_fds, timeout, sigmask)
    };

    let mut in_ready = in_interests.as_ref().map(|_| FdBitmap::new());
    let mut out_ready = out_interests.as_ref().map(|_| FdBitmap::new());
    let mut exp_ready = exp_interests.as_ref().map(|_| FdBitmap::new());

    let token = sigmask.map(|mask| task.begin_temporary_sig_mask(mask));
    let wait_outcome = wait_for_iomux_ready("sys_pselect6", &task, timeout, |mode| {
        scan_pselect_fds(
            &task,
            n,
            in_interests.as_ref(),
            out_interests.as_ref(),
            exp_interests.as_ref(),
            &mut in_ready,
            &mut out_ready,
            &mut exp_ready,
            mode,
        )
    });
    let retval = match token {
        Some(token) => finish_temporary_iomux_wait(
            "sys_pselect6",
            &task,
            token,
            wait_outcome,
            TemporaryMaskWaitContext::Pselect6,
        )?,
        None => wait_outcome.into_result_without_temporary_mask()?,
    };

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

        if let Some(in_fds) = in_ready.as_ref() {
            update_user_fds(in_fds, inp.unwrap())?;
        }
        if let Some(out_fds) = out_ready.as_ref() {
            update_user_fds(out_fds, outp.unwrap())?;
        }
        if let Some(exp_fds) = exp_ready.as_ref() {
            update_user_fds(exp_fds, exp.unwrap())?;
        }
    }

    Ok(retval as u64)
}
