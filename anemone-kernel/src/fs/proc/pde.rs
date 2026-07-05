//! Roughly the `struct proc_dir_entry` in Linux.

use crate::{
    fs::{
        inode::Inode,
        iomux::PollEvent,
        proc::{
            celf::PROC_SELF_DIR_ENTRY, meminfo::PROC_MEMINFO_DIR_ENTRY,
            mounts::PROC_MOUNTS_DIR_ENTRY, procfs_sb, read_snapshot_at, root::PROC_ROOT_INO,
            superblock::alloc_ino, sys::PROC_SYS_DIR_ENTRY, uptime::PROC_UPTIME_DIR_ENTRY,
        },
    },
    prelude::*,
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

pub struct ProcDirEntry {
    pub name: &'static str,
    pub mode: InodeMode,
    pub kind: ProcDirEntryKind,
    /// Pseudo inodes should always leave this field [MonoOnce::new], and real
    /// inode numbers will be allocated during probe initcall and stored here.
    pub ino: MonoOnce<Ino>,
}

#[derive(Clone, Copy)]
pub enum ProcDirEntryKind {
    Dir(&'static [&'static ProcDirEntry]),
    File(&'static ProcFileEntryOps),
    Symlink(&'static ProcSymlinkEntryOps),
    Custom(&'static InodeOps),
}

pub struct ProcFileEntryOps {
    pub read: fn() -> String,
    pub write: Option<fn(pos: &mut usize, buf: &[u8], ctx: FileIoCtx) -> Result<usize, SysError>>,
    pub write_at: Option<fn(pos: usize, buf: &[u8], ctx: FileIoCtx) -> Result<usize, SysError>>,
}

pub struct ProcSymlinkEntryOps {
    pub target: fn() -> PathBuf,
}

#[derive(Opaque)]
struct ProcDirEntryPrivate {
    /// This is the owning procfs PDE for a seeded static inode. Generic PDE
    /// ops derive behavior only from this pointer; `ino` is asserted as the
    /// icache identity witness, not used as a reverse lookup key.
    pde: &'static ProcDirEntry,
    /// Stable parent identity captured while seeding the static tree. It is
    /// only used for the `..` dirent inode number; topology behavior still
    /// comes from the `Dir` children in `pde.kind`.
    parent_ino: Ino,
}

fn pde_inode_private(inode: &InodeRef) -> &ProcDirEntryPrivate {
    let prv = inode
        .inode()
        .prv()
        .cast::<ProcDirEntryPrivate>()
        .expect("procfs pde inode without ProcDirEntry private data");
    assert!(
        inode.ino() == *prv.pde.ino.get(),
        "procfs pde inode private data bound to wrong inode"
    );
    prv
}

fn pde_nlink(pde: &'static ProcDirEntry) -> u64 {
    match pde.kind {
        ProcDirEntryKind::Dir(children) => {
            2 + children
                .iter()
                .filter(|child| child.mode.ty() == InodeType::Dir)
                .count() as u64
        },
        _ => 1,
    }
}

fn find_pde_child(
    children: &'static [&'static ProcDirEntry],
    name: &str,
) -> Option<&'static ProcDirEntry> {
    children.iter().find(|entry| entry.name == name).copied()
}

fn proc_pde_lookup(dir: &InodeRef, name: &str) -> Result<InodeRef, SysError> {
    let children = match pde_inode_private(dir).pde.kind {
        ProcDirEntryKind::Dir(children) => children,
        _ => return Err(SysError::NotDir),
    };
    let child = find_pde_child(children, name).ok_or(SysError::NotFound)?;

    dir.sb()
        .try_iget(*child.ino.get())
        .ok_or(SysError::NotFound)
}

fn proc_pde_open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let pde = pde_inode_private(inode).pde;

    match pde.kind {
        ProcDirEntryKind::Dir(_) => Ok(OpenedFile::new(&PROC_PDE_DIR_FILE_OPS, NilOpaque::new())),
        ProcDirEntryKind::File(_) => Ok(OpenedFile::new(&PROC_PDE_FILE_OPS, NilOpaque::new())),
        ProcDirEntryKind::Symlink(_) => Err(SysError::IsDir),
        ProcDirEntryKind::Custom(_) => unreachable!("custom pde uses its own inode ops"),
    }
}

fn proc_pde_read_link(inode: &InodeRef) -> Result<PathBuf, SysError> {
    match pde_inode_private(inode).pde.kind {
        ProcDirEntryKind::Symlink(ops) => Ok((ops.target)()),
        _ => Err(SysError::NotSymlink),
    }
}

fn proc_pde_get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let pde = pde_inode_private(inode).pde;
    let meta = inode.inode().meta_snapshot();
    let now = Instant::now().to_duration();

    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: pde.mode,
        nlink: pde_nlink(pde),
        uid: meta.uid,
        gid: meta.gid,
        rdev: DeviceId::None,
        size: 0,
        atime: now,
        mtime: now,
        ctime: now,
    })
}

static PROC_PDE_INODE_OPS: InodeOps = InodeOps {
    lookup: proc_pde_lookup,
    touch: |_, _, _| Err(SysError::NotSupported),
    mkdir: |_, _, _| Err(SysError::NotSupported),
    symlink: |_, _, _| Err(SysError::NotSupported),
    link: |_, _, _| Err(SysError::NotSupported),
    unlink: |_, _| Err(SysError::NotSupported),
    rmdir: |_, _| Err(SysError::NotSupported),
    rename: |_, _, _, _, _| Err(SysError::NotSupported),
    open: proc_pde_open,
    truncate: |_, _| Err(SysError::NotSupported),
    read_link: proc_pde_read_link,
    get_attr: proc_pde_get_attr,
};

fn push_pde_dir_entry(
    sink: &mut dyn DirSink,
    name: &str,
    ino: Ino,
    ty: InodeType,
) -> Result<SinkResult, SysError> {
    sink.push(DirEntry {
        name: name.to_string(),
        ino,
        ty,
    })
}

fn push_pde_entry(
    sink: &mut dyn DirSink,
    pde: &'static ProcDirEntry,
) -> Result<SinkResult, SysError> {
    push_pde_dir_entry(sink, pde.name, *pde.ino.get(), pde.mode.ty())
}

fn pde_read_static_dir(
    pos: &mut usize,
    sink: &mut dyn DirSink,
    self_ino: Ino,
    parent_ino: Ino,
    children: &'static [&'static ProcDirEntry],
) -> Result<ReadDirResult, SysError> {
    let mut pushed_any = false;

    loop {
        match *pos {
            0 => match push_pde_dir_entry(sink, ".", self_ino, InodeType::Dir)? {
                SinkResult::Accepted => {
                    pushed_any = true;
                    *pos = 1;
                },
                SinkResult::Stop => {
                    return Ok(if pushed_any {
                        ReadDirResult::Progressed
                    } else {
                        ReadDirResult::Eof
                    });
                },
            },
            1 => match push_pde_dir_entry(sink, "..", parent_ino, InodeType::Dir)? {
                SinkResult::Accepted => {
                    pushed_any = true;
                    *pos = 2;
                },
                SinkResult::Stop => {
                    return Ok(if pushed_any {
                        ReadDirResult::Progressed
                    } else {
                        ReadDirResult::Eof
                    });
                },
            },
            _ => break,
        }
    }

    while *pos - 2 < children.len() {
        let child = children[*pos - 2];
        match push_pde_entry(sink, child)? {
            SinkResult::Accepted => {
                pushed_any = true;
                *pos += 1;
            },
            SinkResult::Stop => {
                return Ok(if pushed_any {
                    ReadDirResult::Progressed
                } else {
                    ReadDirResult::Eof
                });
            },
        }
    }

    Ok(if pushed_any {
        ReadDirResult::Progressed
    } else {
        ReadDirResult::Eof
    })
}

pub fn read_pde_root_entries(
    pos: &mut usize,
    sink: &mut dyn DirSink,
) -> Result<ReadDirResult, SysError> {
    pde_read_static_dir(
        pos,
        sink,
        PROC_ROOT_INO,
        PROC_ROOT_INO,
        PROC_ROOT_DIR_ENTRIES,
    )
}

fn proc_pde_read_dir(
    file: &File,
    pos: &mut usize,
    sink: &mut dyn DirSink,
) -> Result<ReadDirResult, SysError> {
    let prv = pde_inode_private(file.inode());
    let children = match prv.pde.kind {
        ProcDirEntryKind::Dir(children) => children,
        _ => unreachable!("non-directory procfs pde opened with directory file ops"),
    };

    pde_read_static_dir(pos, sink, file.inode().ino(), prv.parent_ino, children)
}

static PROC_PDE_DIR_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::IsDir),
    write: |_, _, _, _| Err(SysError::IsDir),
    read_at: |_, _, _, _| Err(SysError::IsDir),
    write_at: |_, _, _, _| Err(SysError::IsDir),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: seek_dir_rewind,
    read_dir: proc_pde_read_dir,
    poll: |_, req| Ok(req.ready_or_unsupported(PollEvent::READABLE & req.interests())),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

fn proc_pde_file_ops(file: &File) -> &'static ProcFileEntryOps {
    match pde_inode_private(file.inode()).pde.kind {
        ProcDirEntryKind::File(ops) => ops,
        _ => unreachable!("non-file procfs pde opened with regular file ops"),
    }
}

fn proc_pde_file_read(
    file: &File,
    pos: &mut usize,
    buf: &mut [u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let data = ((proc_pde_file_ops(file)).read)();
    let read = read_snapshot_at(*pos, buf, data.as_bytes())?;
    *pos += read;

    Ok(read)
}

fn proc_pde_file_read_at(
    file: &File,
    pos: usize,
    buf: &mut [u8],
    _ctx: FileIoCtx,
) -> Result<usize, SysError> {
    let data = ((proc_pde_file_ops(file)).read)();
    read_snapshot_at(pos, buf, data.as_bytes())
}

fn proc_pde_file_write(
    file: &File,
    pos: &mut usize,
    buf: &[u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    if let Some(write) = (proc_pde_file_ops(file)).write {
        return write(pos, buf, ctx);
    }

    Err(SysError::NotSupported)
}

fn proc_pde_file_write_at(
    file: &File,
    pos: usize,
    buf: &[u8],
    ctx: FileIoCtx,
) -> Result<usize, SysError> {
    if let Some(write_at) = (proc_pde_file_ops(file)).write_at {
        return write_at(pos, buf, ctx);
    }

    Err(SysError::NotSupported)
}

fn proc_pde_file_seek(file: &File, pos: &mut usize, from: SeekFrom) -> Result<usize, SysError> {
    let data = ((proc_pde_file_ops(file)).read)();
    seek_with_bounded_size(file, pos, from, data.as_bytes().len())
}

static PROC_PDE_FILE_OPS: FileOps = FileOps {
    read: proc_pde_file_read,
    write: proc_pde_file_write,
    read_at: proc_pde_file_read_at,
    write_at: proc_pde_file_write_at,
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: proc_pde_file_seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, req| Ok(req.ready_or_unsupported(PollEvent::READABLE & req.interests())),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

static PROC_ROOT_DIR_ENTRIES: &[&ProcDirEntry] = &[
    &PROC_UPTIME_DIR_ENTRY,
    &PROC_SELF_DIR_ENTRY,
    &PROC_MOUNTS_DIR_ENTRY,
    &PROC_MEMINFO_DIR_ENTRY,
    &PROC_SYS_DIR_ENTRY,
    // TODO: mounts, interrupts, version, devices, kallsyms, etc.
];

pub fn proc_root_dir_entries() -> &'static [&'static ProcDirEntry] {
    PROC_ROOT_DIR_ENTRIES
}

pub fn find_proc_root_pde_by_name(name: &str) -> Option<&'static ProcDirEntry> {
    find_pde_child(PROC_ROOT_DIR_ENTRIES, name)
}

fn seed_pde_tree(sb: &Arc<SuperBlock>, pde: &'static ProcDirEntry, parent_ino: Ino) {
    match pde.kind {
        ProcDirEntryKind::Dir(_) => assert!(
            pde.mode.ty() == InodeType::Dir,
            "procfs pde {} mode {:?} does not match entry kind",
            pde.name,
            pde.mode.ty()
        ),
        ProcDirEntryKind::File(_) => assert!(
            pde.mode.ty() == InodeType::Regular,
            "procfs pde {} mode {:?} does not match entry kind",
            pde.name,
            pde.mode.ty()
        ),
        ProcDirEntryKind::Symlink(_) => assert!(
            pde.mode.ty() == InodeType::Symlink,
            "procfs pde {} mode {:?} does not match entry kind",
            pde.name,
            pde.mode.ty()
        ),
        ProcDirEntryKind::Custom(_) => {},
    }

    let ino = alloc_ino();
    let inode = Inode::new(
        ino,
        pde.mode.ty(),
        match pde.kind {
            ProcDirEntryKind::Custom(ops) => ops,
            _ => &PROC_PDE_INODE_OPS,
        },
        sb.clone(),
        AnyOpaque::new(ProcDirEntryPrivate { pde, parent_ino }),
    );
    inode.set_meta(&InodeMeta {
        nlink: pde_nlink(pde),
        size: 0,
        perm: pde.mode.perm(),
        uid: Uid::ROOT,
        gid: Gid::ROOT,
        atime: Instant::ZERO.to_duration(),
        mtime: Instant::ZERO.to_duration(),
        ctime: Instant::ZERO.to_duration(),
    });
    pde.ino.init(|slot| {
        slot.write(inode.ino());
    });
    let inode = sb.seed_inode(Arc::new(inode));

    kdebugln!(
        "procfs: registered pde {} with ino {}",
        pde.name,
        inode.ino()
    );

    if let ProcDirEntryKind::Dir(children) = pde.kind {
        for child in children {
            seed_pde_tree(sb, child, inode.ino());
        }
    }
}

// TODO: create a new `late_init` for this?
#[initcall(probe)]
fn init() {
    let sb = procfs_sb();

    for &pde in PROC_ROOT_DIR_ENTRIES {
        seed_pde_tree(&sb, pde, PROC_ROOT_INO);
    }
}
