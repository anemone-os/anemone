//! Anonymous file helpers.
//!
//! The anonymous namespace currently exists only to provide a dedicated
//! superblock and unique inode identity for kernel-internal files. We create a
//! fresh inode and immediately wrap it in a detached [PathRef]; there is no
//! lookup-based organization here.

mod anony_fs;

use crate::{fs::inode::Inode, prelude::*, utils::any_opaque::AnyOpaque};

static ANONY_FS: MonoOnce<Arc<FileSystem>> = unsafe { MonoOnce::new() };

fn alloc_anony_ino() -> Result<Ino, SysError> {
    static ANONY_INODE_COUNTER: AtomicU64 = AtomicU64::new(2);

    // classic CAS loop to avoid overflow (which is very unlikely tho)
    loop {
        let ino = ANONY_INODE_COUNTER.load(Ordering::Acquire);
        if ino == u64::MAX {
            return Err(SysError::NoSpace);
        }
        if ANONY_INODE_COUNTER
            .compare_exchange(ino, ino + 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Ok(Ino::try_from(ino).unwrap());
        }
    }
}

/// Create a new anonymous inode and return it as a detached [PathRef].
///
/// The resulting path is only used as the identity container required by
/// [File]. It is not inserted into any lookup structure.
pub fn anony_new_inode(
    ty: InodeType,
    ops: &'static InodeOps,
    prv: AnyOpaque,
) -> Result<PathRef, SysError> {
    if ty == InodeType::Dir {
        return Err(SysError::InvalidArgument);
    }

    let ino = alloc_anony_ino()?;

    let root = anonymous_root_pathref();
    let sb = root.mount().sb();

    let inode = Arc::new(Inode::new(ino, ty, ops, sb.clone(), prv));

    let meta = InodeMeta {
        nlink: 1,
        size: 0,
        perm: InodePerm::all_rwx(),
        atime: Duration::ZERO,
        mtime: Duration::ZERO,
        ctime: Duration::ZERO,
    };
    inode.set_meta(&meta);

    let inode = sb.seed_inode(inode);

    let dentry = Dentry::new(
        format!("anony_{}", ino.get()),
        Some(root.dentry().clone()),
        inode.clone(),
    );

    Ok(PathRef::new(root.mount().clone(), Arc::new(dentry)))
}

/// Internally this calls [InodeRef::open] to get file ops and private data.
///
/// For a more flexible version that takes an explicitly constructed
/// [OpenedFile], see [anony_open_with].
pub fn anony_open(path: &PathRef) -> Result<File, SysError> {
    let OpenedFile { file_ops, prv } = path.inode().open()?;

    Ok(File::new(path.clone(), file_ops, prv))
}

/// Create a [File] from an explicitly constructed [OpenedFile]. This is useful
/// for those cases where the file ops and private data are not directly derived
/// from the inode, e.g. when creating a pipe file which may be either a read
/// end or a write end, and thus has different file ops and private data.
pub fn anony_open_with(path: &PathRef, state: OpenedFile) -> Result<File, SysError> {
    let OpenedFile { file_ops, prv } = state;

    Ok(File::new(path.clone(), file_ops, prv))
}
