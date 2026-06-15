use crate::{
    fs::proc::{
        read_snapshot_at,
        tgid::{TgidEntry, default_tgid_entry_prv, validate_tgid_sub_inode},
    },
    prelude::*,
    utils::any_opaque::NilOpaque,
};

fn tgid_mounts_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let _binding = validate_tgid_sub_inode(inode)?;

    Ok(OpenedFile::new(&TGID_MOUNTS_FILE_OPS, NilOpaque::new()))
}

fn tgid_mounts_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let _binding = validate_tgid_sub_inode(inode)?;
    let meta = inode.inode().meta_snapshot();
    let now = Instant::now().to_duration();

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: inode.mode(),
        nlink: 1,
        uid: meta.uid,
        gid: meta.gid,
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
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: tgid_mounts_get_attr,
};

// TODO: for now we just return empty content.

fn tgid_mounts_read(
    _file: &File,
    _pos: &mut usize,
    _buf: &mut [u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    Ok(0)
}

fn tgid_mounts_read_at(
    _file: &File,
    pos: usize,
    buf: &mut [u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    read_snapshot_at(pos, buf, &[])
}

fn tgid_mounts_seek(file: &File, pos: &mut usize, from: SeekFrom) -> Result<usize, SysError> {
    seek_with_bounded_size(file, pos, from, 0)
}

static TGID_MOUNTS_FILE_OPS: FileOps = FileOps {
    read: tgid_mounts_read,
    write: |_, _, _, _| Err(SysError::NotSupported),
    read_at: tgid_mounts_read_at,
    write_at: |_, _, _, _| Err(SysError::NotSupported),
    check_status_flags: accept_file_op_status_flags,
    seek: tgid_mounts_seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, req| Ok(req.ready_or_unsupported(PollEvent::READABLE & req.interests())),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

pub static TGID_MOUNTS_TGID_ENTRY: TgidEntry = TgidEntry {
    name: "mounts",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    inode_ops: &TGID_MOUNTS_INODE_OPS,
    make_prv: default_tgid_entry_prv,
};
