#![no_std]
#![no_main]

use anemone_rs::{
    abi::fs::linux::open::{O_APPEND, O_NONBLOCK},
    os::linux::fs::{
        Fd, PipeFlags, close, dup, fcntl_getfl, fcntl_setfl, ioctl_set_nonblocking, pipe2, read,
    },
    prelude::*,
};

fn test_fionbio_roundtrip() -> Result<(), Errno> {
    let (read_fd, write_fd) = pipe2(PipeFlags::empty())?;
    let initial = fcntl_getfl(read_fd)?;
    assert_eq!(initial & O_NONBLOCK, 0);

    fcntl_setfl(read_fd, initial | O_APPEND)?;
    ioctl_set_nonblocking(read_fd, true)?;
    let enabled = fcntl_getfl(read_fd)?;
    assert_ne!(enabled & O_NONBLOCK, 0);
    assert_ne!(enabled & O_APPEND, 0);

    let mut byte = [0u8; 1];
    assert_eq!(read(read_fd, &mut byte).unwrap_err(), EAGAIN);

    ioctl_set_nonblocking(read_fd, false)?;
    let disabled = fcntl_getfl(read_fd)?;
    assert_eq!(disabled & O_NONBLOCK, 0);
    assert_ne!(disabled & O_APPEND, 0);

    close(read_fd)?;
    close(write_fd)?;
    Ok(())
}

fn test_fionbio_shared_description() -> Result<(), Errno> {
    let (read_fd, write_fd) = pipe2(PipeFlags::empty())?;
    let duplicate = dup(read_fd)?;

    ioctl_set_nonblocking(duplicate, true)?;
    assert_ne!(fcntl_getfl(read_fd)? & O_NONBLOCK, 0);

    ioctl_set_nonblocking(read_fd, false)?;
    assert_eq!(fcntl_getfl(duplicate)? & O_NONBLOCK, 0);

    close(duplicate)?;
    close(read_fd)?;
    close(write_fd)?;
    Ok(())
}

fn test_fionbio_bad_fd() -> Result<(), Errno> {
    const INVALID_FD: Fd = i32::MAX as Fd;

    assert_eq!(ioctl_set_nonblocking(INVALID_FD, true).unwrap_err(), EBADF);
    Ok(())
}

#[anemone_rs::main]
pub fn main() -> Result<(), Errno> {
    println!("ioctl-test: CASE fionbio-roundtrip start");
    test_fionbio_roundtrip()?;
    println!("ioctl-test: CASE fionbio-roundtrip ok");

    println!("ioctl-test: CASE fionbio-shared-description start");
    test_fionbio_shared_description()?;
    println!("ioctl-test: CASE fionbio-shared-description ok");

    println!("ioctl-test: CASE fionbio-bad-fd start");
    test_fionbio_bad_fd()?;
    println!("ioctl-test: CASE fionbio-bad-fd ok");

    println!("ioctl-test: all cases passed");
    Ok(())
}
