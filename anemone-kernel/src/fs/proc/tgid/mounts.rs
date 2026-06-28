use crate::{
    fs::proc::{
        read_snapshot_at,
        tgid::{
            TgidEntry, binding::ThreadGroupBinding, default_tgid_entry_prv, validate_tgid_sub_inode,
        },
    },
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

#[derive(Debug, Opaque)]
struct TgidMountsFilePrivate {
    binding: Arc<ThreadGroupBinding>,
}

#[inline]
fn tgid_mounts_file_private(file: &File) -> &TgidMountsFilePrivate {
    file.prv().cast::<TgidMountsFilePrivate>().unwrap()
}

fn tgid_mounts_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let binding = validate_tgid_sub_inode(inode)?;

    Ok(OpenedFile::new(
        &TGID_MOUNTS_FILE_OPS,
        AnyOpaque::new(TgidMountsFilePrivate { binding }),
    ))
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

fn tgid_mounts_reader(binding: &Arc<ThreadGroupBinding>) -> Result<Option<Arc<Task>>, SysError> {
    if !binding.alive() {
        return Err(SysError::NoSuchProcess);
    }

    if binding.tg.ty() == ThreadGroupType::KThread {
        return Ok(None);
    }

    let current = get_current_task();
    if binding.tg.tgid() == current.get_thread_group().tgid() {
        return Ok(Some(current));
    }

    binding.tg.leader().ok_or(SysError::NoSuchProcess).map(Some)
}

fn mount_source_name(mount: &Mount) -> String {
    match mount.sb().backing() {
        MountSource::Pseudo => mount.sb().fs().name().to_string(),
        MountSource::Block(dev) => format!("dev({})", dev.devnum()),
    }
}

fn mount_options(mount: &Mount) -> &'static str {
    if mount.attrs().contains(MountAttrFlags::RDONLY) {
        "ro"
    } else {
        "rw"
    }
}

fn tgid_mounts_snapshot(binding: &Arc<ThreadGroupBinding>) -> Result<Vec<u8>, SysError> {
    let Some(reader) = tgid_mounts_reader(binding)? else {
        return Ok(Vec::new());
    };

    let mut out = String::new();
    for mount in visible_mounts_snapshot() {
        let global_target = PathRef::new(mount.clone(), mount.root().clone()).to_pathbuf();
        let Some(target) = reader.rel_abs_path(&global_target) else {
            knoticeln!(
                "proc mounts: skipping mount target={} reason=outside-task-root tgid={}",
                global_target.display(),
                reader.get_thread_group().tgid()
            );
            continue;
        };

        out.push_str(&format!(
            "{} {} {} {} 0 0\n",
            mount_source_name(&mount),
            target.display(),
            mount.sb().fs().name(),
            mount_options(&mount)
        ));
    }

    Ok(out.into_bytes())
}

fn tgid_mounts_read(
    file: &File,
    pos: &mut usize,
    buf: &mut [u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let data = tgid_mounts_snapshot(&tgid_mounts_file_private(file).binding)?;
    let read = read_snapshot_at(*pos, buf, &data)?;
    *pos += read;
    Ok(read)
}

fn tgid_mounts_read_at(
    file: &File,
    pos: usize,
    buf: &mut [u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let data = tgid_mounts_snapshot(&tgid_mounts_file_private(file).binding)?;
    read_snapshot_at(pos, buf, &data)
}

fn tgid_mounts_seek(file: &File, pos: &mut usize, from: SeekFrom) -> Result<usize, SysError> {
    let data = tgid_mounts_snapshot(&tgid_mounts_file_private(file).binding)?;
    seek_with_bounded_size(file, pos, from, data.len())
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
