use anemone_rs::{
    abi::{
        fs::linux::{
            open::{O_NOCTTY, O_RDWR},
            stat::Stat,
        },
        tty::linux::{ECHO, ICANON, ISIG, OPOST, Termios, VMIN, VTIME},
    },
    os::linux::{
        fs::{AtFd, Fd, close, openat, read, write},
        process::{WStatus, WStatusRaw, WaitFor, WaitOptions, wait4},
        tty::{SetTermiosWhen, pty_get_number, pty_open_peer, pty_set_lock, tcgetattr, tcsetattr},
    },
    prelude::*,
};

pub const PTMX_PATH: &str = "/dev/ptmx";
pub const CANONICAL_VIEW: &str = "/dev/pts";
pub const ADDITIONAL_VIEW: &str = "/tmp/pty-view";

pub struct OwnedFd(Fd);

impl OwnedFd {
    pub fn new(fd: Fd) -> Self {
        Self(fd)
    }

    pub fn raw(&self) -> Fd {
        self.0
    }

    pub fn close(mut self) -> Result<(), Errno> {
        let fd = core::mem::replace(&mut self.0, Fd::MAX);
        close(fd)
    }
}

impl Drop for OwnedFd {
    fn drop(&mut self) {
        if self.0 != Fd::MAX {
            let _ = close(self.0);
        }
    }
}

pub struct Pair {
    pub master: OwnedFd,
    pub number: u32,
}

impl Pair {
    pub fn allocate() -> Result<Self, Errno> {
        let master = OwnedFd::new(openat(
            AtFd::Cwd,
            Path::new(PTMX_PATH),
            O_RDWR | O_NOCTTY,
            0,
        )?);
        let number = pty_get_number(master.raw())?;
        Ok(Self { master, number })
    }

    pub fn unlock(&self) -> Result<(), Errno> {
        pty_set_lock(self.master.raw(), false)
    }

    pub fn path_at(&self, view: &str) -> String {
        format!("{view}/{}", self.number)
    }

    pub fn open_path_at(&self, view: &str, flags: u32) -> Result<OwnedFd, Errno> {
        let path = self.path_at(view);
        openat(AtFd::Cwd, Path::new(path.as_str()), flags, 0).map(OwnedFd::new)
    }

    pub fn open_path(&self, flags: u32) -> Result<OwnedFd, Errno> {
        self.open_path_at(CANONICAL_VIEW, flags)
    }

    pub fn open_peer(&self, flags: u32) -> Result<OwnedFd, Errno> {
        pty_open_peer(self.master.raw(), flags).map(OwnedFd::new)
    }
}

#[track_caller]
pub fn ensure(condition: bool) -> Result<(), Errno> {
    if condition {
        Ok(())
    } else {
        let caller = core::panic::Location::caller();
        println!("PTYTEST:ASSERT:{}:{}", caller.file(), caller.line());
        Err(EIO)
    }
}

#[track_caller]
pub fn expect_errno<T>(result: Result<T, Errno>, expected: Errno) -> Result<(), Errno> {
    match result {
        Err(actual) if actual == expected => Ok(()),
        _ => {
            let caller = core::panic::Location::caller();
            println!(
                "PTYTEST:ERRNO-MISMATCH:{}:{}:expected={}",
                caller.file(),
                caller.line(),
                expected
            );
            Err(EIO)
        },
    }
}

pub fn write_all(fd: Fd, mut bytes: &[u8]) -> Result<(), Errno> {
    while !bytes.is_empty() {
        match write(fd, bytes) {
            Ok(0) => return Err(EIO),
            Ok(count) => bytes = &bytes[count..],
            Err(EINTR) => {},
            Err(errno) => return Err(errno),
        }
    }
    Ok(())
}

pub fn read_exact(fd: Fd, mut bytes: &mut [u8]) -> Result<(), Errno> {
    while !bytes.is_empty() {
        match read(fd, bytes) {
            Ok(0) => return Err(EIO),
            Ok(count) => bytes = &mut bytes[count..],
            Err(EINTR) => {},
            Err(errno) => return Err(errno),
        }
    }
    Ok(())
}

pub fn raw_termios(fd: Fd) -> Result<Termios, Errno> {
    let mut termios = tcgetattr(fd)?;
    termios.c_iflag = 0;
    termios.c_oflag &= !OPOST;
    termios.c_lflag &= !(ICANON | ECHO | ISIG);
    termios.c_cc[VMIN] = 1;
    termios.c_cc[VTIME] = 0;
    tcsetattr(fd, SetTermiosWhen::Now, &termios)?;
    Ok(termios)
}

pub fn wait_child(child: u32) -> Result<(), Errno> {
    let mut status = WStatusRaw::EMPTY;
    loop {
        match wait4(
            WaitFor::ChildWithTgid(child),
            Some(&mut status),
            WaitOptions::empty(),
        ) {
            Ok(Some(waited)) if waited == child => break,
            Ok(Some(_)) => return Err(ECHILD),
            Ok(None) => return Err(ECHILD),
            Err(EINTR) => {},
            Err(errno) => return Err(errno),
        }
    }
    ensure(matches!(status.read(), WStatus::Exited(0)))
}

pub fn same_inode(left: &Stat, right: &Stat) -> bool {
    left.st_dev == right.st_dev && left.st_ino == right.st_ino
}
