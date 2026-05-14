use crate::{fs::proc::pde::ProcDirEntry, prelude::*};

fn proc_mounts_read_link(_inode: &InodeRef) -> Result<PathBuf, SysError> {
    Ok(PathBuf::from("self/mounts"))
}

fn proc_mounts_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let now = Instant::now().to_duration();

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: inode.mode(),
        nlink: 1,
        uid: 0,
        gid: 0,
        rdev: DeviceId::None,
        size: 0,
        atime: now,
        mtime: now,
        ctime: now,
    })
}

static PROC_MOUNTS_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: |_| Err(SysError::IsDir),
    read_link: proc_mounts_read_link,
    get_attr: proc_mounts_get_attr,
};

pub static PROC_MOUNTS_DIR_ENTRY: ProcDirEntry = ProcDirEntry {
    name: "mounts",
    mode: InodeMode::new(InodeType::Symlink, InodePerm::all_rwx()),
    ops: &PROC_MOUNTS_INODE_OPS,
    ino: unsafe { MonoOnce::new() },
};
