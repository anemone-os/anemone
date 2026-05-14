use crate::{
    fs::proc::tgid::{TgidEntry, validate_tgid_sub_inode},
    prelude::*,
    utils::any_opaque::NilOpaque,
};

fn tgid_mounts_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let _binding = validate_tgid_sub_inode(inode)?;

    Ok(OpenedFile {
        file_ops: &TGID_MOUNTS_FILE_OPS,
        prv: NilOpaque::new(),
    })
}

fn tgid_mounts_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let _binding = validate_tgid_sub_inode(inode)?;

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

static TGID_MOUNTS_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: tgid_mounts_open,
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: tgid_mounts_get_attr,
};

// TODO: for now we just return empty content.

fn tgid_mounts_read(_file: &File, pos: &mut usize, buf: &mut [u8]) -> Result<usize, SysError> {
    Ok(0)
}

fn tgid_mounts_validate_seek(_file: &File, pos: usize) -> Result<(), SysError> {
    if pos > 0 {
        Err(SysError::InvalidArgument)
    } else {
        Ok(())
    }
}

static TGID_MOUNTS_FILE_OPS: FileOps = FileOps {
    read: tgid_mounts_read,
    write: |_, _, _| Err(SysError::NotSupported),
    validate_seek: tgid_mounts_validate_seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, _| Ok(PollEvent::READABLE),
};

pub static TGID_MOUNTS_TGID_ENTRY: TgidEntry = TgidEntry {
    name: "mounts",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    inode_ops: &TGID_MOUNTS_INODE_OPS,
};
