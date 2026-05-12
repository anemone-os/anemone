use crate::{
    fs::proc::tgid::{TgidEntry, validate_tgid_sub_inode},
    prelude::*,
};

fn tgid_exe_read_link(inode: &InodeRef) -> Result<PathBuf, SysError> {
    let binding = validate_tgid_sub_inode(inode)?;
    let leader = binding.tg.leader().ok_or(SysError::NoSuchProcess)?;
    let usp_handle = leader.clone_uspace_handle();
    let exe = usp_handle.exe();

    // in leader's namespace.
    // if the exe is not accessible in leader's namespace, we should return an error
    // instead of leaking info about the host fs.
    let rel_exe = leader
        .rel_abs_path(&exe.to_pathbuf())
        .ok_or(SysError::PermissionDenied)?;

    Ok(rel_exe)
}

fn tgid_exe_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let _binding = validate_tgid_sub_inode(inode)?;

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

static TGID_EXE_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    open: |_| Err(SysError::NotSupported),
    read_link: tgid_exe_read_link,
    get_attr: tgid_exe_get_attr,
};

pub static TGID_EXE_TGID_ENTRY: TgidEntry = TgidEntry {
    name: "exe",
    mode: InodeMode::new(InodeType::Symlink, InodePerm::all_rwx()),
    inode_ops: &TGID_EXE_INODE_OPS,
};
