//! ppoll system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/ppoll.2.html
//! - https://elixir.bootlin.com/linux/v6.6.32/source/fs/select.c#L1101

use core::mem::offset_of;

use super::*;
use crate::{
    prelude::*,
    syscall::{
        handler::TryFromSyscallArg,
        user_access::{
            SyscallArgValidatorExt as _, UserReadPtr, UserReadSlice, UserWritePtr, user_addr,
        },
    },
    task::{
        files::Fd,
        sig::{SigNo, set::SigSet},
    },
};

use anemone_abi::{
    fs::linux::poll::PollFd as LinuxPollFd, process::linux::signal::SigSet as LinuxSigSet,
    time::linux::TimeSpec,
};

#[derive(Debug)]
struct PollFd {
    fd: Option<Fd>,
    events: PollEvent,
    revents: LinuxPollEvent,
}

impl PollFd {
    fn try_from_linux(fd: i32, events: i16) -> Result<Self, SysError> {
        let fd = if fd >= 0 {
            Some(Fd::try_from_syscall_arg(fd as u64)?)
        } else {
            None
        };

        let linux_events = LinuxPollEvent::from_bits(events)
            .ok_or(SysError::InvalidArgument)
            .map_err(|e| {
                knoticeln!("sys_ppoll: unrecognized poll event bits: {:#x}", events,);
                e
            })?
            .difference(LinuxPollEvent::NVAL | LinuxPollEvent::ERR | LinuxPollEvent::HUP);

        let mut events = PollEvent::empty();

        if linux_events.contains(LinuxPollEvent::IN) {
            events |= PollEvent::READABLE;
        }
        if linux_events.contains(LinuxPollEvent::OUT) {
            events |= PollEvent::WRITABLE;
        }
        if linux_events.contains(LinuxPollEvent::RDHUP) {
            events |= PollEvent::HANG_UP;
        }

        Ok(Self {
            fd,
            events,
            revents: LinuxPollEvent::empty(),
        })
    }
}

#[syscall(SYS_PPOLL)]
fn sys_ppoll(
    #[validate_with(user_addr.nullable())] ufds: Option<VirtAddr>,
    nfds: u32,
    #[validate_with(user_addr.nullable())] tsp: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] sigmask: Option<VirtAddr>,
    sigsetsize: usize,
) -> Result<u64, SysError> {
    kdebugln!(
        "sys_ppoll: ufds={:?}, nfds={}, tsp={:#?}, sigmask={:?}, sigsetsize={}",
        ufds,
        nfds,
        tsp,
        sigmask,
        sigsetsize
    );

    if nfds as usize > MAX_FD_PER_PROCESS {
        knoticeln!("sys_ppoll: nfds {} exceeds MAX_FD_PER_PROCESS", nfds);
        return Err(SysError::InvalidArgument);
    }
    if sigmask.is_some() && sigsetsize != size_of::<LinuxSigSet>() {
        knoticeln!("sys_ppoll: invalid sigsetsize: {}", sigsetsize);
        return Err(SysError::InvalidArgument);
    }

    let task = get_current_task();

    let (mut poll_fds, revent_ptrs, timeout, sigmask) = {
        let usp_handle = task.clone_uspace_handle();
        let mut usp = usp_handle.lock();

        let (poll_fds, revent_ptrs) = if nfds == 0 {
            (Vec::new(), Vec::new())
        } else {
            let ufds = ufds.ok_or(SysError::NotMapped)?;
            let revent_ptrs = (0..nfds)
                .map(|i| {
                    user_addr(
                        (ufds.get() as usize
                            + i as usize * size_of::<LinuxPollFd>()
                            + offset_of!(LinuxPollFd, revents)) as u64,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;

            let mut poll_fds_kbuf = vec![LinuxPollFd::default(); nfds as usize];
            UserReadSlice::<LinuxPollFd>::try_new(ufds, nfds as usize, &mut usp)?
                .copy_to_slice(&mut poll_fds_kbuf);
            let poll_fds = poll_fds_kbuf
                .into_iter()
                .map(|pollfd| PollFd::try_from_linux(pollfd.fd, pollfd.events))
                .collect::<Result<Vec<_>, _>>()?;

            (poll_fds, revent_ptrs)
        };

        let timeout = if let Some(tsp_ptr) = tsp {
            let ts = UserReadPtr::<TimeSpec>::try_new(tsp_ptr, &mut usp)?.read();
            if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
                knoticeln!("sys_ppoll: invalid timeout: {:?}", ts);
                return Err(SysError::InvalidArgument);
            }
            Some(Duration::from_secs(ts.tv_sec as u64) + Duration::from_nanos(ts.tv_nsec as u64))
        } else {
            None
        };

        let sigmask = if let Some(sigmask_ptr) = sigmask {
            let mut sigmask = SigSet::new_with_mask(
                UserReadPtr::<LinuxSigSet>::try_new(sigmask_ptr, &mut usp)?
                    .read()
                    .bits,
            );
            sigmask.clear(SigNo::SIGKILL);
            sigmask.clear(SigNo::SIGSTOP);
            Some(sigmask)
        } else {
            None
        };

        (poll_fds, revent_ptrs, timeout, sigmask)
    };

    let prev_sigmask = sigmask.map(|sigmask| {
        let prev_sigmask = task.sig_mask();
        task.set_sig_mask(sigmask);
        prev_sigmask
    });

    let deadline = timeout.map(|timeout| Instant::now() + timeout);
    let wait_result = (|| -> Result<u64, SysError> {
        let nready = loop {
            let mut nready = 0;

            for poll_fd in poll_fds.iter_mut() {
                poll_fd.revents = LinuxPollEvent::empty();

                if let Some(fd) = poll_fd.fd {
                    let Ok(fd) = task.get_fd(fd) else {
                        poll_fd.revents = LinuxPollEvent::NVAL;
                        nready += 1;
                        continue;
                    };
                    match fd.poll(&PollRequest::snapshot(poll_fd.events)) {
                        Ok(r) => {
                            if !r.is_empty() {
                                poll_fd.revents = LinuxPollEvent::from_kernel_poll_event(r);
                                nready += 1;
                            }
                        },
                        Err(e) => {
                            knoticeln!("sys_ppoll: poll error: {:?}", e);
                            poll_fd.revents = LinuxPollEvent::ERR;
                            nready += 1;
                        },
                    }
                }
            }

            if nready > 0 {
                break nready;
            }

            if task.has_unmasked_signal() {
                kdebugln!("sys_ppoll: interrupted by signal");
                return Err(SysError::Interrupted);
            }

            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                break 0;
            }

            yield_now();
        };

        if !revent_ptrs.is_empty() {
            let usp_handle = task.clone_uspace_handle();
            let mut usp = usp_handle.lock();

            for (poll_fd, revent_ptr) in poll_fds.iter().zip(revent_ptrs.iter()) {
                UserWritePtr::<LinuxPollEvent>::try_new(*revent_ptr, &mut usp)?
                    .write(poll_fd.revents);
            }
        }

        Ok(nready)
    })();

    if let Some(prev_sigmask) = prev_sigmask {
        task.set_sig_mask(prev_sigmask);
    }

    let nready = wait_result?;

    kdebugln!("sys_ppoll: {} fds are ready", nready);
    Ok(nready)
}
