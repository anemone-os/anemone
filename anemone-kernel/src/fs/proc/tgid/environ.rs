use crate::{
    fs::{
        iomux::PollEvent,
        proc::tgid::{TgidEntry, validate_tgid_sub_inode},
    },
    prelude::*,
    utils::any_opaque::NilOpaque,
};

fn tgid_environ_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let _binding = validate_tgid_sub_inode(inode)?;

    Ok(OpenedFile {
        file_ops: &TGID_ENVIRON_FILE_OPS,
        prv: NilOpaque::new(),
    })
}

fn tgid_environ_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
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

static TGID_ENVIRON_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: tgid_environ_open,
    read_link: |_| Err(SysError::NotSymlink),
    get_attr: tgid_environ_get_attr,
};

fn tgid_environ_read(file: &File, pos: &mut usize, buf: &mut [u8]) -> Result<usize, SysError> {
    let binding = validate_tgid_sub_inode(file.inode())?;
    let leader = binding.tg.leader().ok_or(SysError::NoSuchProcess)?;

    let usp_handle = leader.clone_uspace_handle();

    let (addr, len) = usp_handle.lock().env_range();

    if *pos >= len {
        return Ok(0);
    }

    let cur_task = get_current_task();
    let cur_usp_handle = cur_task.clone_uspace_handle();
    if usp_handle != cur_usp_handle {
        usp_handle.activate();
    }

    // now we can access target user space directly.
    // since environment range are guaranteed to be mapped when a user space is
    // constructed, and stack area is reserved, so it can't be modified by user.

    let to_read = usize::min(buf.len(), len - *pos);

    unsafe {
        let src = (addr.get() as usize + *pos) as *const u8;
        let dst = buf.as_mut_ptr();

        core::ptr::copy_nonoverlapping(src, dst, to_read);
    }

    *pos += to_read;

    if usp_handle != cur_usp_handle {
        // return to original user space.
        cur_usp_handle.activate();
    }

    Ok(to_read)
}

fn tgid_environ_validate_seek(file: &File, pos: usize) -> Result<(), SysError> {
    let binding = validate_tgid_sub_inode(file.inode())?;

    let leader = binding.tg.leader().ok_or(SysError::NoSuchProcess)?;
    let usp_handle = leader.clone_uspace_handle();

    let (_addr, len) = usp_handle.lock().env_range();

    if pos > len {
        return Err(SysError::InvalidArgument);
    }

    Ok(())
}

static TGID_ENVIRON_FILE_OPS: FileOps = FileOps {
    read: tgid_environ_read,
    write: |_, _, _| Err(SysError::NotSupported),
    validate_seek: tgid_environ_validate_seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, _| Ok(PollEvent::READABLE),
};

pub static TGID_ENVIRON_TGID_ENTRY: TgidEntry = TgidEntry {
    name: "environ",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    inode_ops: &TGID_ENVIRON_INODE_OPS,
};
