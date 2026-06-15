use crate::{
    fs::proc::{
        pde::{ProcDirEntry, ProcDirEntryKind},
        read_snapshot_at,
    },
    prelude::*,
    utils::any_opaque::NilOpaque,
};

fn proc_uptime_open(_inode: &InodeRef) -> Result<OpenedFile, SysError> {
    Ok(OpenedFile::new(&PROC_UPTIME_FILE_OPS, NilOpaque::new()))
}

fn proc_uptime_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
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

static PROC_UPTIME_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: proc_uptime_open,
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: proc_uptime_get_attr,
};

fn uptime_string() -> String {
    // kernel dosn't use floating point.
    let uptime = Instant::now().to_duration().as_secs();

    let idle_uptime = 0; // TODO: calculate idle uptime.

    format!("{}.{:02} {}.{:02}\n", uptime, 0, idle_uptime, 0)
}

fn proc_uptime_read(
    file: &File,
    pos: &mut usize,
    buf: &mut [u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let uptime_string = uptime_string();
    let uptime_bytes = uptime_string.as_bytes();

    if *pos >= uptime_bytes.len() {
        return Ok(0);
    }

    let to_read = usize::min(buf.len(), uptime_bytes.len() - *pos);
    buf[..to_read].copy_from_slice(&uptime_bytes[*pos..*pos + to_read]);
    *pos += to_read;

    Ok(to_read)
}

fn proc_uptime_read_at(
    _file: &File,
    pos: usize,
    buf: &mut [u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let uptime_string = uptime_string();

    read_snapshot_at(pos, buf, uptime_string.as_bytes())
}

fn proc_uptime_seek(file: &File, pos: &mut usize, from: SeekFrom) -> Result<usize, SysError> {
    let uptime_string = uptime_string();
    let uptime_bytes = uptime_string.as_bytes();

    seek_with_bounded_size(file, pos, from, uptime_bytes.len())
}

static PROC_UPTIME_FILE_OPS: FileOps = FileOps {
    read: proc_uptime_read,
    write: |_, _, _, _| Err(SysError::NotSupported),
    read_at: proc_uptime_read_at,
    write_at: |_, _, _, _| Err(SysError::NotSupported),
    check_status_flags: accept_file_op_status_flags,
    seek: proc_uptime_seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, req| Ok(req.ready_or_unsupported(PollEvent::READABLE & req.interests())),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

pub static PROC_UPTIME_DIR_ENTRY: ProcDirEntry = ProcDirEntry {
    name: "uptime",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    kind: ProcDirEntryKind::Custom(&PROC_UPTIME_INODE_OPS),
    ino: unsafe { MonoOnce::new() },
};
