use anemone_rs::{
    abi::{
        fs::linux::{
            open::{O_NOCTTY, O_RDWR},
            poll::{POLLHUP, PollFd},
        },
        time::linux::TimeSpec,
    },
    os::linux::{
        fs::{
            AtFd, PipeFlags, close, dup, fstat, fstatat, ioctl_readable_bytes, pipe2, ppoll, read,
            write,
        },
        process::{exit, fork},
        tty::pty_open_peer,
    },
    prelude::*,
};

const ZERO_TIMEOUT: TimeSpec = TimeSpec {
    tv_sec: 0,
    tv_nsec: 0,
};

use crate::support::{
    ADDITIONAL_VIEW, CANONICAL_VIEW, OwnedFd, Pair, ensure, expect_errno, raw_termios, read_exact,
    same_inode, wait_child, write_all,
};

pub fn test_identity_reuse() -> Result<(), Errno> {
    const REUSE_SEARCH_BOUND: usize = 4096;

    let pair = Pair::allocate()?;
    pair.unlock()?;
    let path = pair.path_at(CANONICAL_VIEW);
    let old_path_stat = fstatat(AtFd::Cwd, Path::new(path.as_str()))?;
    let old_slave = pair.open_path(O_RDWR | O_NOCTTY)?;
    let old_number = pair.number;
    let Pair { master, .. } = pair;
    master.close()?;

    let mut eof = [0u8; 1];
    ensure(read(old_slave.raw(), &mut eof)? == 0)?;
    expect_errno(write(old_slave.raw(), b"retired"), EIO)?;

    // Keep every other live index occupied until the allocator reuses the
    // retired episode. This proves reuse without freezing a first-free policy.
    let mut intervening = Vec::new();
    let mut current = None;
    for _ in 0..REUSE_SEARCH_BOUND {
        let candidate = Pair::allocate()?;
        if candidate.number == old_number {
            current = Some(candidate);
            break;
        }
        intervening.push(candidate);
    }
    let current = current.ok_or(EIO)?;
    current.unlock()?;
    // The canonical pathname may retain a generic VFS cached-positive dentry.
    // Resolve the reused binding through the second view, where this N has not
    // been looked up, so the oracle stays within the RFC's fresh-lookup claim.
    let current_path = current.path_at(ADDITIONAL_VIEW);
    let current_path_stat = fstatat(AtFd::Cwd, Path::new(current_path.as_str()))?;
    ensure(!same_inode(&old_path_stat, &current_path_stat))?;
    ensure(!same_inode(&fstat(old_slave.raw())?, &current_path_stat))?;
    let current_slave = current.open_path_at(ADDITIONAL_VIEW, O_RDWR | O_NOCTTY)?;
    raw_termios(current_slave.raw())?;
    write_all(current.master.raw(), b"new")?;
    let mut bytes = [0u8; 3];
    read_exact(current_slave.raw(), &mut bytes)?;
    ensure(&bytes == b"new")
}

pub fn test_description_lifecycle() -> Result<(), Errno> {
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let first = pair.open_path(O_RDWR | O_NOCTTY)?;
    let second = pair.open_peer(O_RDWR | O_NOCTTY)?;
    raw_termios(first.raw())?;
    let first_alias = OwnedFd::new(dup(first.raw())?);
    first.close()?;

    write_all(pair.master.raw(), b"shared")?;
    let mut bytes = [0u8; 6];
    read_exact(first_alias.raw(), &mut bytes)?;
    ensure(&bytes == b"shared")?;
    first_alias.close()?;

    write_all(second.raw(), b"still-live")?;
    second.close()?;
    ensure(ioctl_readable_bytes(pair.master.raw())? == 10)?;
    let mut output = [0u8; 10];
    read_exact(pair.master.raw(), &mut output)?;
    ensure(&output == b"still-live")?;
    let mut absent = [0u8; 1];
    ensure(ioctl_readable_bytes(pair.master.raw())? == 0)?;
    expect_errno(read(pair.master.raw(), &mut absent), EIO)?;
    let mut peer_state = [PollFd {
        fd: pair.master.raw() as i32,
        events: 0,
        revents: 0,
    }];
    ensure(ppoll(&mut peer_state, Some(&ZERO_TIMEOUT))? == 1)?;
    ensure(peer_state[0].revents & POLLHUP != 0)?;

    let reopened = pair.open_peer(O_RDWR | O_NOCTTY)?;
    peer_state[0].revents = 0;
    ensure(ppoll(&mut peer_state, Some(&ZERO_TIMEOUT))? == 0)?;
    ensure(peer_state[0].revents & POLLHUP == 0)?;
    raw_termios(reopened.raw())?;
    write_all(pair.master.raw(), b"reopen")?;
    let mut reopened_input = [0u8; 6];
    read_exact(reopened.raw(), &mut reopened_input)?;
    ensure(&reopened_input == b"reopen")?;

    let master_alias = OwnedFd::new(dup(pair.master.raw())?);
    let Pair { master, .. } = pair;
    master.close()?;
    let peer = OwnedFd::new(pty_open_peer(master_alias.raw(), O_RDWR | O_NOCTTY)?);
    peer.close()?;
    master_alias.close()?;
    expect_errno(ioctl_readable_bytes(reopened.raw()), EIO)?;
    ensure(read(reopened.raw(), &mut absent)? == 0)
}

pub fn test_fork_final_release() -> Result<(), Errno> {
    let pair = Pair::allocate()?;
    pair.unlock()?;
    let slave = pair.open_peer(O_RDWR | O_NOCTTY)?;
    raw_termios(slave.raw())?;
    let (ready_read, ready_write) = pipe2(PipeFlags::empty())?;
    let child = match fork()? {
        None => {
            let outcome = (|| {
                close(ready_read)?;
                let peer = pty_open_peer(pair.master.raw(), O_RDWR | O_NOCTTY)?;
                close(peer)?;
                ensure(write(ready_write, &[1])? == 1)?;
                close(ready_write)
            })();
            exit(if outcome.is_ok() { 0 } else { 1 })
        },
        Some(child) => child,
    };
    close(ready_write)?;
    let Pair { master, .. } = pair;
    master.close()?;
    let mut ready = [0u8; 1];
    read_exact(ready_read, &mut ready)?;
    close(ready_read)?;
    wait_child(child)?;
    let mut eof = [0u8; 1];
    ensure(read(slave.raw(), &mut eof)? == 0)
}
