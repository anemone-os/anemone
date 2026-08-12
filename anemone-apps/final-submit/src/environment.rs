use anemone_rs::{
    abi::fs::linux::open::O_RDONLY,
    os::linux::fs::{AtFd, close, fstatat, mkdirat, mount, openat},
    prelude::*,
};

pub(crate) fn path_exists(path: &str) -> bool {
    let Ok(fd) = openat(AtFd::Cwd, Path::new(path), O_RDONLY, 0) else {
        return false;
    };
    let _ = close(fd);
    true
}

pub(crate) fn ensure_dir(path: &str, mode: u32) -> Result<(), Errno> {
    match fstatat(AtFd::Cwd, Path::new(path)) {
        Ok(_) => Ok(()),
        Err(ENOENT) => mkdirat(AtFd::Cwd, Path::new(path), mode),
        Err(errno) => Err(errno),
    }
}

fn mount_fs(source: &str, target: &str, fstype: &str) -> Result<(), Errno> {
    mount(Path::new(source), Path::new(target), fstype).map_err(|errno| {
        eprintln!("final-submit: mount {fstype} on {target} failed: {errno}");
        errno
    })
}

pub(crate) fn prepare_common_filesystems() -> Result<(), Errno> {
    ensure_dir("/dev", 0o755)?;
    mount_fs("devfs", "/dev", "devfs")?;

    ensure_dir("/dev/shm", 0o1777)?;
    mount_fs("ramfs", "/dev/shm", "ramfs")?;

    ensure_dir("/proc", 0o755)?;
    mount_fs("proc", "/proc", "proc")?;

    ensure_dir("/run", 0o755)?;
    mount_fs("ramfs", "/run", "ramfs")?;

    ensure_dir("/tmp", 0o1777)?;
    mount_fs("ramfs", "/tmp", "ramfs")?;
    Ok(())
}
