use anemone_rs::{
    abi::fs::linux::open::{O_NOCTTY, O_RDWR},
    os::linux::fs::{AtFd, fstatat, mkdirat, mount, mount_with_data, umount},
    prelude::*,
};

use crate::support::{
    ADDITIONAL_VIEW, CANONICAL_VIEW, Pair, ensure, expect_errno, raw_termios, read_exact,
    same_inode, write_all,
};

const INVALID_VIEW: &str = "/tmp/pty-invalid-view";

fn ensure_dir(path: &str) -> Result<(), Errno> {
    match mkdirat(AtFd::Cwd, Path::new(path), 0o755) {
        Ok(()) | Err(EEXIST) => Ok(()),
        Err(errno) => Err(errno),
    }
}

fn mount_devpts(target: &str) -> Result<(), Errno> {
    mount(Path::new("devpts"), Path::new(target), "devpts")
}

pub fn test_mount_views() -> Result<(), Errno> {
    ensure_dir(INVALID_VIEW)?;
    expect_errno(
        mount_with_data(
            Path::new("devpts"),
            Path::new(INVALID_VIEW),
            "devpts",
            Some("newinstance"),
        ),
        EINVAL,
    )?;

    let canonical_root = fstatat(AtFd::Cwd, Path::new(CANONICAL_VIEW))?;
    let additional_root = fstatat(AtFd::Cwd, Path::new(ADDITIONAL_VIEW))?;
    ensure(same_inode(&canonical_root, &additional_root))?;

    let pair = Pair::allocate()?;
    pair.unlock()?;
    let canonical_path = pair.path_at(CANONICAL_VIEW);
    let additional_path = pair.path_at(ADDITIONAL_VIEW);
    let canonical_stat = fstatat(AtFd::Cwd, Path::new(canonical_path.as_str()))?;
    let additional_stat = fstatat(AtFd::Cwd, Path::new(additional_path.as_str()))?;
    ensure(same_inode(&canonical_stat, &additional_stat))?;
    ensure(canonical_stat.st_rdev == additional_stat.st_rdev)?;

    let canonical = pair.open_path_at(CANONICAL_VIEW, O_RDWR | O_NOCTTY)?;
    let additional = pair.open_path_at(ADDITIONAL_VIEW, O_RDWR | O_NOCTTY)?;
    raw_termios(canonical.raw())?;
    write_all(pair.master.raw(), b"view")?;
    let mut input = [0u8; 4];
    read_exact(additional.raw(), &mut input)?;
    ensure(&input == b"view")?;
    canonical.close()?;
    additional.close()?;

    umount(Path::new(ADDITIONAL_VIEW))?;
    let after_single_unmount = (|| {
        let slave = pair.open_path_at(CANONICAL_VIEW, O_RDWR | O_NOCTTY)?;
        raw_termios(slave.raw())?;
        write_all(slave.raw(), b"live")?;
        let mut output = [0u8; 4];
        read_exact(pair.master.raw(), &mut output)?;
        ensure(&output == b"live")
    })();
    if let Err(errno) = after_single_unmount {
        let _ = mount_devpts(ADDITIONAL_VIEW);
        return Err(errno);
    }

    umount(Path::new(CANONICAL_VIEW))?;
    let remount_result = mount_devpts(CANONICAL_VIEW).and_then(|_| {
        let remounted = fstatat(AtFd::Cwd, Path::new(canonical_path.as_str()))?;
        ensure(same_inode(&canonical_stat, &remounted))?;
        let slave = pair.open_path_at(CANONICAL_VIEW, O_RDWR | O_NOCTTY)?;
        raw_termios(slave.raw())?;
        write_all(pair.master.raw(), b"again")?;
        let mut input = [0u8; 5];
        read_exact(slave.raw(), &mut input)?;
        ensure(&input == b"again")
    });
    let additional_result = mount_devpts(ADDITIONAL_VIEW);
    remount_result.and(additional_result)
}
