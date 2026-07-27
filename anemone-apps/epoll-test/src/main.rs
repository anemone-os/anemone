#![no_std]
#![no_main]

use core::{
    mem::offset_of,
    sync::atomic::{AtomicUsize, Ordering},
};

use anemone_rs::{
    abi::{
        fs::linux::{
            epoll::{
                EPOLL_CTL_ADD, EPOLL_CTL_DEL, EPOLL_CTL_MOD, EPOLLERR, EPOLLET, EPOLLEXCLUSIVE,
                EPOLLHUP, EPOLLIN, EPOLLONESHOT, EPOLLOUT, EPOLLPRI, EPOLLRDHUP, EPOLLWAKEUP,
                EpollEvent,
            },
            open::O_RDONLY,
            poll::{POLLIN, PollFd},
        },
        process::linux::signal as linux_signal,
        time::linux::TimeSpec,
    },
    os::linux::{
        fs::{
            AtFd, EpollCreateFlags, EpollCtlOp, Fd, PipeFlags, close, dup, epoll_create,
            epoll_create1, epoll_create1_raw, epoll_ctl, epoll_ctl_raw, epoll_pwait,
            epoll_pwait_raw, epoll_pwait2, epoll_pwait2_raw, epoll_wait, fcntl_getfd, openat,
            pipe2, ppoll, read, write,
        },
        process::{
            WStatus, WStatusRaw, WaitFor, WaitOptions, exit, fork, getpid, sched_yield,
            signal::{SigNo, SigProcMaskHow, kill, sigaction, sigprocmask},
            wait4,
        },
    },
    prelude::*,
};

const ZERO_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 0,
};
const SHORT_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 2_000_000,
};
const RACE_ROUNDS: usize = 16;

static USR1_COUNT: AtomicUsize = AtomicUsize::new(0);

#[anemone_rs::signal_handler]
fn usr1_handler(_: SigNo) {
    USR1_COUNT.fetch_add(1, Ordering::SeqCst);
}

fn ensure(condition: bool) -> Result<(), Errno> {
    if condition { Ok(()) } else { Err(EIO) }
}

fn expect_errno<T>(result: Result<T, Errno>, expected: Errno) -> Result<(), Errno> {
    match result {
        Err(actual) if actual == expected => Ok(()),
        _ => Err(EIO),
    }
}

fn event(events: u32, data: u64) -> EpollEvent {
    EpollEvent::new(events, data)
}

fn wait_one(epfd: Fd, timeout_ms: i32) -> Result<EpollEvent, Errno> {
    let mut events = [EpollEvent::default(); 1];
    ensure(epoll_wait(epfd, &mut events, timeout_ms)? == 1)?;
    Ok(events[0])
}

fn wait_empty(epfd: Fd) -> Result<(), Errno> {
    let mut events = [EpollEvent::default(); 1];
    ensure(epoll_wait(epfd, &mut events, 0)? == 0)
}

fn add(epfd: Fd, fd: Fd, events: u32, data: u64) -> Result<(), Errno> {
    epoll_ctl(epfd, EpollCtlOp::Add, fd, Some(&event(events, data)))
}

fn modify(epfd: Fd, fd: Fd, events: u32, data: u64) -> Result<(), Errno> {
    epoll_ctl(epfd, EpollCtlOp::Modify, fd, Some(&event(events, data)))
}

fn delete(epfd: Fd, fd: Fd) -> Result<(), Errno> {
    epoll_ctl(epfd, EpollCtlOp::Delete, fd, None)
}

fn close_all(fds: &[Fd]) {
    for &fd in fds {
        let _ = close(fd);
    }
}

fn wait_child(pid: u32) -> Result<(), Errno> {
    let mut status = WStatusRaw::EMPTY;
    ensure(
        wait4(
            WaitFor::ChildWithTgid(pid),
            Some(&mut status),
            WaitOptions::empty(),
        )? == Some(pid),
    )?;
    ensure(matches!(status.read(), WStatus::Exited(0)))
}

fn unaligned_event_bytes(events: u32, data: u64) -> [u8; core::mem::size_of::<EpollEvent>() + 1] {
    let event = EpollEvent::new(events, data);
    let mut bytes = [0u8; core::mem::size_of::<EpollEvent>() + 1];
    let event_bytes = unsafe {
        core::slice::from_raw_parts(
            (&event as *const EpollEvent).cast::<u8>(),
            core::mem::size_of::<EpollEvent>(),
        )
    };
    bytes[1..].copy_from_slice(event_bytes);
    bytes
}

fn test_abi_and_create() -> Result<(), Errno> {
    ensure(core::mem::size_of::<EpollEvent>() == 16)?;
    ensure(offset_of!(EpollEvent, events) == 0)?;
    ensure(offset_of!(EpollEvent, data) == 8)?;
    expect_errno(epoll_create(0), EINVAL)?;
    expect_errno(unsafe { epoll_create1_raw(0x4000_0000) }, EINVAL)?;

    let epfd = epoll_create1(EpollCreateFlags::CLOEXEC)?;
    ensure(fcntl_getfd(epfd)? == 1)?;
    close(epfd)
}

fn test_ctl_errno_and_unaligned() -> Result<(), Errno> {
    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    let (rx, tx) = pipe2(PipeFlags::empty())?;
    let mut raw = unaligned_event_bytes(EPOLLIN, 0x1122_3344_5566_7788);
    unsafe {
        epoll_ctl_raw(
            epfd as i32,
            EPOLL_CTL_ADD,
            rx as i32,
            raw.as_mut_ptr().add(1),
        )
    }?;
    write(tx, b"a")?;
    let got = wait_one(epfd, 0)?;
    ensure(got.events & EPOLLIN != 0 && got.data == 0x1122_3344_5566_7788)?;

    raw = unaligned_event_bytes(EPOLLIN | EPOLLONESHOT, 0x8877_6655_4433_2211);
    unsafe {
        epoll_ctl_raw(
            epfd as i32,
            EPOLL_CTL_MOD,
            rx as i32,
            raw.as_mut_ptr().add(1),
        )
    }?;
    let got = wait_one(epfd, 0)?;
    ensure(got.data == 0x8877_6655_4433_2211)?;

    expect_errno(
        unsafe { epoll_ctl_raw(epfd as i32, EPOLL_CTL_ADD, rx as i32, 1usize as *const u8) },
        EFAULT,
    )?;
    expect_errno(
        unsafe { epoll_ctl_raw(-1, EPOLL_CTL_ADD, rx as i32, raw.as_ptr()) },
        EBADF,
    )?;
    expect_errno(
        unsafe { epoll_ctl_raw(epfd as i32, EPOLL_CTL_ADD, -1, raw.as_ptr()) },
        EBADF,
    )?;
    expect_errno(
        unsafe { epoll_ctl_raw(epfd as i32, 99, rx as i32, core::ptr::null()) },
        EINVAL,
    )?;
    expect_errno(
        epoll_ctl(epfd, EpollCtlOp::Add, rx, Some(&event(EPOLLIN, 1))),
        EEXIST,
    )?;
    unsafe {
        epoll_ctl_raw(
            epfd as i32,
            EPOLL_CTL_DEL,
            rx as i32,
            usize::MAX as *const u8,
        )
    }?;
    expect_errno(modify(epfd, rx, EPOLLIN, 1), ENOENT)?;
    expect_errno(delete(epfd, rx), ENOENT)?;

    expect_errno(add(epfd, epfd, EPOLLIN, 1), EINVAL)?;
    let nested = epoll_create1(EpollCreateFlags::empty())?;
    expect_errno(add(epfd, nested, EPOLLIN, 1), EINVAL)?;

    let regular = openat(AtFd::Cwd, Path::new("/bin/epoll-test"), O_RDONLY, 0)?;
    expect_errno(add(epfd, regular, EPOLLIN, 1), EPERM)?;

    add(epfd, rx, EPOLLPRI | EPOLLRDHUP | EPOLLWAKEUP, 2)?;
    delete(epfd, rx)?;
    expect_errno(add(epfd, rx, EPOLLEXCLUSIVE, 2), EINVAL)?;
    expect_errno(add(epfd, rx, 0x0800_0000, 2), EINVAL)?;

    close_all(&[regular, nested, tx, rx, epfd]);
    Ok(())
}

fn test_wait_validation_and_rollback() -> Result<(), Errno> {
    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    let (rx, tx) = pipe2(PipeFlags::empty())?;
    add(epfd, rx, EPOLLIN | EPOLLONESHOT, 0x55)?;
    write(tx, b"x")?;
    let mut out = [EpollEvent::default(); 1];
    expect_errno(
        unsafe {
            epoll_pwait_raw(
                epfd as i32,
                out.as_mut_ptr().cast(),
                0,
                0,
                core::ptr::null(),
                0,
            )
        },
        EINVAL,
    )?;
    expect_errno(
        unsafe { epoll_pwait_raw(epfd as i32, 1usize as *mut u8, 1, 0, core::ptr::null(), 0) },
        EFAULT,
    )?;
    let got = wait_one(epfd, 0)?;
    ensure(got.data == 0x55)?;
    wait_empty(epfd)?;

    let mut bytes = [0u8; core::mem::size_of::<EpollEvent>() + 1];
    modify(epfd, rx, EPOLLIN, 0x66)?;
    ensure(
        unsafe {
            epoll_pwait_raw(
                epfd as i32,
                bytes.as_mut_ptr().add(1),
                1,
                0,
                core::ptr::null(),
                0,
            )
        }? == 1,
    )?;
    let got = unsafe { core::ptr::read_unaligned(bytes.as_ptr().add(1).cast::<EpollEvent>()) };
    ensure(got.events & EPOLLIN != 0 && got.data == 0x66)?;
    close_all(&[tx, rx, epfd]);
    Ok(())
}

fn test_lt_et_oneshot() -> Result<(), Errno> {
    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    let (rx, tx) = pipe2(PipeFlags::empty())?;
    write(tx, b"a")?;
    add(epfd, rx, EPOLLIN, 1)?;
    ensure(wait_one(epfd, 0)?.data == 1)?;
    ensure(wait_one(epfd, 0)?.data == 1)?;
    let mut byte = [0u8; 1];
    ensure(read(rx, &mut byte)? == 1)?;
    wait_empty(epfd)?;
    delete(epfd, rx)?;

    add(epfd, rx, EPOLLIN | EPOLLET, 2)?;
    write(tx, b"b")?;
    ensure(wait_one(epfd, 0)?.data == 2)?;
    wait_empty(epfd)?;
    ensure(read(rx, &mut byte)? == 1)?;
    write(tx, b"c")?;
    ensure(wait_one(epfd, 0)?.data == 2)?;
    delete(epfd, rx)?;

    add(epfd, rx, EPOLLIN | EPOLLONESHOT, 3)?;
    ensure(wait_one(epfd, 0)?.data == 3)?;
    wait_empty(epfd)?;
    modify(epfd, rx, EPOLLIN | EPOLLONESHOT, 4)?;
    ensure(wait_one(epfd, 0)?.data == 4)?;
    close_all(&[tx, rx, epfd]);
    Ok(())
}

fn test_hup_err_coalescing_and_fairness() -> Result<(), Errno> {
    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    let (out_rx, out_tx) = pipe2(PipeFlags::empty())?;
    add(epfd, out_tx, EPOLLOUT, 9)?;
    let got = wait_one(epfd, 0)?;
    ensure(got.data == 9 && got.events & EPOLLOUT != 0)?;
    delete(epfd, out_tx)?;
    close_all(&[out_tx, out_rx]);

    let (hup_rx, hup_tx) = pipe2(PipeFlags::empty())?;
    add(epfd, hup_rx, 0, 10)?;
    close(hup_tx)?;
    let got = wait_one(epfd, 0)?;
    ensure(got.data == 10 && got.events & EPOLLHUP != 0 && got.events & EPOLLIN == 0)?;
    delete(epfd, hup_rx)?;
    close(hup_rx)?;

    let (err_rx, err_tx) = pipe2(PipeFlags::empty())?;
    add(epfd, err_tx, 0, 11)?;
    close(err_rx)?;
    let got = wait_one(epfd, 0)?;
    ensure(got.data == 11 && got.events & EPOLLERR != 0 && got.events & EPOLLOUT == 0)?;
    delete(epfd, err_tx)?;
    close(err_tx)?;

    let (rx, tx) = pipe2(PipeFlags::empty())?;
    add(epfd, rx, EPOLLIN, 12)?;
    write(tx, b"abc")?;
    ensure(wait_one(epfd, 0)?.data == 12)?;
    let mut batch = [EpollEvent::default(); 4];
    ensure(epoll_wait(epfd, &mut batch, 0)? == 1)?;
    delete(epfd, rx)?;
    close_all(&[tx, rx]);

    let mut reads = [0u32; 4];
    let mut writes = [0u32; 4];
    for index in 0..4 {
        let (read_fd, write_fd) = pipe2(PipeFlags::empty())?;
        reads[index] = read_fd;
        writes[index] = write_fd;
        add(epfd, read_fd, EPOLLIN, index as u64)?;
        write(write_fd, b"f")?;
    }
    let mut seen = 0u8;
    for _ in 0..4 {
        let got = wait_one(epfd, 0)?;
        ensure(got.data < 4)?;
        seen |= 1 << got.data;
    }
    ensure(seen == 0b1111)?;
    for read_fd in reads {
        delete(epfd, read_fd)?;
    }
    close_all(&reads);
    close_all(&writes);
    close(epfd)
}

fn test_lifecycle_and_epoll_file_poll() -> Result<(), Errno> {
    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    let epfd_alias = dup(epfd)?;
    close(epfd)?;
    let (rx, tx) = pipe2(PipeFlags::empty())?;
    let rx_alias = dup(rx)?;
    add(epfd_alias, rx, EPOLLIN, 20)?;
    close(rx)?;
    write(tx, b"x")?;
    ensure(wait_one(epfd_alias, 0)?.data == 20)?;

    let mut pfd = [PollFd {
        fd: epfd_alias as i32,
        events: POLLIN,
        revents: 0,
    }];
    ensure(ppoll(&mut pfd, Some(&ZERO_TIMEOUT))? == 1 && pfd[0].revents & POLLIN != 0)?;
    let mut byte = [0u8; 1];
    read(rx_alias, &mut byte)?;
    pfd[0].revents = 0;
    ensure(ppoll(&mut pfd, Some(&ZERO_TIMEOUT))? == 0 && pfd[0].revents == 0)?;

    close_all(&[tx, rx_alias]);
    let old_fd = rx;
    let (new_rx, new_tx) = pipe2(PipeFlags::empty())?;
    ensure(new_rx == old_fd)?;
    add(epfd_alias, new_rx, EPOLLIN, 21)?;
    write(new_tx, b"y")?;
    ensure(wait_one(epfd_alias, 0)?.data == 21)?;

    write(new_tx, b"z")?;
    delete(epfd_alias, new_rx)?;
    wait_empty(epfd_alias)?;
    add(epfd_alias, new_rx, EPOLLIN, 22)?;
    modify(epfd_alias, new_rx, EPOLLIN, 23)?;
    ensure(wait_one(epfd_alias, 0)?.data == 23)?;

    close(epfd_alias)?;
    close_all(&[new_tx, new_rx]);

    let teardown_epfd = epoll_create1(EpollCreateFlags::empty())?;
    let (teardown_rx, teardown_tx) = pipe2(PipeFlags::empty())?;
    add(teardown_epfd, teardown_rx, EPOLLIN, 24)?;
    close(teardown_epfd)?;
    write(teardown_tx, b"q")?;
    close_all(&[teardown_tx, teardown_rx]);
    Ok(())
}

fn run_transition_race() -> Result<(), Errno> {
    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    let (data_rx, data_tx) = pipe2(PipeFlags::empty())?;
    let (ack_rx, ack_tx) = pipe2(PipeFlags::empty())?;
    add(epfd, data_rx, EPOLLIN | EPOLLET, 31)?;

    match fork()? {
        None => {
            close_all(&[data_rx, ack_tx, epfd]);
            let mut ack = [0u8; 1];
            for _ in 0..RACE_ROUNDS {
                for _ in 0..8 {
                    let _ = sched_yield();
                }
                if write(data_tx, b"r") != Ok(1) || read(ack_rx, &mut ack) != Ok(1) {
                    exit(1);
                }
            }
            close_all(&[ack_rx, data_tx]);
            exit(0)
        },
        Some(pid) => {
            close_all(&[data_tx, ack_rx]);
            let mut byte = [0u8; 1];
            for _ in 0..RACE_ROUNDS {
                let got = wait_one(epfd, 1000)?;
                ensure(got.data == 31 && got.events & EPOLLIN != 0)?;
                ensure(read(data_rx, &mut byte)? == 1)?;
                ensure(write(ack_tx, b"a")? == 1)?;
            }
            wait_child(pid)?;
            close_all(&[ack_tx, data_rx, epfd]);
            Ok(())
        },
    }
}

fn test_wide_harvest_producer_race() -> Result<(), Errno> {
    const FILLER_COUNT: usize = 15;
    const PRODUCER_WRITES: usize = 128;
    const RACE_DATA: u64 = 0xfeed;

    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    let (data_rx, data_tx) = pipe2(PipeFlags::NONBLOCK)?;
    let (ack_rx, ack_tx) = pipe2(PipeFlags::empty())?;
    add(epfd, data_rx, EPOLLIN | EPOLLET, RACE_DATA)?;

    let mut filler_rx = [0u32; FILLER_COUNT];
    let mut filler_tx = [0u32; FILLER_COUNT];
    for index in 0..FILLER_COUNT {
        let (rx, tx) = pipe2(PipeFlags::empty())?;
        filler_rx[index] = rx;
        filler_tx[index] = tx;
        add(epfd, rx, EPOLLIN, 0x1000 + index as u64)?;
        write(tx, b"f")?;
    }

    let child = match fork()? {
        None => {
            close_all(&[epfd, data_rx, ack_tx]);
            close_all(&filler_rx);
            close_all(&filler_tx);
            for _ in 0..PRODUCER_WRITES {
                if write(data_tx, b"r") != Ok(1) {
                    exit(1);
                }
                let _ = sched_yield();
            }
            let mut ack = [0u8; 1];
            if read(ack_rx, &mut ack) != Ok(1) {
                exit(1);
            }
            close_all(&[ack_rx, data_tx]);
            exit(0)
        },
        Some(pid) => pid,
    };
    close_all(&[data_tx, ack_rx]);

    let mut deliveries = 0usize;
    let mut child_done = false;
    for _ in 0..1024 {
        let mut events = [EpollEvent::default(); FILLER_COUNT + 1];
        let ready = epoll_wait(epfd, &mut events, 10)?;
        for event in &events[..ready] {
            if event.data != RACE_DATA {
                continue;
            }
            let mut byte = [0u8; 1];
            if read(data_rx, &mut byte)? == 1 {
                deliveries += 1;
                if deliveries == 2 {
                    write(ack_tx, b"a")?;
                    close(ack_tx)?;
                }
            }
        }

        if !child_done {
            let mut status = WStatusRaw::EMPTY;
            if wait4(
                WaitFor::ChildWithTgid(child),
                Some(&mut status),
                WaitOptions::NOHANG,
            )? == Some(child)
            {
                ensure(matches!(status.read(), WStatus::Exited(0)))?;
                child_done = true;
            }
        }
        if child_done && deliveries >= 2 {
            break;
        }
        sched_yield()?;
    }
    ensure(child_done && deliveries >= 2)?;

    close_all(&filler_tx);
    close_all(&filler_rx);
    close_all(&[data_rx, epfd]);
    Ok(())
}

fn test_ctl_callback_race() -> Result<(), Errno> {
    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    let (data_rx, data_tx) = pipe2(PipeFlags::empty())?;
    let (go_rx, go_tx) = pipe2(PipeFlags::empty())?;
    let (ack_rx, ack_tx) = pipe2(PipeFlags::empty())?;
    add(epfd, data_rx, EPOLLIN | EPOLLET, 0x2000)?;

    let child = match fork()? {
        None => {
            close_all(&[epfd, data_rx, go_tx, ack_rx]);
            let mut byte = [0u8; 1];
            for _ in 0..RACE_ROUNDS {
                if read(go_rx, &mut byte) != Ok(1)
                    || write(data_tx, b"d") != Ok(1)
                    || write(ack_tx, b"a") != Ok(1)
                {
                    exit(1);
                }
            }
            close_all(&[ack_tx, go_rx, data_tx]);
            exit(0)
        },
        Some(pid) => pid,
    };
    close_all(&[data_tx, go_rx, ack_tx]);

    let mut byte = [0u8; 1];
    for round in 0..RACE_ROUNDS {
        write(go_tx, b"g")?;
        if round % 4 >= 2 {
            sched_yield()?;
        }

        if round % 2 == 0 {
            let replacement_data = 0x3000 + round as u64;
            modify(epfd, data_rx, EPOLLIN | EPOLLET, replacement_data)?;
            ensure(read(ack_rx, &mut byte)? == 1)?;
            let got = wait_one(epfd, 1000)?;
            ensure(got.data == replacement_data && got.events & EPOLLIN != 0)?;
        } else {
            delete(epfd, data_rx)?;
            ensure(read(ack_rx, &mut byte)? == 1)?;
            wait_empty(epfd)?;
        }
        ensure(read(data_rx, &mut byte)? == 1)?;
        if round % 2 != 0 {
            add(epfd, data_rx, EPOLLIN | EPOLLET, 0x2000 + round as u64)?;
        }
    }
    wait_child(child)?;
    close_all(&[ack_rx, go_tx, data_rx, epfd]);
    Ok(())
}

fn test_multiple_waiters() -> Result<(), Errno> {
    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    let (rx, tx) = pipe2(PipeFlags::empty())?;
    add(epfd, rx, EPOLLIN, 32)?;

    let spawn_waiter = || -> Result<Option<u32>, Errno> {
        match fork()? {
            None => {
                let result = wait_one(epfd, 1000)
                    .and_then(|got| ensure(got.data == 32 && got.events & EPOLLIN != 0));
                exit(if result.is_ok() { 0 } else { 1 })
            },
            Some(pid) => Ok(Some(pid)),
        }
    };
    let first = spawn_waiter()?.ok_or(EIO)?;
    let second = spawn_waiter()?.ok_or(EIO)?;
    for _ in 0..32 {
        sched_yield()?;
    }
    write(tx, b"m")?;
    wait_child(first)?;
    wait_child(second)?;
    close_all(&[tx, rx, epfd]);
    Ok(())
}

fn test_pwait_and_pwait2() -> Result<(), Errno> {
    let epfd = epoll_create1(EpollCreateFlags::empty())?;
    let mut events = [EpollEvent::default(); 1];
    ensure(epoll_pwait(epfd, &mut events, 0, None)? == 0)?;
    ensure(epoll_pwait(epfd, &mut events, 2, None)? == 0)?;
    ensure(epoll_pwait2(epfd, &mut events, Some(&SHORT_TIMEOUT), None)? == 0)?;

    let invalid = TimeSpec {
        tv_sec: -1,
        tv_nsec: 0,
    };
    expect_errno(
        epoll_pwait2(epfd, &mut events, Some(&invalid), None),
        EINVAL,
    )?;
    let invalid = TimeSpec {
        tv_sec: 0,
        tv_nsec: 1_000_000_000,
    };
    expect_errno(
        epoll_pwait2(epfd, &mut events, Some(&invalid), None),
        EINVAL,
    )?;

    let empty_mask = linux_signal::SigSet { bits: 0 };
    expect_errno(
        unsafe {
            epoll_pwait_raw(
                epfd as i32,
                events.as_mut_ptr().cast(),
                1,
                0,
                &empty_mask,
                7,
            )
        },
        EINVAL,
    )?;
    ensure(
        unsafe {
            epoll_pwait_raw(
                epfd as i32,
                events.as_mut_ptr().cast(),
                1,
                0,
                &empty_mask,
                core::mem::size_of::<linux_signal::SigSet>(),
            )
        }? == 0,
    )?;
    expect_errno(
        unsafe {
            epoll_pwait2_raw(
                epfd as i32,
                events.as_mut_ptr().cast(),
                1,
                &ZERO_TIMEOUT,
                &empty_mask,
                7,
            )
        },
        EINVAL,
    )?;

    let action = linux_signal::SigAction {
        sighandler: usr1_handler as *const (),
        sa_flags: 0,
        sa_restorer: core::ptr::null(),
        sa_mask: empty_mask,
    };
    sigaction(SigNo::SIGUSR1, Some(&action), None)?;
    let usr1_mask = linux_signal::SigSet {
        bits: 1u64 << (SigNo::SIGUSR1.as_usize() - 1),
    };
    let mut old_mask = empty_mask;
    sigprocmask(SigProcMaskHow::Block, Some(&usr1_mask), Some(&mut old_mask))?;
    let parent = getpid()?;
    let child = match fork()? {
        None => {
            for _ in 0..64 {
                let _ = sched_yield();
            }
            let result = kill(parent as i32, SigNo::SIGUSR1);
            exit(if result.is_ok() { 0 } else { 1 })
        },
        Some(pid) => pid,
    };
    expect_errno(epoll_pwait(epfd, &mut events, -1, Some(&empty_mask)), EINTR)?;
    wait_child(child)?;
    ensure(USR1_COUNT.load(Ordering::SeqCst) == 1)?;
    let mut current_mask = empty_mask;
    sigprocmask(SigProcMaskHow::SetMask, None, Some(&mut current_mask))?;
    ensure(current_mask.bits & usr1_mask.bits != 0)?;
    sigprocmask(SigProcMaskHow::SetMask, Some(&old_mask), None)?;

    let (rx, tx) = pipe2(PipeFlags::empty())?;
    add(epfd, rx, EPOLLIN, 40)?;
    write(tx, b"p")?;
    let huge_timeout = TimeSpec {
        tv_sec: i64::MAX,
        tv_nsec: 0,
    };
    ensure(epoll_pwait2(epfd, &mut events, Some(&huge_timeout), None)? == 1)?;
    ensure(events[0].data == 40 && events[0].events & EPOLLIN != 0)?;
    let mut byte = [0u8; 1];
    read(rx, &mut byte)?;
    wait_empty(epfd)?;
    write(tx, b"q")?;
    ensure(epoll_pwait2(epfd, &mut events, None, None)? == 1)?;
    ensure(events[0].data == 40 && events[0].events & EPOLLIN != 0)?;
    close_all(&[tx, rx, epfd]);
    Ok(())
}

struct Results {
    passed: usize,
    failed: usize,
}

impl Results {
    const fn new() -> Self {
        Self {
            passed: 0,
            failed: 0,
        }
    }

    fn case(&mut self, name: &str, test: fn() -> Result<(), Errno>) {
        match test() {
            Ok(()) => {
                self.passed += 1;
                println!("EPOLLTEST:PASS:{name}");
            },
            Err(errno) => {
                self.failed += 1;
                println!("EPOLLTEST:FAIL:{name}:{errno}");
            },
        }
    }
}

#[anemone_rs::main]
fn main() -> Result<(), Errno> {
    println!("EPOLLTEST:START");
    let mut results = Results::new();
    results.case("abi-create", test_abi_and_create);
    results.case("ctl-errno-unaligned", test_ctl_errno_and_unaligned);
    results.case("wait-copyout-rollback", test_wait_validation_and_rollback);
    results.case("lt-et-oneshot", test_lt_et_oneshot);
    results.case(
        "hup-err-coalesce-fair",
        test_hup_err_coalescing_and_fairness,
    );
    results.case("lifecycle-epoll-file", test_lifecycle_and_epoll_file_poll);
    results.case("producer-transition-race", run_transition_race);
    results.case(
        "wide-harvest-producer-race",
        test_wide_harvest_producer_race,
    );
    results.case("ctl-callback-race", test_ctl_callback_race);
    results.case("multiple-waiters", test_multiple_waiters);
    results.case("pwait-pwait2", test_pwait_and_pwait2);

    if results.failed == 0 {
        println!("EPOLLTEST:SUMMARY:PASS:{}", results.passed);
        Ok(())
    } else {
        println!(
            "EPOLLTEST:SUMMARY:FAIL:passed={}:failed={}",
            results.passed, results.failed
        );
        Err(EIO)
    }
}
