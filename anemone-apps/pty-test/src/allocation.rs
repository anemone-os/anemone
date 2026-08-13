use anemone_rs::{
    abi::fs::linux::{
        dev_t,
        fcntl::FD_CLOEXEC,
        mode::{S_IFCHR, S_IFMT},
        open::{O_ACCMODE, O_CLOEXEC, O_NOCTTY, O_NONBLOCK, O_PATH, O_RDWR},
    },
    os::linux::{
        fs::{AtFd, Fd, fcntl_getfd, fcntl_getfl, fstat, fstatat, openat},
        process::{exit, fork, getgid, getuid, setuid},
        tty::{pty_get_number, pty_open_peer, pty_set_lock},
    },
    prelude::*,
};

use crate::support::{CANONICAL_VIEW, PTMX_PATH, Pair, ensure, expect_errno, wait_child};

pub fn test_allocation_admission() -> Result<(), Errno> {
    let uid = getuid()?;
    let gid = getgid()?;
    let pair = Pair::allocate()?;
    let master_stat = fstat(pair.master.raw())?;
    ensure(master_stat.st_mode & S_IFMT == S_IFCHR)?;
    ensure(master_stat.st_mode & 0o777 == 0o666)?;
    ensure(dev_t::decode(master_stat.st_rdev as u32) == (5, 2))?;

    expect_errno(pty_get_number(Fd::MAX), EBADF)?;
    let slave_path = pair.path_at(CANONICAL_VIEW);
    let slave_stat = fstatat(AtFd::Cwd, Path::new(slave_path.as_str()))?;
    ensure(slave_stat.st_mode & S_IFMT == S_IFCHR)?;
    ensure(slave_stat.st_mode & 0o777 == 0o600)?;
    ensure(slave_stat.st_uid == uid && slave_stat.st_gid == gid)?;
    ensure(dev_t::decode(slave_stat.st_rdev as u32) == (136, pair.number))?;

    expect_errno(pair.open_path(O_RDWR | O_NOCTTY), EIO)?;
    expect_errno(pair.open_peer(O_RDWR | O_NOCTTY), EIO)?;
    pair.unlock()?;
    pty_set_lock(pair.master.raw(), true)?;
    expect_errno(pair.open_path(O_RDWR | O_NOCTTY), EIO)?;
    expect_errno(pair.open_peer(O_RDWR | O_NOCTTY), EIO)?;
    pty_set_lock(pair.master.raw(), false)?;
    expect_errno(pty_open_peer(pair.master.raw(), O_PATH), EINVAL)?;
    expect_errno(pty_open_peer(pair.master.raw(), O_ACCMODE), EINVAL)?;

    let peer = pair.open_peer(O_RDWR | O_NOCTTY | O_NONBLOCK | O_CLOEXEC)?;
    expect_errno(pty_get_number(peer.raw()), ENOTTY)?;
    expect_errno(pty_set_lock(peer.raw(), false), ENOTTY)?;
    expect_errno(pty_open_peer(peer.raw(), O_RDWR | O_NOCTTY), ENOTTY)?;
    ensure(fcntl_getfl(peer.raw())? & O_NONBLOCK != 0)?;
    ensure(fcntl_getfd(peer.raw())? & FD_CLOEXEC != 0)?;
    let pathname = pair.open_path(O_RDWR | O_NOCTTY)?;
    ensure(fstat(pathname.raw())?.st_ino == slave_stat.st_ino)?;

    let child = match fork()? {
        None => {
            let outcome = setuid(65534)
                .and_then(|_| {
                    expect_errno(
                        openat(
                            AtFd::Cwd,
                            Path::new(slave_path.as_str()),
                            O_RDWR | O_NOCTTY,
                            0,
                        ),
                        EACCES,
                    )
                })
                .and_then(|_| pty_open_peer(pair.master.raw(), O_RDWR | O_NOCTTY).map(|_| ()));
            exit(if outcome.is_ok() { 0 } else { 1 })
        },
        Some(child) => child,
    };
    wait_child(child)
}

pub fn test_capacity_reuse() -> Result<(), Errno> {
    const ALLOCATION_SAFETY_BOUND: usize = 4096;

    let mut pairs: Vec<Pair> = Vec::new();
    let mut exhausted = false;
    for _ in 0..ALLOCATION_SAFETY_BOUND {
        match Pair::allocate() {
            Ok(pair) => {
                ensure(pairs.iter().all(|live| live.number != pair.number))?;
                pairs.push(pair);
            },
            Err(ENOSPC) => {
                exhausted = true;
                break;
            },
            Err(errno) => return Err(errno),
        }
    }
    ensure(exhausted && !pairs.is_empty())?;
    let released_numbers = pairs.iter().map(|pair| pair.number).collect::<Vec<_>>();
    drop(pairs);

    for _ in 0..32 {
        let churn = Pair::allocate()?;
        ensure(released_numbers.contains(&churn.number))?;
    }
    let stat = fstatat(AtFd::Cwd, Path::new(PTMX_PATH))?;
    ensure(stat.st_mode & S_IFMT == S_IFCHR)
}
