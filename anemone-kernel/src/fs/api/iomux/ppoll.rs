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
    task::{files::Fd, sig::{SigNo, set::SigSet}},
};

use anemone_abi::{
    fs::linux::poll::PollFd as LinuxPollFd,
    process::linux::signal::SigSet as LinuxSigSet,
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
        if linux_events.contains(LinuxPollEvent::ERR) {
            events |= PollEvent::ERROR;
        }

        if linux_events.contains(LinuxPollEvent::PRI) {
            knoticeln!("sys_ppoll: POLLPRI is not supported yet.");
            return Err(SysError::NotYetImplemented);
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
    #[validate_with(user_addr)] ufds: VirtAddr,
    nfds: u32,
    #[validate_with(user_addr.nullable())] tsp: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] sigmask: Option<VirtAddr>,
) -> Result<u64, SysError> {
    kdebugln!(
        "sys_ppoll: ufds={}, nfds={}, tsp={:#?}, sigmask={:?}",
        ufds,
        nfds,
        tsp,
        sigmask
    );

    if nfds == 0 {
        kdebugln!("sys_ppoll: nfds is zero");
        return Err(SysError::InvalidArgument);
    }

    let task = get_current_task();

    let (mut poll_fds, revent_ptrs, tsp, sigmask) = {
        let usp_handle = task.clone_uspace_handle();
        let mut usp = usp_handle.lock();

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

        let tsp = if let Some(tsp_ptr) = tsp {
            Some(UserReadPtr::<TimeSpec>::try_new(tsp_ptr, &mut usp)?.read())
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
            sigmask
        } else {
            SigSet::new()
        };

        (poll_fds, revent_ptrs, tsp, sigmask)
    };

    if tsp.is_some() {
        knoticeln!("sys_ppoll: timeout is not supported yet.");
        return Err(SysError::NotYetImplemented);
    }

    let mut nready = 0;

    let prev_sigmask = task.sig_mask();
    task.set_sig_mask(sigmask);

    loop {
        for poll_fd in poll_fds.iter_mut() {
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
            } else {
                //  just for clarity.
                poll_fd.revents = LinuxPollEvent::empty();
            }
        }

        if nready > 0 {
            break;
        }

        if task.has_unmasked_signal() {
            kdebugln!("sys_ppoll: interrupted by signal");
            break;
        }

        yield_now();
    }

    {
        let usp_handle = task.clone_uspace_handle();
        let mut usp = usp_handle.lock();

        for (poll_fd, revent_ptr) in poll_fds.iter().zip(revent_ptrs.iter()) {
            UserWritePtr::<LinuxPollEvent>::try_new(*revent_ptr, &mut usp)
                .map_err(|e| {
                    task.set_sig_mask(prev_sigmask);
                    e
                })?
                .write(poll_fd.revents);
        }
    }

    task.set_sig_mask(prev_sigmask);

    if nready == 0 {
        kdebugln!("sys_ppoll: interrupted by signal");
        Err(SysError::Interrupted)
    } else {
        kdebugln!("sys_ppoll: {} fds are ready", nready);
        Ok(nready)
    }
}
