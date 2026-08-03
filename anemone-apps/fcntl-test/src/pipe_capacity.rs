use anemone_rs::{
    abi::{
        fs::linux::poll::{POLLOUT, PollFd},
        time::linux::TimeSpec,
    },
    os::linux::fs::{
        Fd, PipeFlags, close, fcntl_get_pipe_size, fcntl_set_pipe_size, ioctl_readable_bytes,
        pipe2, ppoll, read, write,
    },
    prelude::*,
};

const PAGE_SIZE: usize = 4096;
const DEFAULT_CAPACITY: usize = 2 * PAGE_SIZE;
const MAX_CAPACITY: usize = 16 * PAGE_SIZE;
const ZERO_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 0,
};

fn ensure(condition: bool) -> Result<(), Errno> {
    if condition { Ok(()) } else { Err(EIO) }
}

fn expect_errno<T>(result: Result<T, Errno>, expected: Errno) -> Result<(), Errno> {
    match result {
        Err(actual) if actual == expected => Ok(()),
        _ => Err(EIO),
    }
}

fn close_pipe(rx: Fd, tx: Fd) -> Result<(), Errno> {
    close(rx)?;
    close(tx)
}

fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|index| seed.wrapping_add((index % 251) as u8))
        .collect()
}

fn test_default_rounding_and_limits() -> Result<(), Errno> {
    let (rx, tx) = pipe2(PipeFlags::NONBLOCK)?;
    ensure(fcntl_get_pipe_size(rx)? == DEFAULT_CAPACITY)?;
    ensure(fcntl_get_pipe_size(tx)? == DEFAULT_CAPACITY)?;
    ensure(fcntl_set_pipe_size(tx, 0)? == PAGE_SIZE)?;
    ensure(fcntl_set_pipe_size(tx, (PAGE_SIZE + 1) as u64)? == 2 * PAGE_SIZE)?;
    ensure(fcntl_set_pipe_size(tx, (2 * PAGE_SIZE + 1) as u64)? == 4 * PAGE_SIZE)?;
    expect_errno(fcntl_set_pipe_size(tx, (MAX_CAPACITY + 1) as u64), EPERM)?;
    expect_errno(fcntl_set_pipe_size(tx, i32::MAX as u64 + 1), EINVAL)?;
    close_pipe(rx, tx)
}

fn test_real_growth_and_fifo_pattern() -> Result<(), Errno> {
    let (rx, tx) = pipe2(PipeFlags::NONBLOCK)?;
    ensure(fcntl_set_pipe_size(tx, MAX_CAPACITY as u64)? == MAX_CAPACITY)?;
    let expected = pattern(MAX_CAPACITY, 17);
    ensure(write(tx, &expected)? == expected.len())?;
    ensure(ioctl_readable_bytes(rx)? == expected.len())?;
    expect_errno(write(tx, b"x"), EAGAIN)?;

    let mut actual = vec![0; expected.len()];
    ensure(read(rx, &mut actual)? == actual.len())?;
    ensure(actual == expected)?;
    close_pipe(rx, tx)
}

fn test_wrapped_resize_and_busy_atomicity() -> Result<(), Errno> {
    let (rx, tx) = pipe2(PipeFlags::NONBLOCK)?;
    let initial = pattern(7_000, 23);
    let appended = pattern(2_500, 61);
    ensure(write(tx, &initial)? == initial.len())?;
    let mut discarded = vec![0; 6_000];
    ensure(read(rx, &mut discarded)? == discarded.len())?;
    ensure(write(tx, &appended)? == appended.len())?;

    let mut expected = initial[6_000..].to_vec();
    expected.extend_from_slice(&appended);
    ensure(ioctl_readable_bytes(rx)? == expected.len())?;
    ensure(fcntl_set_pipe_size(rx, (4 * PAGE_SIZE) as u64)? == 4 * PAGE_SIZE)?;

    let extra = pattern(1_000, 101);
    ensure(write(tx, &extra)? == extra.len())?;
    expected.extend_from_slice(&extra);
    expect_errno(fcntl_set_pipe_size(rx, PAGE_SIZE as u64), EBUSY)?;
    ensure(fcntl_get_pipe_size(rx)? == 4 * PAGE_SIZE)?;
    ensure(ioctl_readable_bytes(rx)? == expected.len())?;

    let mut prefix = vec![0; 1_000];
    ensure(read(rx, &mut prefix)? == prefix.len())?;
    ensure(prefix == expected[..1_000])?;
    expected.drain(..1_000);
    ensure(fcntl_set_pipe_size(tx, PAGE_SIZE as u64)? == PAGE_SIZE)?;
    let mut actual = vec![0; expected.len()];
    ensure(read(rx, &mut actual)? == actual.len())?;
    ensure(actual == expected)?;
    close_pipe(rx, tx)
}

fn test_full_eagain_and_grow_readiness() -> Result<(), Errno> {
    let (rx, tx) = pipe2(PipeFlags::NONBLOCK)?;
    ensure(fcntl_set_pipe_size(tx, PAGE_SIZE as u64)? == PAGE_SIZE)?;
    let page = pattern(PAGE_SIZE, 149);
    ensure(write(tx, &page)? == page.len())?;
    expect_errno(write(tx, b"x"), EAGAIN)?;

    let mut writable = [PollFd {
        fd: tx as i32,
        events: POLLOUT,
        revents: 0,
    }];
    ensure(ppoll(&mut writable, Some(&ZERO_TIMEOUT))? == 0)?;
    ensure(writable[0].revents == 0)?;

    ensure(fcntl_set_pipe_size(tx, (2 * PAGE_SIZE) as u64)? == 2 * PAGE_SIZE)?;
    writable[0].revents = 0;
    ensure(ppoll(&mut writable, Some(&ZERO_TIMEOUT))? == 1)?;
    ensure(writable[0].revents & POLLOUT != 0)?;
    ensure(write(tx, &page)? == page.len())?;
    ensure(ioctl_readable_bytes(rx)? == 2 * PAGE_SIZE)?;
    close_pipe(rx, tx)
}

pub fn run() -> Result<(), Errno> {
    println!("fcntl-test pipe-capacity: CASE default-rounding-limits start");
    test_default_rounding_and_limits()?;
    println!("fcntl-test pipe-capacity: CASE default-rounding-limits ok");

    println!("fcntl-test pipe-capacity: CASE real-growth-fifo start");
    test_real_growth_and_fifo_pattern()?;
    println!("fcntl-test pipe-capacity: CASE real-growth-fifo ok");

    println!("fcntl-test pipe-capacity: CASE wrapped-resize-busy start");
    test_wrapped_resize_and_busy_atomicity()?;
    println!("fcntl-test pipe-capacity: CASE wrapped-resize-busy ok");

    println!("fcntl-test pipe-capacity: CASE full-eagain-grow-readiness start");
    test_full_eagain_and_grow_readiness()?;
    println!("fcntl-test pipe-capacity: CASE full-eagain-grow-readiness ok");
    println!("fcntl-test pipe-capacity: all cases passed");
    Ok(())
}
