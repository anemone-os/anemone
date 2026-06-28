use crate::{
    fs::{
        iomux::PollEvent,
        proc::tgid::{TgidEntry, default_tgid_entry_prv, validate_tgid_sub_inode},
    },
    prelude::*,
    utils::any_opaque::NilOpaque,
};

fn tgid_cmdline_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let _binding = validate_tgid_sub_inode(inode)?;

    Ok(OpenedFile::new(&TGID_CMDLINE_FILE_OPS, NilOpaque::new()))
}

fn tgid_cmdline_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
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

static TGID_CMDLINE_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: tgid_cmdline_open,
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: tgid_cmdline_get_attr,
};

fn tgid_cmdline_read(
    file: &File,
    pos: &mut usize,
    buf: &mut [u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let binding = validate_tgid_sub_inode(file.inode())?;
    if binding.tg.ty() == ThreadGroupType::KThread {
        return Ok(0);
    }
    let leader = binding.tg.leader().ok_or(SysError::NoSuchProcess)?;

    let usp_handle = leader.clone_uspace_handle();

    let (addr, len) = usp_handle.lock().cmdline_range();

    if *pos >= len {
        return Ok(0);
    }

    let cur_task = get_current_task();
    let cur_usp_handle = cur_task.clone_uspace_handle();
    if usp_handle != cur_usp_handle {
        usp_handle.activate();
    }

    // The command-line range is placed on the initial user stack together with
    // environ, so reading it follows the same direct-copy model as environ.
    let to_read = usize::min(buf.len(), len - *pos);

    unsafe {
        let src = (addr.get() as usize + *pos) as *const u8;
        let dst = buf.as_mut_ptr();

        core::ptr::copy_nonoverlapping(src, dst, to_read);
    }

    *pos += to_read;

    if usp_handle != cur_usp_handle {
        cur_usp_handle.activate();
    }

    Ok(to_read)
}

fn tgid_cmdline_read_at(
    file: &File,
    pos: usize,
    buf: &mut [u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let mut local_pos = pos;
    tgid_cmdline_read(file, &mut local_pos, buf, ctx)
}

fn tgid_cmdline_seek(file: &File, pos: &mut usize, from: SeekFrom) -> Result<usize, SysError> {
    let binding = validate_tgid_sub_inode(file.inode())?;
    if binding.tg.ty() == ThreadGroupType::KThread {
        return seek_with_bounded_size(file, pos, from, 0);
    }

    let leader = binding.tg.leader().ok_or(SysError::NoSuchProcess)?;
    let usp_handle = leader.clone_uspace_handle();

    let (_addr, len) = usp_handle.lock().cmdline_range();

    seek_with_bounded_size(file, pos, from, len)
}

static TGID_CMDLINE_FILE_OPS: FileOps = FileOps {
    read: tgid_cmdline_read,
    write: |_, _, _, _| Err(SysError::NotSupported),
    read_at: tgid_cmdline_read_at,
    write_at: |_, _, _, _| Err(SysError::NotSupported),
    check_status_flags: accept_file_op_status_flags,
    seek: tgid_cmdline_seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, req| Ok(req.ready_or_unsupported(PollEvent::READABLE & req.interests())),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

pub static TGID_CMDLINE_TGID_ENTRY: TgidEntry = TgidEntry {
    name: "cmdline",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    inode_ops: &TGID_CMDLINE_INODE_OPS,
    make_prv: default_tgid_entry_prv,
};
