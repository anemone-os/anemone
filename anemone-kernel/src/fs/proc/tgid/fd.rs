use crate::{
    fs::{
        inode::Inode,
        iomux::PollEvent,
        pipe,
        proc::{
            superblock::alloc_ino,
            tgid::{SubInoRecord, TgidEntry, binding::ThreadGroupBinding},
        },
    },
    prelude::*,
    task::files::{Fd, FileDesc},
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

#[derive(Debug, Opaque)]
pub struct ProcFdDirPrivate {
    binding: Arc<ThreadGroupBinding>,
    child_ino: SpinLock<HashMap<Fd, SubInoRecord>>,
}

#[derive(Debug, Opaque)]
struct ProcFdEntryPrivate {
    binding: Arc<ThreadGroupBinding>,
    fd: Fd,
}

#[inline]
fn proc_fd_dir_private(inode: &InodeRef) -> &ProcFdDirPrivate {
    inode.inode().prv().cast::<ProcFdDirPrivate>().unwrap()
}

#[inline]
fn proc_fd_entry_private(inode: &InodeRef) -> &ProcFdEntryPrivate {
    inode.inode().prv().cast::<ProcFdEntryPrivate>().unwrap()
}

fn make_proc_fd_dir_prv(binding: Arc<ThreadGroupBinding>) -> AnyOpaque {
    AnyOpaque::new(ProcFdDirPrivate {
        binding,
        child_ino: SpinLock::new(HashMap::new()),
    })
}

pub fn validate_fd_access(binding: &Arc<ThreadGroupBinding>) -> Result<Arc<Task>, SysError> {
    if !binding.alive() {
        return Err(SysError::NoSuchProcess);
    }

    if binding.tg.tgid() != get_current_task().get_thread_group().tgid() {
        return Err(SysError::AccessDenied);
    }

    binding.tg.leader().ok_or(SysError::NoSuchProcess)
}

pub fn parse_proc_fd_name(name: &str) -> Result<Fd, SysError> {
    if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(SysError::NotFound);
    }

    let raw = name.parse::<u32>().map_err(|_| SysError::NotFound)?;
    Fd::new(raw).ok_or(SysError::NotFound)
}

pub fn lookup_proc_fd(leader: &Task, fd: Fd) -> Result<Arc<FileDesc>, SysError> {
    leader.get_fd(fd).map_err(|err| match err {
        SysError::BadFileDescriptor => SysError::NotFound,
        other => other,
    })
}

fn proc_fd_anon_display_name(file: &File) -> PathBuf {
    let target = format!("anon_inode:[anemone-{}]", file.inode().ino().get());
    PathBuf::from(target.as_str())
}

fn proc_fd_readlink_target(leader: &Task, file_desc: &FileDesc) -> Result<PathBuf, SysError> {
    let file = file_desc.vfs_file();

    if let Some(target) = pipe::display_name(file) {
        return Ok(target);
    }

    if file.path().mount().sb().fs().name() == "anonymous" {
        return Ok(proc_fd_anon_display_name(file));
    }

    let path = file.path().to_pathbuf();
    leader.rel_abs_path(&path).ok_or(SysError::PermissionDenied)
}

fn proc_fd_child_ino(dir: &ProcFdDirPrivate, fd: Fd) -> (Ino, bool) {
    let mut child_ino = dir.child_ino.lock();
    if let Some(SubInoRecord { ino, instantiated }) = child_ino.get_mut(&fd) {
        (*ino, *instantiated)
    } else {
        let ino = alloc_ino();
        child_ino.insert(
            fd,
            SubInoRecord {
                ino,
                instantiated: false,
            },
        );
        (ino, false)
    }
}

fn new_proc_fd_entry_inode(
    binding: Arc<ThreadGroupBinding>,
    fd: Fd,
    sb: Arc<SuperBlock>,
    ino: Ino,
) -> Inode {
    let inode = Inode::new(
        ino,
        InodeType::Symlink,
        &PROC_FD_ENTRY_INODE_OPS,
        sb,
        AnyOpaque::new(ProcFdEntryPrivate { binding, fd }),
    );

    let now = Instant::now().to_duration();
    inode.set_meta(&InodeMeta {
        nlink: 1,
        size: 0,
        perm: InodePerm::all_rwx(),
        uid: Uid::ROOT,
        gid: Gid::ROOT,
        atime: now,
        mtime: now,
        ctime: now,
    });

    inode
}

fn proc_fd_child_inode(dir_inode: &InodeRef, fd: Fd) -> Result<InodeRef, SysError> {
    let dir = proc_fd_dir_private(dir_inode);
    let mut child_ino = dir.child_ino.lock();

    if let Some(SubInoRecord { ino, instantiated }) = child_ino.get_mut(&fd) {
        if *instantiated {
            return Ok(dir_inode
                .sb()
                .try_iget(*ino)
                .expect("proc fd child inode should exist if its ino is recorded"));
        }

        let inode = new_proc_fd_entry_inode(dir.binding.clone(), fd, dir_inode.sb(), *ino);
        let inode = dir_inode.sb().seed_inode(Arc::new(inode));
        *instantiated = true;
        return Ok(inode);
    }

    let ino = alloc_ino();
    let inode = new_proc_fd_entry_inode(dir.binding.clone(), fd, dir_inode.sb(), ino);
    let inode = dir_inode.sb().seed_inode(Arc::new(inode));
    child_ino.insert(
        fd,
        SubInoRecord {
            ino,
            instantiated: true,
        },
    );
    Ok(inode)
}

fn proc_fd_lookup(dir_inode: &InodeRef, name: &str) -> Result<InodeRef, SysError> {
    let dir = proc_fd_dir_private(dir_inode);
    let leader = validate_fd_access(&dir.binding)?;
    let fd = parse_proc_fd_name(name)?;
    let _file_desc = lookup_proc_fd(&leader, fd)?;

    proc_fd_child_inode(dir_inode, fd)
}

fn proc_fd_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let dir = proc_fd_dir_private(inode);
    let _leader = validate_fd_access(&dir.binding)?;

    Ok(OpenedFile::new(&PROC_FD_DIR_FILE_OPS, NilOpaque::new()))
}

fn proc_fd_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let dir = proc_fd_dir_private(inode);
    let _leader = validate_fd_access(&dir.binding)?;
    let meta = inode.inode().meta_snapshot();
    let now = Instant::now().to_duration();

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: inode.mode(),
        nlink: inode.nlink(),
        uid: meta.uid,
        gid: meta.gid,
        rdev: DeviceId::None,
        size: 0,
        atime: now,
        mtime: now,
        ctime: now,
    })
}

fn proc_fd_entry_read_link(inode: &InodeRef) -> Result<PathBuf, SysError> {
    let entry = proc_fd_entry_private(inode);
    let leader = validate_fd_access(&entry.binding)?;
    let file_desc = lookup_proc_fd(&leader, entry.fd)?;

    proc_fd_readlink_target(&leader, &file_desc)
}

fn proc_fd_entry_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let entry = proc_fd_entry_private(inode);
    let leader = validate_fd_access(&entry.binding)?;
    let _file_desc = lookup_proc_fd(&leader, entry.fd)?;
    let meta = inode.inode().meta_snapshot();
    let now = Instant::now().to_duration();

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: inode.mode(),
        nlink: inode.nlink(),
        uid: meta.uid,
        gid: meta.gid,
        rdev: DeviceId::None,
        size: 0,
        atime: now,
        mtime: now,
        ctime: now,
    })
}

fn proc_fd_entry_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let entry = proc_fd_entry_private(inode);
    let leader = validate_fd_access(&entry.binding)?;
    let _file_desc = lookup_proc_fd(&leader, entry.fd)?;

    Err(SysError::NotSupported)
}

fn proc_fd_read_dir(
    file: &File,
    pos: &mut usize,
    sink: &mut dyn DirSink,
) -> Result<ReadDirResult, SysError> {
    let dir = proc_fd_dir_private(file.inode());
    let leader = validate_fd_access(&dir.binding)?;
    let snapshot = leader.opened_fd_numbers_snapshot();

    let old_pos = *pos;
    if old_pos >= snapshot.len() {
        return Ok(ReadDirResult::Eof);
    }

    // Existing procfs tgid subdirectories do not emit "." or ".."; keep fd aligned
    // with that cursor model and only enumerate dynamic fd entries.
    for &fd in &snapshot[old_pos..] {
        let (ino, _) = proc_fd_child_ino(dir, fd);
        match sink.push(DirEntry {
            name: fd.raw().to_string(),
            ino,
            ty: InodeType::Symlink,
        })? {
            SinkResult::Accepted => *pos += 1,
            SinkResult::Stop => {
                if *pos == old_pos {
                    return Ok(ReadDirResult::Eof);
                }
                break;
            },
        }
    }

    Ok(ReadDirResult::Progressed)
}

static PROC_FD_DIR_INODE_OPS: InodeOps = InodeOps {
    lookup: proc_fd_lookup,
    touch: |_, _, _| Err(SysError::NotSupported),
    mkdir: |_, _, _| Err(SysError::NotSupported),
    symlink: |_, _, _| Err(SysError::NotSupported),
    link: |_, _, _| Err(SysError::IsDir),
    unlink: |_, _| Err(SysError::IsDir),
    rmdir: |_, _| Err(SysError::NotSupported),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: proc_fd_open,
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: |_| Err(SysError::IsDir),
    get_attr: proc_fd_get_attr,
};

static PROC_FD_ENTRY_INODE_OPS: InodeOps = InodeOps {
    lookup: |_, _| Err(SysError::NotDir),
    touch: |_, _, _| Err(SysError::NotDir),
    mkdir: |_, _, _| Err(SysError::NotDir),
    symlink: |_, _, _| Err(SysError::NotDir),
    link: |_, _, _| Err(SysError::NotDir),
    unlink: |_, _| Err(SysError::NotDir),
    rmdir: |_, _| Err(SysError::NotDir),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: proc_fd_entry_open,
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: proc_fd_entry_read_link,
    get_attr: proc_fd_entry_get_attr,
};

static PROC_FD_DIR_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::IsDir),
    write: |_, _, _, _| Err(SysError::IsDir),
    read_at: |_, _, _, _| Err(SysError::IsDir),
    write_at: |_, _, _, _| Err(SysError::IsDir),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: seek_dir_rewind,
    read_dir: proc_fd_read_dir,
    poll: |_, req| Ok(req.ready_or_unsupported(PollEvent::READABLE & req.interests())),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

pub static TGID_FD_TGID_ENTRY: TgidEntry = TgidEntry {
    name: "fd",
    mode: InodeMode::new(InodeType::Dir, InodePerm::all_rx()),
    inode_ops: &PROC_FD_DIR_INODE_OPS,
    make_prv: make_proc_fd_dir_prv,
};
