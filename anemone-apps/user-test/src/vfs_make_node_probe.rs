//! Temporary Stage 2 make-node runtime probe.
//!
//! C3 owns execution and removal. This module must not survive
//! `VFS-MAKE-NODE-CUTOVER`; the durable userspace surface is anemone-rs.

use alloc::ffi::CString;

use anemone_rs::{
    abi::{
        fs::linux::{
            at::AT_FDCWD,
            dev_t,
            mode::{S_IFBLK, S_IFCHR, S_IFIFO, S_IFMT, S_IFREG, S_IFSOCK},
            mount::MS_RDONLY,
            open::{O_DIRECTORY, O_RDONLY, O_RDWR},
            statx,
        },
        syscall::{
            linux::{SYS_GETDENTS64, SYS_MOUNT, SYS_SETUID},
            syscall,
        },
    },
    os::linux::{
        fs::{
            AtFd, chdir, close, fstatat, mkdirat, mknodat, mknodat_raw, mount, openat, read,
            statx as statx_at, umount, unlinkat, write,
        },
        process::{WStatus, WStatusRaw, WaitFor, WaitOptions, exit, fork, wait4},
    },
    prelude::*,
};

const EXT4_DIR: &str = "/vfs-make-node-probe";
const RAMFS_DIR: &str = "/tmp/vfs-make-node-probe";

fn ensure(condition: bool, context: &str) -> Result<(), Errno> {
    if condition {
        Ok(())
    } else {
        println!("vfs-make-node-probe: assertion failed: {context}");
        Err(EINVAL)
    }
}

fn expect_errno<T>(result: Result<T, Errno>, expected: Errno, context: &str) -> Result<(), Errno> {
    match result {
        Err(actual) if actual == expected => Ok(()),
        Err(actual) => {
            println!("vfs-make-node-probe: {context}: expected {expected:?}, got {actual:?}");
            Err(EINVAL)
        },
        Ok(_) => {
            println!("vfs-make-node-probe: {context}: unexpectedly succeeded");
            Err(EINVAL)
        },
    }
}

fn path(dir: &str, name: &str) -> PathBuf {
    let mut path = PathBuf::from(dir);
    path.push(name);
    path
}

fn ensure_dir(path: &str, mode: u32) -> Result<(), Errno> {
    match mkdirat(AtFd::Cwd, Path::new(path), mode) {
        Ok(()) | Err(EEXIST) => Ok(()),
        Err(err) => Err(err),
    }
}

fn dirent_contains(dir: &str, expected: &[&str]) -> Result<(), Errno> {
    let fd = openat(AtFd::Cwd, Path::new(dir), O_RDONLY | O_DIRECTORY, 0)?;
    let mut buffer = [0u8; 2048];
    let count = unsafe {
        syscall(
            SYS_GETDENTS64,
            fd as u64,
            buffer.as_mut_ptr() as u64,
            buffer.len() as u64,
            0,
            0,
            0,
        )
    }? as usize;
    close(fd)?;

    let mut found = [false; 5];
    let mut offset = 0;
    while offset + 19 <= count {
        let reclen = u16::from_ne_bytes([buffer[offset + 16], buffer[offset + 17]]) as usize;
        if reclen < 20 || offset + reclen > count {
            return Err(EINVAL);
        }
        let name_bytes = &buffer[offset + 19..offset + reclen];
        let name_len = name_bytes
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(EINVAL)?;
        for (index, name) in expected.iter().enumerate() {
            if name_bytes[..name_len] == name.as_bytes()[..] {
                found[index] = true;
            }
        }
        offset += reclen;
    }

    ensure(
        found[..expected.len()].iter().all(|entry| *entry),
        "getdents node matrix",
    )
}

fn verify_stat(path: &Path, ty: u32, mode: u32, rdev: Option<(u32, u32)>) -> Result<(), Errno> {
    let stat = fstatat(AtFd::Cwd, path)?;
    ensure(stat.st_mode & S_IFMT == ty, "stat node kind")?;
    ensure(stat.st_mode & 0o7777 == mode, "stat requested mode")?;
    ensure(stat.st_uid == 0 && stat.st_gid == 0, "stat final owner")?;

    let statx = statx_at(AtFd::Cwd, path, 0, statx::BASIC_STATS)?;
    ensure(u32::from(statx.stx_mode) & S_IFMT == ty, "statx node kind")?;
    ensure(
        u32::from(statx.stx_mode) & 0o7777 == mode,
        "statx requested mode",
    )?;
    match rdev {
        Some((major, minor)) => {
            let (stat_major, stat_minor) = dev_t::decode(stat.st_rdev as u32);
            ensure((stat_major, stat_minor) == (major, minor), "stat rdev")?;
            ensure(
                (statx.stx_rdev_major, statx.stx_rdev_minor) == (major, minor),
                "statx rdev",
            )?;
        },
        None => {
            ensure(stat.st_rdev == 0, "non-device stat rdev")?;
            ensure(
                statx.stx_rdev_major == 0 && statx.stx_rdev_minor == 0,
                "non-device statx rdev",
            )?;
        },
    }
    Ok(())
}

fn probe_backend(dir: &str) -> Result<(), Errno> {
    ensure_dir(dir, 0o777)?;
    let regular = path(dir, "regular");
    let fifo = path(dir, "fifo");
    let character = path(dir, "character");
    let block = path(dir, "block");
    let socket = path(dir, "socket");

    mknodat(AtFd::Cwd, &regular, S_IFREG | 0o6750, u32::MAX)?;
    mknodat(AtFd::Cwd, &fifo, S_IFIFO | 0o640, u32::MAX)?;
    mknodat(AtFd::Cwd, &character, S_IFCHR | 0o600, dev_t::encode(0, 0))?;
    mknodat(
        AtFd::Cwd,
        &block,
        S_IFBLK | 0o620,
        dev_t::encode(0xabc, 0x54321),
    )?;
    mknodat(AtFd::Cwd, &socket, S_IFSOCK | 0o660, u32::MAX)?;

    verify_stat(&regular, S_IFREG, 0o6750, None)?;
    verify_stat(&fifo, S_IFIFO, 0o640, None)?;
    verify_stat(&character, S_IFCHR, 0o600, Some((0, 0)))?;
    verify_stat(&block, S_IFBLK, 0o620, Some((0xabc, 0x54321)))?;
    verify_stat(&socket, S_IFSOCK, 0o660, None)?;
    dirent_contains(dir, &["regular", "fifo", "character", "block", "socket"])?;

    let fd = openat(AtFd::Cwd, &regular, O_RDWR, 0)?;
    ensure(write(fd, b"make-node")? == 9, "regular write")?;
    close(fd)?;
    let fd = openat(AtFd::Cwd, &regular, O_RDONLY, 0)?;
    let mut data = [0u8; 9];
    ensure(read(fd, &mut data)? == data.len(), "regular read length")?;
    ensure(&data == b"make-node", "regular read content")?;
    close(fd)?;

    expect_errno(
        mknodat(AtFd::Cwd, &regular, S_IFREG | 0o600, 0),
        EEXIST,
        "duplicate",
    )?;
    expect_errno(
        openat(AtFd::Cwd, &fifo, O_RDONLY, 0),
        EOPNOTSUPP,
        "fifo open",
    )?;
    for (node, label) in [
        (&character, "character open"),
        (&block, "block open"),
        (&socket, "socket open"),
    ] {
        expect_errno(openat(AtFd::Cwd, node, O_RDONLY, 0), ENXIO, label)?;
    }

    let mountpoint = path(dir, "mountpoint");
    ensure_dir(mountpoint.to_str().ok_or(EINVAL)?, 0o755)?;
    expect_errno(
        mount(&regular, &mountpoint, "ext4"),
        ENOTBLK,
        "mount non-block",
    )?;
    expect_errno(
        mount(&block, &mountpoint, "ext4"),
        ENOENT,
        "mount provider miss",
    )?;

    for node in [&regular, &fifo, &character, &block, &socket] {
        unlinkat(AtFd::Cwd, node, 0)?;
        expect_errno(fstatat(AtFd::Cwd, node), ENOENT, "unlink lookup")?;
    }
    Ok(())
}

fn probe_dirfd_and_pointer_boundaries() -> Result<(), Errno> {
    let fd = openat(AtFd::Cwd, Path::new(RAMFS_DIR), O_RDONLY | O_DIRECTORY, 0)?;
    mknodat(AtFd::Fd(fd), Path::new("relative"), S_IFREG | 0o600, 0)?;
    close(fd)?;
    unlinkat(AtFd::Cwd, &path(RAMFS_DIR, "relative"), 0)?;

    let absolute = CString::new(
        path(RAMFS_DIR, "absolute-invalid-dirfd")
            .to_str()
            .ok_or(EINVAL)?,
    )
    .map_err(|_| EINVAL)?;
    unsafe { mknodat_raw(-1, absolute.as_ptr().cast(), S_IFREG | 0o600, 0) }?;
    unlinkat(AtFd::Cwd, &path(RAMFS_DIR, "absolute-invalid-dirfd"), 0)?;

    expect_errno(
        unsafe { mknodat_raw(AT_FDCWD, core::ptr::null(), S_IFREG | 0o600, 0) },
        EFAULT,
        "bad pathname pointer",
    )?;
    expect_errno(
        mknodat(AtFd::Cwd, Path::new(RAMFS_DIR), S_IFREG | 0o600, 0),
        EEXIST,
        "existing path",
    )
}

fn probe_ext4_name_admission() -> Result<(), Errno> {
    // Namei rejects a component above the shared 255-byte limit before the
    // filesystem backend can allocate or publish an inode.
    let name = String::from_utf8(vec![b'x'; 256]).map_err(|_| EINVAL)?;
    let rejected = path(EXT4_DIR, name.as_str());
    expect_errno(
        mknodat(AtFd::Cwd, &rejected, S_IFREG | 0o600, 0),
        ENAMETOOLONG,
        "ext4 name admission",
    )?;
    expect_errno(
        fstatat(AtFd::Cwd, &rejected),
        ENOENT,
        "ext4 rejected name leaves no dirent",
    )
}

fn probe_readonly_mount() -> Result<(), Errno> {
    let mountpoint = "/tmp/vfs-make-node-readonly";
    ensure_dir(mountpoint, 0o755)?;
    let source = CString::new("ramfs").unwrap();
    let target = CString::new(mountpoint).unwrap();
    let fstype = CString::new("ramfs").unwrap();
    unsafe {
        syscall(
            SYS_MOUNT,
            source.as_ptr() as u64,
            target.as_ptr() as u64,
            fstype.as_ptr() as u64,
            MS_RDONLY,
            0,
            0,
        )
    }?;
    expect_errno(
        mknodat(AtFd::Cwd, &path(mountpoint, "node"), S_IFREG | 0o600, 0),
        EROFS,
        "read-only mount",
    )?;
    umount(Path::new(mountpoint))
}

fn probe_cap_mknod() -> Result<(), Errno> {
    let pid = match fork()? {
        Some(pid) => pid,
        None => {
            unsafe { syscall(SYS_SETUID, 65534, 0, 0, 0, 0, 0) }
                .expect("vfs-make-node-probe: setuid failed");
            let result = mknodat(
                AtFd::Cwd,
                &path(RAMFS_DIR, "unprivileged-character"),
                S_IFCHR | 0o600,
                dev_t::encode(0, 0),
            );
            exit(if result == Err(EPERM) { 0 } else { 1 });
        },
    };
    let mut status = WStatusRaw::EMPTY;
    wait4(
        WaitFor::ChildWithTgid(pid),
        Some(&mut status),
        WaitOptions::empty(),
    )?;
    ensure(
        matches!(status.read(), WStatus::Exited(0)),
        "CAP_MKNOD child",
    )
}

fn run_inner() -> Result<(), Errno> {
    probe_backend(EXT4_DIR)?;
    probe_backend(RAMFS_DIR)?;
    probe_ext4_name_admission()?;
    probe_dirfd_and_pointer_boundaries()?;
    probe_readonly_mount()?;
    probe_cap_mknod()?;
    chdir("/")
}

pub(crate) fn run() {
    println!("vfs-make-node-probe: starting");
    run_inner().expect("vfs-make-node-probe: failed");
    println!("vfs-make-node-probe: passed");
}
