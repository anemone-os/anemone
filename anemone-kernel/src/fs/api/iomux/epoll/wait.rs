use anemone_abi::{
    fs::linux::epoll::{EPOLLERR, EPOLLHUP, EPOLLIN, EPOLLOUT, EpollEvent},
    process::linux::signal::SigSet as LinuxSigSet,
    syscall::{SYS_EPOLL_PWAIT, SYS_EPOLL_PWAIT2},
    time::linux::TimeSpec,
};

use crate::{
    fs::{
        PollEvent, PollRegisterResult,
        epoll::{Epoll, EpollHarvest},
        iomux::IomuxWaitRound,
    },
    prelude::*,
    syscall::user_access::{SyscallArgValidatorExt as _, UserReadPtr, UserWriteSlice, user_addr},
    task::{
        files::{Fd, FileDesc},
        sig::{SigNo, TemporaryMaskWaitContext, set::SigSet},
    },
};

use super::{
    super::wait::{IomuxWaitOutcome, finish_temporary_iomux_wait},
    resolve_epoll_fd,
};

enum EpollWaitResult<'a> {
    Ready(EpollHarvest<'a>),
    Terminal(IomuxWaitOutcome),
}

fn harvest_ready<'a>(
    epoll: &'a Epoll,
    maxevents: usize,
) -> Result<Option<EpollHarvest<'a>>, SysError> {
    let harvest = epoll.harvest(maxevents)?;
    if harvest.events().is_empty() {
        drop(harvest);
        Ok(None)
    } else {
        Ok(Some(harvest))
    }
}

fn terminal_after_empty(
    outcome: LatchWaitOutcome,
    register_retry: bool,
) -> Option<IomuxWaitOutcome> {
    match outcome {
        LatchWaitOutcome::Triggered => None,
        LatchWaitOutcome::Cancelled if register_retry => None,
        LatchWaitOutcome::Timeout if !register_retry => Some(IomuxWaitOutcome::Timeout),
        LatchWaitOutcome::Signal => Some(IomuxWaitOutcome::Signal),
        LatchWaitOutcome::Force => Some(IomuxWaitOutcome::Force),
        LatchWaitOutcome::Cancelled | LatchWaitOutcome::Timeout | LatchWaitOutcome::Unexpected => {
            kwarningln!(
                "epoll wait: unexpected empty final harvest outcome={:?} register_retry={}",
                outcome,
                register_retry,
            );
            Some(IomuxWaitOutcome::Error(SysError::IO))
        },
    }
}

fn wait_for_harvest<'a>(
    context: &'static str,
    task: &Arc<Task>,
    epoll: &'a Epoll,
    epoll_file: &FileDesc,
    maxevents: usize,
    timeout: Option<Duration>,
) -> EpollWaitResult<'a> {
    let deadline = timeout.map(|duration| {
        let now = Instant::now();
        // Linux accepts every nonnegative i64 timespec. Durations beyond the
        // monotonic clock's representable horizon are still valid waits, so
        // clamp them to the farthest deadline instead of panicking in `Add`.
        now.checked_add(duration)
            .unwrap_or(Instant::from_mono(u64::MAX))
    });

    loop {
        match harvest_ready(epoll, maxevents) {
            Ok(Some(harvest)) => return EpollWaitResult::Ready(harvest),
            Ok(None) => {},
            Err(err) => return EpollWaitResult::Terminal(IomuxWaitOutcome::Error(err)),
        }

        if task.has_unmasked_signal() {
            return EpollWaitResult::Terminal(IomuxWaitOutcome::Signal);
        }

        let remaining = match deadline {
            Some(deadline) => {
                let now = Instant::now();
                if now >= deadline {
                    return EpollWaitResult::Terminal(IomuxWaitOutcome::Timeout);
                }
                Some(deadline.saturating_duration_since(now))
            },
            None => None,
        };

        let round = IomuxWaitRound::begin_current();
        let request = round.poll_request(PollEvent::READABLE);
        let (register_ready, register_recheck) = match epoll_file.poll(&request) {
            Ok(PollRegisterResult::Subscribed(events)) => (!events.is_empty(), false),
            Ok(PollRegisterResult::SubscribedRecheck) => (false, true),
            Ok(PollRegisterResult::Ready(events)) if !events.is_empty() => (true, false),
            Ok(PollRegisterResult::Ready(_) | PollRegisterResult::Unsupported) => {
                round.cancel(LatchCancelReason::RegisterError);
                let outcome = round.finish();
                kwarningln!(
                    "{}: epoll file failed to publish READABLE route outcome={:?}",
                    context,
                    outcome,
                );
                match harvest_ready(epoll, maxevents) {
                    Ok(Some(harvest)) => return EpollWaitResult::Ready(harvest),
                    Ok(None) => {
                        return EpollWaitResult::Terminal(IomuxWaitOutcome::Error(
                            SysError::NotSupported,
                        ));
                    },
                    Err(err) => {
                        return EpollWaitResult::Terminal(IomuxWaitOutcome::Error(err));
                    },
                }
            },
            Err(err) => {
                round.cancel(LatchCancelReason::SyscallError);
                let outcome = round.finish();
                kwarningln!(
                    "{}: epoll file route registration failed err={:?} outcome={:?}",
                    context,
                    err,
                    outcome,
                );
                match harvest_ready(epoll, maxevents) {
                    Ok(Some(harvest)) => return EpollWaitResult::Ready(harvest),
                    Ok(None) => {
                        return EpollWaitResult::Terminal(IomuxWaitOutcome::Error(err));
                    },
                    Err(final_err) => {
                        return EpollWaitResult::Terminal(IomuxWaitOutcome::Error(final_err));
                    },
                }
            },
        };

        if register_ready || register_recheck {
            // PredicateReady is only the established cancellation carrier for
            // SubscribedRecheck. The recheck branch is not counted as an event;
            // it retires this round before the final harvest and never parks.
            round.cancel(LatchCancelReason::PredicateReady);
        } else {
            let _ = round.schedule_with_timeout(remaining);
        }
        let outcome = round.finish();

        match harvest_ready(epoll, maxevents) {
            Ok(Some(harvest)) => return EpollWaitResult::Ready(harvest),
            Ok(None) => {},
            Err(err) => return EpollWaitResult::Terminal(IomuxWaitOutcome::Error(err)),
        }
        if let Some(terminal) = terminal_after_empty(outcome, register_ready || register_recheck) {
            return EpollWaitResult::Terminal(terminal);
        }
    }
}

fn read_sigmask(
    task: &Task,
    sigmask_addr: Option<VirtAddr>,
    sigsetsize: usize,
) -> Result<Option<SigSet>, SysError> {
    let Some(sigmask_addr) = sigmask_addr else {
        return Ok(None);
    };
    if sigsetsize != size_of::<LinuxSigSet>() {
        knoticeln!("epoll_pwait: invalid sigsetsize {}", sigsetsize);
        return Err(SysError::InvalidArgument);
    }

    let usp_handle = task.clone_uspace_handle();
    let mut usp = usp_handle.lock();
    let mut mask = SigSet::new_with_mask(
        UserReadPtr::<LinuxSigSet>::try_new(sigmask_addr, &mut usp)?
            .read()?
            .bits,
    );
    mask.clear(SigNo::SIGKILL);
    mask.clear(SigNo::SIGSTOP);
    Ok(Some(mask))
}

fn linux_event_bytes(events: PollEvent, data: u64, dst: &mut [u8]) {
    assert_eq!(dst.len(), size_of::<EpollEvent>());
    let mut linux_events = 0u32;
    if events.contains(PollEvent::READABLE) {
        linux_events |= EPOLLIN;
    }
    if events.contains(PollEvent::WRITABLE) {
        linux_events |= EPOLLOUT;
    }
    if events.contains(PollEvent::ERROR) {
        linux_events |= EPOLLERR;
    }
    if events.contains(PollEvent::HANG_UP) {
        linux_events |= EPOLLHUP;
    }
    dst.fill(0);
    dst[0..4].copy_from_slice(&linux_events.to_ne_bytes());
    dst[8..16].copy_from_slice(&data.to_ne_bytes());
}

fn run_epoll_wait(
    context: &'static str,
    epfd: Fd,
    events_addr: Option<VirtAddr>,
    maxevents: i32,
    timeout: Option<Duration>,
    sigmask_addr: Option<VirtAddr>,
    sigsetsize: usize,
) -> Result<u64, SysError> {
    if maxevents <= 0 {
        return Err(SysError::InvalidArgument);
    }
    let maxevents = maxevents as usize;
    let event_size = size_of::<EpollEvent>();
    let output_capacity = maxevents
        .checked_mul(event_size)
        .ok_or(SysError::InvalidArgument)?;
    let events_addr = events_addr.ok_or(SysError::BadAddress)?;

    let task = get_current_task();
    {
        // Validate the complete Linux-requested output range before claiming
        // ET obligations. Copyout revalidates its actual prefix; any later
        // fault still drops the uncommitted harvest and restores every claim.
        let usp_handle = task.clone_uspace_handle();
        let mut usp = usp_handle.lock();
        let mut events = UserWriteSlice::<u8>::try_new(events_addr, output_capacity, &mut usp)?;
        events.fault_in()?;
    }
    let sigmask = read_sigmask(&task, sigmask_addr, sigsetsize)?;
    let (epoll_file, epoll) = resolve_epoll_fd(&task, epfd)?;
    let token = sigmask.map(|mask| task.begin_temporary_sig_mask(mask));

    match wait_for_harvest(
        context,
        &task,
        epoll.as_ref(),
        epoll_file.as_ref(),
        maxevents,
        timeout,
    ) {
        EpollWaitResult::Ready(harvest) => {
            if let Some(token) = token {
                token.restore_now();
            }

            let count = harvest.events().len();
            let byte_len = count
                .checked_mul(event_size)
                .ok_or(SysError::InvalidArgument)?;
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(byte_len)
                .map_err(|_| SysError::OutOfMemory)?;
            bytes.resize(byte_len, 0);
            for (event, dst) in harvest
                .events()
                .iter()
                .zip(bytes.chunks_exact_mut(event_size))
            {
                linux_event_bytes(event.events(), event.user_data(), dst);
            }

            {
                let usp_handle = task.clone_uspace_handle();
                let mut usp = usp_handle.lock();
                UserWriteSlice::<u8>::try_new(events_addr, byte_len, &mut usp)?
                    .copy_from_slice(&bytes)?;
            }
            harvest.commit();
            Ok(count as u64)
        },
        EpollWaitResult::Terminal(outcome) => {
            let result = match token {
                Some(token) => finish_temporary_iomux_wait(
                    context,
                    &task,
                    token,
                    outcome,
                    TemporaryMaskWaitContext::EpollPwait,
                ),
                None => outcome.into_result_without_temporary_mask(),
            }?;
            assert_eq!(result, 0, "terminal epoll wait returned a ready count");
            Ok(0)
        },
    }
}

#[syscall(SYS_EPOLL_PWAIT)]
fn sys_epoll_pwait(
    epfd: Fd,
    #[validate_with(user_addr.nullable())] events_addr: Option<VirtAddr>,
    maxevents: i32,
    timeout_ms: i32,
    #[validate_with(user_addr.nullable())] sigmask_addr: Option<VirtAddr>,
    sigsetsize: usize,
) -> Result<u64, SysError> {
    let timeout = (timeout_ms >= 0).then(|| Duration::from_millis(timeout_ms as u64));
    run_epoll_wait(
        "sys_epoll_pwait",
        epfd,
        events_addr,
        maxevents,
        timeout,
        sigmask_addr,
        sigsetsize,
    )
}

#[syscall(SYS_EPOLL_PWAIT2)]
fn sys_epoll_pwait2(
    epfd: Fd,
    #[validate_with(user_addr.nullable())] events_addr: Option<VirtAddr>,
    maxevents: i32,
    #[validate_with(user_addr.nullable())] timeout_addr: Option<VirtAddr>,
    #[validate_with(user_addr.nullable())] sigmask_addr: Option<VirtAddr>,
    sigsetsize: usize,
) -> Result<u64, SysError> {
    let timeout = timeout_addr
        .map(|timeout_addr| {
            let task = get_current_task();
            let usp_handle = task.clone_uspace_handle();
            let mut usp = usp_handle.lock();
            let TimeSpec { tv_sec, tv_nsec } =
                UserReadPtr::<TimeSpec>::try_new(timeout_addr, &mut usp)?.read()?;
            if tv_sec < 0 || tv_nsec < 0 || tv_nsec >= 1_000_000_000 {
                return Err(SysError::InvalidArgument);
            }
            Ok(Duration::new(tv_sec as u64, tv_nsec as u32))
        })
        .transpose()?;

    run_epoll_wait(
        "sys_epoll_pwait2",
        epfd,
        events_addr,
        maxevents,
        timeout,
        sigmask_addr,
        sigsetsize,
    )
}
