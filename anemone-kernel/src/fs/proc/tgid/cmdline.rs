use crate::{
    fs::{
        iomux::PollEvent,
        proc::tgid::{TgidEntry, default_tgid_entry_prv, validate_tgid_sub_inode},
    },
    prelude::{user_access::UserReadSlice, *},
    utils::any_opaque::NilOpaque,
};

fn tgid_cmdline_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let _binding = validate_tgid_sub_inode(inode)?;

    Ok(OpenedFile::new(&TGID_CMDLINE_FILE_OPS, NilOpaque::new()))
}

fn tgid_cmdline_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let _binding = validate_tgid_sub_inode(inode)?;
    let meta = inode.inode().meta_snapshot();
    let now = RealtimeInstant::now().to_duration();

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
    make_node: reject_make_node,
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

    // Keep the address-space completion guard from the range snapshot through
    // the copy: exec/exit can clear mappings while the proc binding and leader
    // are still observable.
    let mut usp = usp_handle.lock();
    let (addr, len) = usp.cmdline_range();

    if *pos >= len {
        return Ok(0);
    }

    let cur_task = get_current_task();
    let cur_usp_handle = cur_task.clone_uspace_handle();
    let _temporary_activation = (usp_handle != cur_usp_handle)
        .then(|| TemporaryUserSpaceActivation::new(cur_usp_handle.as_ref(), usp_handle.as_ref()));

    let to_read = usize::min(buf.len(), len - *pos);
    let start = addr
        .get()
        .checked_add(*pos as u64)
        .map(VirtAddr::new)
        .ok_or(SysError::BadAddress)?;
    let mut source = UserReadSlice::<u8>::try_new(start, to_read, &mut usp)?;
    let read = match source.copy_to_slice_partial(&mut buf[..to_read]) {
        Ok(read) => read,
        Err(error) if error.copied() != 0 => error.copied(),
        // This address belongs to the remote target, not the read caller. A
        // target retired before this guard was acquired is observed as EOF.
        Err(error) if error.error() == SysError::BadAddress => 0,
        Err(error) => return Err(error.error()),
    };

    *pos += read;

    Ok(read)
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
    read_user_at: None,
    write_user_at: None,
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
