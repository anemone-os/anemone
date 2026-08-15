use anemone_abi::tty::linux::{
    TCGETS, TCSETS, TCSETSF, TCSETSW, TIOCGPGRP, TIOCGPTN, TIOCGPTPEER, TIOCGSID,
    TIOCGWINSZ, TIOCNOTTY, TIOCSCTTY, TIOCSPGRP, TIOCSPTLCK, TIOCSWINSZ, Termios, Winsize,
};

use crate::{os::linux::fs::Fd, prelude::*, sys::linux::fs};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetTermiosWhen {
    Now,
    Drain,
    DrainFlush,
}

pub fn tcgetattr(fd: Fd) -> Result<Termios, Errno> {
    let mut termios = Termios::default();
    fs::ioctl(
        fd as u64,
        TCGETS as u64,
        &mut termios as *mut Termios as u64,
    )?;
    Ok(termios)
}

pub fn tcsetattr(fd: Fd, when: SetTermiosWhen, termios: &Termios) -> Result<(), Errno> {
    let command = match when {
        SetTermiosWhen::Now => TCSETS,
        SetTermiosWhen::Drain => TCSETSW,
        SetTermiosWhen::DrainFlush => TCSETSF,
    };
    fs::ioctl(fd as u64, command as u64, termios as *const Termios as u64).map(|_| ())
}

pub fn get_winsize(fd: Fd) -> Result<Winsize, Errno> {
    let mut winsize = Winsize::default();
    fs::ioctl(
        fd as u64,
        TIOCGWINSZ as u64,
        &mut winsize as *mut Winsize as u64,
    )?;
    Ok(winsize)
}

pub fn set_winsize(fd: Fd, winsize: &Winsize) -> Result<(), Errno> {
    fs::ioctl(
        fd as u64,
        TIOCSWINSZ as u64,
        winsize as *const Winsize as u64,
    )
    .map(|_| ())
}

pub fn tiocsctty(fd: Fd, argument: u64) -> Result<(), Errno> {
    fs::ioctl(fd as u64, TIOCSCTTY as u64, argument).map(|_| ())
}

pub fn tiocnotty(fd: Fd) -> Result<(), Errno> {
    fs::ioctl(fd as u64, TIOCNOTTY as u64, 0).map(|_| ())
}

pub fn tcgetsid(fd: Fd) -> Result<i32, Errno> {
    let mut sid = 0i32;
    fs::ioctl(fd as u64, TIOCGSID as u64, &mut sid as *mut i32 as u64)?;
    Ok(sid)
}

pub fn tcgetpgrp(fd: Fd) -> Result<i32, Errno> {
    let mut pgid = 0i32;
    fs::ioctl(
        fd as u64,
        TIOCGPGRP as u64,
        &mut pgid as *mut i32 as u64,
    )?;
    Ok(pgid)
}

pub fn tcsetpgrp(fd: Fd, pgid: i32) -> Result<(), Errno> {
    fs::ioctl(fd as u64, TIOCSPGRP as u64, &pgid as *const i32 as u64).map(|_| ())
}

pub fn pty_get_number(master: Fd) -> Result<u32, Errno> {
    let mut number = 0u32;
    fs::ioctl(
        master as u64,
        TIOCGPTN as u64,
        &mut number as *mut u32 as u64,
    )?;
    Ok(number)
}

pub fn pty_set_lock(master: Fd, locked: bool) -> Result<(), Errno> {
    let locked = i32::from(locked);
    fs::ioctl(
        master as u64,
        TIOCSPTLCK as u64,
        &locked as *const i32 as u64,
    )
    .map(|_| ())
}

/// Open the slave peer through a live PTY master using Linux open flags.
///
/// Keeping the raw flag word visible is intentional: the PTY acceptance app
/// must exercise both the accepted mask and invalid-bit rejection without a
/// libc helper normalizing the input first.
pub fn pty_open_peer(master: Fd, flags: u32) -> Result<Fd, Errno> {
    fs::ioctl(master as u64, TIOCGPTPEER as u64, flags as u64).map(|fd| fd as Fd)
}

/// Issues an ioctl whose command has no argument payload.
pub fn ioctl_noarg(fd: Fd, command: u32) -> Result<(), Errno> {
    fs::ioctl(fd as u64, command as u64, 0).map(|_| ())
}
