use super::*;
use crate::prelude::*;

fn proc_root_lookup(dir: &InodeRef, name: &str) -> Result<InodeRef, SysError> {
    if let Some(tgid) = u32::from_str_radix(name, 10).ok() {
        // dynamic part.

        let tgid = Tid::new(tgid);

        // TODO: maintain a table for already generated inodes, otherwise double
        // source of truth will cause problems.
    }

    // TODO: static part.
    Err(SysError::NotFound)
}

fn proc_root_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let now = Instant::now().to_duration();

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: PROC_ROOT_INO,
        mode: InodeMode::new(InodeType::Dir, InodePerm::all_rwx()),
        nlink: 3, // TODO: should we calculate this dynamically? it's not hard, but it's too slow.
        uid: 0,
        gid: 0,
        rdev: DeviceId::None,
        size: 0,
        atime: now,
        mtime: now,
        ctime: now,
    })
}

static ROOT_INODE_OPS: InodeOps = InodeOps {
    lookup: proc_root_lookup,
    touch: |_, _, _| Err(SysError::NotSupported),
    mkdir: |_, _, _| Err(SysError::NotSupported),
    symlink: |_, _, _| Err(SysError::NotSupported),
    link: |_, _, _| Err(SysError::IsDir),
    unlink: |_, _| Err(SysError::IsDir),
    rmdir: |_, _| Err(SysError::NotSupported),
    open: |_| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: proc_root_get_attr,
};
