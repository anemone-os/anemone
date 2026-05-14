use crate::{
    fs::{
        inode::Inode,
        proc::{
            superblock::alloc_ino,
            tgid::{
                binding::ThreadGroupBinding, cwd::TGID_CWD_TGID_ENTRY,
                environ::TGID_ENVIRON_TGID_ENTRY, exe::TGID_EXE_TGID_ENTRY, inode::TGID_INODE_OPS,
                mounts::TGID_MOUNTS_TGID_ENTRY, root::TGID_ROOT_TGID_ENTRY,
            },
        },
    },
    prelude::*,
    utils::any_opaque::AnyOpaque,
};

#[derive(Debug, Clone, Copy)]
pub struct SubInoRecord {
    pub ino: Ino,
    pub instantiated: bool,
}

#[derive(Debug, Opaque)]
pub struct TgidInodePrivate {
    binding: Arc<ThreadGroupBinding>,
    sub_ino: SpinLock<HashMap<&'static str, SubInoRecord>>,
}

#[inline]
fn tgid_inode_private(inode: &InodeRef) -> &TgidInodePrivate {
    inode.inode().prv().cast::<TgidInodePrivate>().unwrap()
}

#[inline]
fn validate_tgid_inode(inode: &InodeRef) -> Result<Arc<ThreadGroupBinding>, SysError> {
    let prv = tgid_inode_private(inode);
    if !prv.binding.alive() {
        return Err(SysError::NoSuchProcess);
    }
    Ok(prv.binding.clone())
}

#[derive(Debug, Opaque)]
pub struct TgidSubInodePrivate {
    binding: Arc<ThreadGroupBinding>,
}

#[inline]
fn tgid_sub_inode_private(inode: &InodeRef) -> &TgidSubInodePrivate {
    inode.inode().prv().cast::<TgidSubInodePrivate>().unwrap()
}

/// This function must be called before any operation on a <tdig>/** inode.
#[inline]
fn validate_tgid_sub_inode(inode: &InodeRef) -> Result<Arc<ThreadGroupBinding>, SysError> {
    let prv = tgid_sub_inode_private(inode);
    if !prv.binding.alive() {
        return Err(SysError::NoSuchProcess);
    }
    Ok(prv.binding.clone())
}

pub struct TgidEntry {
    pub name: &'static str,
    pub mode: InodeMode,
    pub inode_ops: &'static InodeOps,
}

impl TgidEntry {
    /// If `ino` is `None`, this method will allocate a new inode number.
    pub fn new_inode(
        &self,
        binding: Arc<ThreadGroupBinding>,
        sb: Arc<SuperBlock>,
        ino: Option<Ino>,
    ) -> Inode {
        let inode = Inode::new(
            ino.unwrap_or_else(|| alloc_ino()),
            self.mode.ty(),
            self.inode_ops,
            sb,
            AnyOpaque::new(TgidSubInodePrivate { binding }),
        );

        let now = Instant::now().to_duration();
        inode.set_meta(&InodeMeta {
            nlink: match self.mode.ty() {
                InodeType::Dir => 3, // randomly chosen. doesn't make much sense.
                _ => 1,
            },
            size: 0,
            perm: self.mode.perm(),
            atime: now,
            mtime: now,
            ctime: now,
        });

        inode
    }
}

static TGID_ENTRIES: &[&TgidEntry] = &[
    &TGID_ROOT_TGID_ENTRY,
    &TGID_CWD_TGID_ENTRY,
    &TGID_ENVIRON_TGID_ENTRY,
    &TGID_EXE_TGID_ENTRY,
    &TGID_MOUNTS_TGID_ENTRY,
    // TODO: exe, cmdline, environ, fd, etc.
];

fn find_tgid_entry_by_name(name: &str) -> Option<&'static TgidEntry> {
    TGID_ENTRIES
        .iter()
        .find(|entry| entry.name == name)
        .copied()
}

pub fn new_tgid_dir_inode(
    sb: Arc<SuperBlock>,
    ino: Ino,
    binding: Arc<ThreadGroupBinding>,
) -> Inode {
    let prv = TgidInodePrivate {
        binding,
        sub_ino: SpinLock::new(HashMap::new()),
    };
    let inode = Inode::new(
        ino,
        InodeType::Dir,
        &TGID_INODE_OPS,
        sb,
        AnyOpaque::new(prv),
    );

    let now = Instant::now().to_duration();

    inode.set_meta(&InodeMeta {
        nlink: 3, // randomly chosen. doesn't make much sense.
        size: 0,
        perm: InodePerm::all_rw(),
        atime: now,
        mtime: now,
        ctime: now,
    });

    inode
}

// infra
pub mod binding;
pub mod file;
pub mod inode;

// entries
mod cwd;
mod environ;
mod exe;
mod mounts;
mod root;
