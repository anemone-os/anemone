use core::time::Duration;

use crate::{
    arch,
    fs::{inode::Inode, iomux::PollEvent},
    prelude::*,
    utils::any_opaque::{AnyOpaque, NilOpaque},
};

pub(super) struct StaticEntry {
    name: &'static str,
    mode: InodeMode,
    kind: StaticEntryKind,
    ino: MonoOnce<Ino>,
}

#[derive(Clone, Copy)]
enum StaticEntryKind {
    Dir(&'static [&'static StaticEntry]),
    Text(fn() -> String),
}

#[derive(Opaque)]
struct StaticEntryPrivate {
    entry: &'static StaticEntry,
    parent_ino: Ino,
}

fn address_bits() -> String {
    format!("{}\n", arch::address_bits())
}

fn cpu_byteorder() -> String {
    format!("{}\n", arch::cpu_byteorder())
}

static ADDRESS_BITS: StaticEntry = StaticEntry {
    name: "address_bits",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    kind: StaticEntryKind::Text(address_bits),
    ino: unsafe { MonoOnce::new() },
};

static CPU_BYTEORDER: StaticEntry = StaticEntry {
    name: "cpu_byteorder",
    mode: InodeMode::new(InodeType::Regular, InodePerm::all_r()),
    kind: StaticEntryKind::Text(cpu_byteorder),
    ino: unsafe { MonoOnce::new() },
};

static KERNEL_CHILDREN: &[&StaticEntry] = &[&ADDRESS_BITS, &CPU_BYTEORDER];

static KERNEL: StaticEntry = StaticEntry {
    name: "kernel",
    mode: InodeMode::new(InodeType::Dir, InodePerm::all_rx()),
    kind: StaticEntryKind::Dir(KERNEL_CHILDREN),
    ino: unsafe { MonoOnce::new() },
};

static ROOT_CHILDREN: &[&StaticEntry] = &[&KERNEL];

pub(super) static ROOT: StaticEntry = StaticEntry {
    name: "/",
    mode: InodeMode::new(InodeType::Dir, InodePerm::all_rx()),
    kind: StaticEntryKind::Dir(ROOT_CHILDREN),
    ino: unsafe { MonoOnce::new() },
};

fn private(inode: &InodeRef) -> &StaticEntryPrivate {
    let private = inode
        .inode()
        .prv()
        .cast::<StaticEntryPrivate>()
        .expect("sysfs inode without static entry capability");
    assert_eq!(
        inode.ino(),
        *private.entry.ino.get(),
        "sysfs inode bound to the wrong static entry"
    );
    private
}

fn nlink(entry: &StaticEntry) -> u64 {
    match entry.kind {
        StaticEntryKind::Dir(children) => {
            2 + children
                .iter()
                .filter(|child| child.mode.ty() == InodeType::Dir)
                .count() as u64
        },
        StaticEntryKind::Text(_) => 1,
    }
}

fn text(entry: &StaticEntry) -> Option<String> {
    match entry.kind {
        StaticEntryKind::Text(getter) => Some(getter()),
        StaticEntryKind::Dir(_) => None,
    }
}

fn lookup(dir: &InodeRef, name: &str) -> Result<InodeRef, SysError> {
    let children = match private(dir).entry.kind {
        StaticEntryKind::Dir(children) => children,
        StaticEntryKind::Text(_) => return Err(SysError::NotDir),
    };
    let child = children
        .iter()
        .find(|child| child.name == name)
        .ok_or(SysError::NotFound)?;
    dir.sb()
        .try_iget(*child.ino.get())
        .ok_or(SysError::NotFound)
}

fn open(inode: &InodeRef) -> Result<OpenedFile, SysError> {
    let ops = match private(inode).entry.kind {
        StaticEntryKind::Dir(_) => &DIR_FILE_OPS,
        StaticEntryKind::Text(_) => &TEXT_FILE_OPS,
    };
    Ok(OpenedFile::new(ops, NilOpaque::new()))
}

fn get_attr(inode: &InodeRef) -> Result<InodeStat, SysError> {
    let entry = private(inode).entry;
    let meta = inode.inode().meta_snapshot();
    let now = RealtimeInstant::now().to_duration();
    Ok(InodeStat {
        fs_dev: DeviceId::None,
        ino: inode.ino(),
        mode: entry.mode,
        nlink: nlink(entry),
        uid: meta.uid,
        gid: meta.gid,
        rdev: DeviceId::None,
        size: text(entry).map_or(0, |value| value.len() as u64),
        atime: now,
        mtime: now,
        ctime: now,
    })
}

static INODE_OPS: InodeOps = InodeOps {
    make_node: reject_make_node,
    lookup,
    touch: |_, _, _| Err(SysError::PermissionDenied),
    mkdir: |_, _, _| Err(SysError::PermissionDenied),
    symlink: |_, _, _| Err(SysError::PermissionDenied),
    link: |_, _, _| Err(SysError::PermissionDenied),
    unlink: |_, _| Err(SysError::PermissionDenied),
    rmdir: |_, _| Err(SysError::PermissionDenied),
    rename: |_, _, _, _, _| Err(SysError::PermissionDenied),
    open,
    truncate: |_, _| Err(SysError::PermissionDenied),
    read_link: |_| Err(SysError::NotSymlink),
    get_attr,
};

fn push_entry(
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

fn read_dir(
    file: &File,
    pos: &mut usize,
    sink: &mut dyn DirSink,
) -> Result<ReadDirResult, SysError> {
    let private = private(file.inode());
    let children = match private.entry.kind {
        StaticEntryKind::Dir(children) => children,
        StaticEntryKind::Text(_) => return Err(SysError::NotDir),
    };
    let mut progressed = false;
    loop {
        let candidate = match *pos {
            0 => (".", file.inode().ino(), InodeType::Dir),
            1 => ("..", private.parent_ino, InodeType::Dir),
            index => match children.get(index - 2) {
                Some(entry) => (entry.name, *entry.ino.get(), entry.mode.ty()),
                None => break,
            },
        };
        match push_entry(sink, candidate.0, candidate.1, candidate.2)? {
            SinkResult::Accepted => {
                progressed = true;
                *pos += 1;
            },
            SinkResult::Stop => break,
        }
    }
    Ok(if progressed {
        ReadDirResult::Progressed
    } else {
        ReadDirResult::Eof
    })
}

static DIR_FILE_OPS: FileOps = FileOps {
    read: |_, _, _, _| Err(SysError::IsDir),
    write: |_, _, _, _| Err(SysError::IsDir),
    read_at: |_, _, _, _| Err(SysError::IsDir),
    write_at: |_, _, _, _| Err(SysError::IsDir),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek: seek_dir_rewind,
    read_dir,
    poll: |_, request| Ok(request.ready_or_unsupported(PollEvent::READABLE & request.interests())),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

fn read_snapshot(pos: usize, buf: &mut [u8], snapshot: &[u8]) -> usize {
    if pos >= snapshot.len() {
        return 0;
    }
    let count = usize::min(buf.len(), snapshot.len() - pos);
    buf[..count].copy_from_slice(&snapshot[pos..pos + count]);
    count
}

fn read(file: &File, pos: &mut usize, buf: &mut [u8], _ctx: FileIoCtx) -> Result<usize, SysError> {
    let snapshot = text(private(file.inode()).entry).expect("text file lost its getter");
    let count = read_snapshot(*pos, buf, snapshot.as_bytes());
    *pos = pos.checked_add(count).ok_or(SysError::FileTooLarge)?;
    Ok(count)
}

fn read_at(file: &File, pos: usize, buf: &mut [u8], _ctx: FileIoCtx) -> Result<usize, SysError> {
    let snapshot = text(private(file.inode()).entry).expect("text file lost its getter");
    Ok(read_snapshot(pos, buf, snapshot.as_bytes()))
}

fn seek(file: &File, pos: &mut usize, from: SeekFrom) -> Result<usize, SysError> {
    let snapshot = text(private(file.inode()).entry).expect("text file lost its getter");
    seek_with_fixed_size(file, pos, from, snapshot.len())
}

static TEXT_FILE_OPS: FileOps = FileOps {
    read,
    write: |_, _, _, _| Err(SysError::PermissionDenied),
    read_at,
    write_at: |_, _, _, _| Err(SysError::PermissionDenied),
    read_user_at: None,
    write_user_at: None,
    check_status_flags: accept_file_op_status_flags,
    seek,
    read_dir: |_, _, _| Err(SysError::NotDir),
    poll: |_, request| Ok(request.ready_or_unsupported(PollEvent::READABLE & request.interests())),
    fcntl: None,
    ioctl: |_, _| Err(SysError::UnsupportedIoctl),
};

fn validate_name(name: &str, is_root: bool) {
    assert!(
        if is_root {
            name == "/"
        } else {
            !name.is_empty() && name != "." && name != ".." && !name.contains('/')
        },
        "invalid static sysfs entry name: {}",
        name
    );
}

fn validate_entry(entry: &'static StaticEntry, seen: &mut Vec<*const StaticEntry>, is_root: bool) {
    validate_name(entry.name, is_root);
    assert!(
        !seen.contains(&(entry as *const StaticEntry)),
        "static sysfs entry is reused or cyclic: {}",
        entry.name
    );
    seen.push(entry as *const StaticEntry);
    match entry.kind {
        StaticEntryKind::Dir(children) => {
            assert_eq!(entry.mode.ty(), InodeType::Dir);
            assert_eq!(entry.mode.perm(), InodePerm::all_rx());
            for (index, child) in children.iter().enumerate() {
                assert!(
                    !children[..index]
                        .iter()
                        .any(|existing| existing.name == child.name),
                    "duplicate static sysfs entry: {}",
                    child.name
                );
                validate_entry(child, seen, false);
            }
        },
        StaticEntryKind::Text(getter) => {
            assert_eq!(entry.mode.ty(), InodeType::Regular);
            assert_eq!(entry.mode.perm(), InodePerm::all_r());
            let value = getter();
            assert!(value.is_ascii(), "sysfs text attribute must be ASCII");
            assert!(
                value.ends_with('\n'),
                "sysfs text attribute must end in newline"
            );
        },
    }
}

pub(super) fn validate_tree() {
    validate_entry(&ROOT, &mut Vec::new(), true);
}

pub(super) fn seed_tree(sb: &Arc<SuperBlock>) {
    fn seed(
        sb: &Arc<SuperBlock>,
        entry: &'static StaticEntry,
        parent_ino: Ino,
        next_ino: &mut u64,
    ) {
        let ino = if core::ptr::eq(entry, &ROOT) {
            super::ROOT_INO
        } else {
            let ino = Ino::new(*next_ino);
            *next_ino = next_ino
                .checked_add(1)
                .expect("static sysfs inode overflow");
            ino
        };
        entry.ino.init(|slot| {
            slot.write(ino);
        });
        let inode = Inode::new(
            ino,
            entry.mode.ty(),
            &INODE_OPS,
            sb.clone(),
            AnyOpaque::new(StaticEntryPrivate { entry, parent_ino }),
        );
        inode.set_meta(&InodeMeta {
            nlink: nlink(entry),
            size: text(entry).map_or(0, |value| value.len() as u64),
            perm: entry.mode.perm(),
            uid: Uid::ROOT,
            gid: Gid::ROOT,
            atime: Duration::ZERO,
            mtime: Duration::ZERO,
            ctime: Duration::ZERO,
        });
        sb.seed_inode(Arc::new(inode));
        if let StaticEntryKind::Dir(children) = entry.kind {
            for child in children {
                seed(sb, child, ino, next_ino);
            }
        }
    }

    seed(sb, &ROOT, super::ROOT_INO, &mut 2);
}
