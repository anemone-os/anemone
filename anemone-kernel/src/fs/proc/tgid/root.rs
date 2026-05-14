use crate::fs::iomux::PollEvent;

use super::*;

fn tgid_root_read_link(inode: &InodeRef) -> Result<PathBuf, SysError> {
    let _tg = validate_tgid_sub_inode(inode)?;

    // let leader = tg.leader();

    // idk if this is true. we only have one global namespace. but we support
    // chroot. refine this later.
    Ok(PathBuf::from("/"))
}

fn tgid_root_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    validate_tgid_sub_inode(inode)?;

    let now = Instant::now().to_duration();

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: inode.mode(),
        nlink: inode.nlink(),
        uid: 0,
        gid: 0,
        rdev: DeviceId::None,
        size: 0,
        atime: now,
        mtime: now,
        ctime: now,
    })
}

static TGID_ROOT_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotSupported),
    touch: |_, _, _| Err(SysError::NotSupported),
    mkdir: |_, _, _| Err(SysError::NotSupported),
    symlink: |_, _, _| Err(SysError::NotSupported),
    link: |_, _, _| Err(SysError::NotSupported),
    unlink: |_, _| Err(SysError::NotSupported),
    rmdir: |_, _| Err(SysError::NotSupported),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: |_| Err(SysError::NotSupported),
    read_link: tgid_root_read_link,
    get_attr: tgid_root_get_attr,
};

static TGID_ROOT_FILE_OPS: FileOps = FileOps {
    read: |_, _, _| Err(SysError::NotSupported),
    write: |_, _, _| Err(SysError::NotSupported),
    validate_seek: |_, _| Err(SysError::NotSupported),
    read_dir: |_, _, _| Err(SysError::NotSupported),
    poll: |_, _| Ok(PollEvent::READABLE),
};

pub static TGID_ROOT_TGID_ENTRY: TgidEntry = TgidEntry {
    name: "root",
    mode: InodeMode::new(InodeType::Symlink, InodePerm::all_rwx()),
    inode_ops: &TGID_ROOT_INODE_OPS,
};
