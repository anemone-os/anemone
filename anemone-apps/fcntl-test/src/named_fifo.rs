use core::sync::atomic::{AtomicUsize, Ordering};

use anemone_rs::{
    abi::{
        fs::linux::{
            mode::S_IFIFO,
            open::{O_NONBLOCK, O_PATH, O_RDONLY, O_RDWR, O_WRONLY},
            poll::{POLLERR, POLLHUP, POLLIN, POLLOUT, PollFd},
        },
        process::linux::signal::{SigAction, SigSet},
        time::linux::TimeSpec,
    },
    os::linux::{
        fs::{
            AtFd, Fd, PipeFlags, close, dup, fcntl_get_pipe_size, fcntl_set_pipe_size,
            ioctl_readable_bytes, linkat, mkdirat, mknodat, mount, openat, pipe2, ppoll, read,
            renameat2, umount, unlinkat, write,
        },
        process::{
            WStatus, WStatusRaw, WaitFor, WaitOptions, exit, fork, sched_yield,
            signal::{SigNo, kill, sigaction},
            wait4,
        },
    },
    prelude::*,
};

const MODE: u32 = 0o666;
const PAGE_SIZE: usize = 4096;
const DEFAULT_CAPACITY: usize = 2 * PAGE_SIZE;
const ZERO_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 0,
};
const SETTLE_YIELDS: usize = 128;

static SIGNAL_COUNT: AtomicUsize = AtomicUsize::new(0);

#[anemone_rs::signal_handler]
fn signal_handler(_: SigNo) {
    SIGNAL_COUNT.fetch_add(1, Ordering::SeqCst);
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

fn remove(path: &Path) {
    let _ = unlinkat(AtFd::Cwd, path, 0);
}

fn create_fifo(path: &Path) -> Result<(), Errno> {
    remove(path);
    mknodat(AtFd::Cwd, path, S_IFIFO | MODE, 0)
}

fn open_fifo(path: &Path, flags: u32) -> Result<Fd, Errno> {
    openat(AtFd::Cwd, path, flags, 0)
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

fn child_result(result: Result<(), Errno>) -> ! {
    exit(if result.is_ok() { 0 } else { 1 })
}

fn read_exact(fd: Fd, buf: &mut [u8]) -> Result<(), Errno> {
    let mut offset = 0usize;
    while offset < buf.len() {
        let progressed = read(fd, &mut buf[offset..])?;
        if progressed == 0 {
            return Err(EIO);
        }
        offset += progressed;
    }
    Ok(())
}

fn settle() -> Result<(), Errno> {
    for _ in 0..SETTLE_YIELDS {
        sched_yield()?;
    }
    Ok(())
}

fn install_handler(signal: SigNo, flags: u64) -> Result<(), Errno> {
    let action = SigAction {
        sighandler: signal_handler as *const (),
        sa_flags: flags,
        sa_restorer: core::ptr::null(),
        sa_mask: SigSet { bits: 0 },
    };
    sigaction(signal, Some(&action), None)
}

fn test_nonblocking_duplex_capacity_and_readiness(path: &Path) -> Result<(), Errno> {
    create_fifo(path)?;
    expect_errno(open_fifo(path, O_WRONLY | O_NONBLOCK), ENXIO)?;

    let path_fd = open_fifo(path, O_PATH)?;
    expect_errno(open_fifo(path, O_WRONLY | O_NONBLOCK), ENXIO)?;
    close(path_fd)?;

    let reader = open_fifo(path, O_RDONLY | O_NONBLOCK)?;
    let mut byte = [0u8; 1];
    ensure(read(reader, &mut byte)? == 0)?;
    let mut initial_poll = [PollFd {
        fd: reader as i32,
        events: POLLIN,
        revents: 0,
    }];
    ensure(ppoll(&mut initial_poll, Some(&ZERO_TIMEOUT))? == 0)?;
    ensure(initial_poll[0].revents == 0)?;
    let writer = open_fifo(path, O_WRONLY | O_NONBLOCK)?;
    expect_errno(read(reader, &mut byte), EAGAIN)?;
    ensure(write(writer, b"fifo")? == 4)?;
    ensure(ioctl_readable_bytes(reader)? == 4)?;

    let mut poll = [
        PollFd {
            fd: reader as i32,
            events: POLLIN,
            revents: 0,
        },
        PollFd {
            fd: writer as i32,
            events: POLLOUT,
            revents: 0,
        },
    ];
    ensure(ppoll(&mut poll, Some(&ZERO_TIMEOUT))? == 2)?;
    ensure(poll[0].revents & POLLIN != 0)?;
    ensure(poll[1].revents & POLLOUT != 0)?;
    close(writer)?;
    poll[0].revents = 0;
    ensure(ppoll(&mut poll[..1], Some(&ZERO_TIMEOUT))? == 1)?;
    ensure(poll[0].revents & (POLLIN | POLLHUP) == (POLLIN | POLLHUP))?;
    let mut payload = [0u8; 4];
    ensure(read(reader, &mut payload)? == payload.len())?;
    ensure(&payload == b"fifo")?;
    // Buffered bytes must be delivered before the final writer's retirement
    // becomes EOF for this reader.
    ensure(read(reader, &mut byte)? == 0)?;
    close(reader)?;

    let duplex = open_fifo(path, O_RDWR | O_NONBLOCK)?;
    ensure(fcntl_get_pipe_size(duplex)? == DEFAULT_CAPACITY)?;
    ensure(fcntl_set_pipe_size(duplex, (4 * PAGE_SIZE) as u64)? == 4 * PAGE_SIZE)?;
    ensure(write(duplex, b"self")? == 4)?;
    ensure(read(duplex, &mut payload)? == payload.len())?;
    ensure(&payload == b"self")?;
    close(duplex)?;

    // No endpoint pins the old session. A later open starts with empty bytes
    // and the configured default capacity rather than the resized old value.
    let fresh = open_fifo(path, O_RDONLY | O_NONBLOCK)?;
    ensure(fcntl_get_pipe_size(fresh)? == DEFAULT_CAPACITY)?;
    ensure(ioctl_readable_bytes(fresh)? == 0)?;
    ensure(read(fresh, &mut byte)? == 0)?;
    close(fresh)?;
    remove(path);
    Ok(())
}

fn test_reader_first_and_writer_first(path: &Path) -> Result<(), Errno> {
    create_fifo(path)?;
    match fork()? {
        Some(pid) => {
            settle()?;
            let writer = open_fifo(path, O_WRONLY)?;
            ensure(write(writer, b"r")? == 1)?;
            close(writer)?;
            wait_child(pid)?;
        },
        None => child_result((|| {
            let reader = open_fifo(path, O_RDONLY)?;
            let mut byte = [0u8; 1];
            ensure(read(reader, &mut byte)? == 1)?;
            ensure(byte[0] == b'r')?;
            close(reader)
        })()),
    }

    match fork()? {
        Some(pid) => {
            settle()?;
            let reader = open_fifo(path, O_RDONLY)?;
            let mut byte = [0u8; 1];
            ensure(read(reader, &mut byte)? == 1)?;
            ensure(byte[0] == b'w')?;
            close(reader)?;
            wait_child(pid)?;
        },
        None => child_result((|| {
            let writer = open_fifo(path, O_WRONLY)?;
            ensure(write(writer, b"w")? == 1)?;
            close(writer)
        })()),
    }
    remove(path);
    Ok(())
}

fn test_dup_fork_and_final_close(path: &Path) -> Result<(), Errno> {
    create_fifo(path)?;
    let reader = open_fifo(path, O_RDONLY | O_NONBLOCK)?;
    let writer = open_fifo(path, O_WRONLY | O_NONBLOCK)?;
    let alias = dup(writer)?;
    close(writer)?;
    let mut byte = [0u8; 1];
    expect_errno(read(reader, &mut byte), EAGAIN)?;
    close(alias)?;
    ensure(read(reader, &mut byte)? == 0)?;

    let writer = open_fifo(path, O_WRONLY | O_NONBLOCK)?;
    let (control_rx, control_tx) = pipe2(PipeFlags::empty())?;
    match fork()? {
        Some(pid) => {
            close(control_rx)?;
            close(writer)?;
            expect_errno(read(reader, &mut byte), EAGAIN)?;
            ensure(write(control_tx, b"x")? == 1)?;
            close(control_tx)?;
            wait_child(pid)?;
            ensure(read(reader, &mut byte)? == 0)?;
        },
        None => child_result((|| {
            close(control_tx)?;
            close(reader)?;
            let mut release = [0u8; 1];
            ensure(read(control_rx, &mut release)? == 1)?;
            close(control_rx)?;
            close(writer)
        })()),
    }

    close(reader)?;
    remove(path);
    Ok(())
}

fn test_distinct_readers_consume_each_byte_once(path: &Path) -> Result<(), Errno> {
    create_fifo(path)?;
    let anchor = open_fifo(path, O_RDWR | O_NONBLOCK)?;
    let (ready_rx, ready_tx) = pipe2(PipeFlags::empty())?;
    let (release_rx, release_tx) = pipe2(PipeFlags::empty())?;
    let (result_rx, result_tx) = pipe2(PipeFlags::empty())?;
    let mut children = [0u32; 2];

    for pid_slot in &mut children {
        match fork()? {
            Some(pid) => *pid_slot = pid,
            None => child_result((|| {
                close(ready_rx)?;
                close(release_tx)?;
                close(result_rx)?;
                let reader = open_fifo(path, O_RDONLY)?;
                close(anchor)?;
                ensure(write(ready_tx, b"r")? == 1)?;
                let mut release = [0u8; 1];
                read_exact(release_rx, &mut release)?;
                let mut byte = [0u8; 1];
                read_exact(reader, &mut byte)?;
                ensure(write(result_tx, &byte)? == 1)?;
                close(reader)?;
                close(ready_tx)?;
                close(release_rx)?;
                close(result_tx)
            })()),
        }
    }

    close(ready_tx)?;
    close(release_rx)?;
    close(result_tx)?;
    let mut ready = [0u8; 2];
    read_exact(ready_rx, &mut ready)?;
    ensure(write(anchor, b"ab")? == 2)?;
    ensure(write(release_tx, b"rr")? == 2)?;
    let mut consumed = [0u8; 2];
    read_exact(result_rx, &mut consumed)?;
    ensure(consumed == *b"ab" || consumed == *b"ba")?;
    close(ready_rx)?;
    close(release_tx)?;
    close(result_rx)?;
    close(anchor)?;
    for pid in children {
        wait_child(pid)?;
    }
    remove(path);
    Ok(())
}

fn test_alias_rename_and_unlink_recreate(
    path: &Path,
    alias: &Path,
    renamed: &Path,
) -> Result<(), Errno> {
    remove(alias);
    remove(renamed);
    create_fifo(path)?;
    linkat(AtFd::Cwd, path, AtFd::Cwd, alias, 0)?;

    let reader = open_fifo(path, O_RDONLY | O_NONBLOCK)?;
    let writer = open_fifo(alias, O_WRONLY | O_NONBLOCK)?;
    renameat2(AtFd::Cwd, alias, AtFd::Cwd, renamed, 0)?;
    let second_writer = open_fifo(renamed, O_WRONLY | O_NONBLOCK)?;
    ensure(write(second_writer, b"a")? == 1)?;
    let mut byte = [0u8; 1];
    ensure(read(reader, &mut byte)? == 1 && byte[0] == b'a')?;

    unlinkat(AtFd::Cwd, path, 0)?;
    create_fifo(path)?;
    expect_errno(open_fifo(path, O_WRONLY | O_NONBLOCK), ENXIO)?;
    let new_reader = open_fifo(path, O_RDONLY | O_NONBLOCK)?;
    let new_writer = open_fifo(path, O_WRONLY | O_NONBLOCK)?;
    ensure(write(writer, b"o")? == 1)?;
    ensure(write(new_writer, b"n")? == 1)?;
    ensure(read(reader, &mut byte)? == 1 && byte[0] == b'o')?;
    ensure(read(new_reader, &mut byte)? == 1 && byte[0] == b'n')?;

    close(second_writer)?;
    close(writer)?;
    close(reader)?;
    close(new_writer)?;
    close(new_reader)?;
    remove(path);
    remove(renamed);
    Ok(())
}

fn test_signal_interrupt(path: &Path) -> Result<(), Errno> {
    create_fifo(path)?;
    let (ready_rx, ready_tx) = pipe2(PipeFlags::empty())?;
    match fork()? {
        Some(pid) => {
            close(ready_tx)?;
            let mut byte = [0u8; 1];
            ensure(read(ready_rx, &mut byte)? == 1)?;
            close(ready_rx)?;
            settle()?;
            kill(pid as i32, SigNo::SIGUSR1)?;
            wait_child(pid)?;
            expect_errno(open_fifo(path, O_WRONLY | O_NONBLOCK), ENXIO)?;
        },
        None => child_result((|| {
            close(ready_rx)?;
            SIGNAL_COUNT.store(0, Ordering::SeqCst);
            install_handler(SigNo::SIGUSR1, 0)?;
            ensure(write(ready_tx, b"r")? == 1)?;
            close(ready_tx)?;
            expect_errno(open_fifo(path, O_RDONLY), EINTR)?;
            ensure(SIGNAL_COUNT.load(Ordering::SeqCst) == 1)
        })()),
    }
    remove(path);
    Ok(())
}

fn test_sigpipe(path: &Path) -> Result<(), Errno> {
    create_fifo(path)?;
    install_handler(SigNo::SIGPIPE, 0)?;
    SIGNAL_COUNT.store(0, Ordering::SeqCst);
    let reader = open_fifo(path, O_RDONLY | O_NONBLOCK)?;
    let writer = open_fifo(path, O_WRONLY | O_NONBLOCK)?;
    close(reader)?;
    let mut poll = [PollFd {
        fd: writer as i32,
        events: POLLOUT,
        revents: 0,
    }];
    ensure(ppoll(&mut poll, Some(&ZERO_TIMEOUT))? == 1)?;
    ensure(poll[0].revents & POLLERR != 0)?;
    expect_errno(write(writer, b"x"), EPIPE)?;
    settle()?;
    ensure(SIGNAL_COUNT.load(Ordering::SeqCst) == 1)?;
    close(writer)?;
    remove(path);
    Ok(())
}

fn run_filesystem_cases(path: &Path, alias: &Path, renamed: &Path) -> Result<(), Errno> {
    println!("fcntl-test named-fifo: subcase nonblocking-duplex-capacity-readiness start");
    test_nonblocking_duplex_capacity_and_readiness(path)?;
    println!("fcntl-test named-fifo: subcase nonblocking-duplex-capacity-readiness ok");
    println!("fcntl-test named-fifo: subcase blocking-open-order start");
    test_reader_first_and_writer_first(path)?;
    println!("fcntl-test named-fifo: subcase blocking-open-order ok");
    println!("fcntl-test named-fifo: subcase dup-fork-final-close start");
    test_dup_fork_and_final_close(path)?;
    println!("fcntl-test named-fifo: subcase dup-fork-final-close ok");
    println!("fcntl-test named-fifo: subcase distinct-reader-consumption start");
    test_distinct_readers_consume_each_byte_once(path)?;
    println!("fcntl-test named-fifo: subcase distinct-reader-consumption ok");
    println!("fcntl-test named-fifo: subcase alias-rename-unlink-recreate start");
    test_alias_rename_and_unlink_recreate(path, alias, renamed)?;
    println!("fcntl-test named-fifo: subcase alias-rename-unlink-recreate ok");
    println!("fcntl-test named-fifo: subcase signal-interrupt start");
    test_signal_interrupt(path)?;
    println!("fcntl-test named-fifo: subcase signal-interrupt ok");
    println!("fcntl-test named-fifo: subcase sigpipe start");
    test_sigpipe(path)
}

pub fn run() -> Result<(), Errno> {
    println!("fcntl-test named-fifo: CASE ext4-lifecycle start");
    run_filesystem_cases(
        Path::new("/named-fifo-ext4"),
        Path::new("/named-fifo-ext4-alias"),
        Path::new("/named-fifo-ext4-renamed"),
    )?;
    println!("fcntl-test named-fifo: CASE ext4-lifecycle ok");

    let mountpoint = Path::new("/named-fifo-ramfs");
    match mkdirat(AtFd::Cwd, mountpoint, 0o777) {
        Ok(()) | Err(EEXIST) => {},
        Err(error) => return Err(error),
    }
    mount(Path::new("none"), mountpoint, "ramfs")?;
    println!("fcntl-test named-fifo: CASE ramfs-lifecycle start");
    let result = run_filesystem_cases(
        Path::new("/named-fifo-ramfs/fifo"),
        Path::new("/named-fifo-ramfs/alias"),
        Path::new("/named-fifo-ramfs/renamed"),
    );
    let unmount_result = umount(mountpoint);
    result?;
    unmount_result?;
    println!("fcntl-test named-fifo: CASE ramfs-lifecycle ok");
    println!("fcntl-test named-fifo: all cases passed");
    Ok(())
}
