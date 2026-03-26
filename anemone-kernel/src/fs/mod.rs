//! Virtual file system and filesystem drivers.

// vfs infrastructure
pub mod dentry;
pub mod error;
pub mod file;
pub mod filesystem;
pub mod inode;
pub mod mount;
pub mod namei;
pub mod path;
pub mod superblock;

// filesystem drivers
pub mod devfs;
pub mod ramfs;

mod namespace {
    use crate::prelude::*;

    pub struct NameSpace {
        root_dentry: Option<Arc<Dentry>>,
        // TODO: hashmap to speed up lookups
    }

    impl NameSpace {
        pub fn new() -> Self {
            Self { root_dentry: None }
        }
    }
}

// We prefer gathering all public APIs in this module, and keep the global state
// hidden in a singleton struct, which helps a lot to ensure lock ordering.
mod vfs {
    use crate::{fs::namespace::NameSpace, prelude::*};

    /// Virtual file system. Singleton instance.
    ///
    /// **LOCK ORDERING:**
    /// **`name_space` `fs_list` → `mounts` → `root_mount`**
    struct VfsSubSys {
        namespace: RwLock<NameSpace>,
        fs_list: RwLock<Vec<Arc<FileSystem>>>,
        mounts: RwLock<Vec<Arc<Mount>>>,
        root_mount: RwLock<Option<Arc<Mount>>>,
    }

    static VFS: Lazy<VfsSubSys> = Lazy::new(|| VfsSubSys {
        namespace: RwLock::new(NameSpace::new()),
        fs_list: RwLock::new(Vec::new()),
        mounts: RwLock::new(Vec::new()),
        root_mount: RwLock::new(None),
    });

    /// Register a file system type.
    ///
    /// On success, returns an `Arc` to the registered `FileSystem`.
    pub fn register_filesystem(fs: &'static FileSystemOps) -> Result<Arc<FileSystem>, FsError> {
        let mut fs_list = VFS.fs_list.write_irqsave();
        for existing in fs_list.iter() {
            if existing.name() == fs.name {
                return Err(FsError::AlreadyExists);
            }
        }
        kinfoln!("registered filesystem: {}", fs.name);
        let fs = Arc::new(FileSystem::new(fs));
        fs_list.push(fs.clone());

        Ok(fs)
    }

    /// Retrieve a file system type by name.
    pub fn get_filesystem(name: &str) -> Option<Arc<FileSystem>> {
        let fs_list = VFS.fs_list.read_irqsave();
        for fs in fs_list.iter() {
            if fs.name() == name {
                return Some(fs.clone());
            }
        }
        None
    }

    /// Mount a filesystem and register the mount in the global mount list.
    ///
    /// If no root mount exists yet, the new mount becomes the root mount.
    pub fn mount(
        fs_name: &str,
        source: &MountSource,
        flags: MountFlags,
        mountpoint: Option<&PathRef>,
    ) -> Result<(), FsError> {
        let fs = get_filesystem(fs_name).ok_or(FsError::NotFound)?;

        let (parent, mp_dentry) = match mountpoint {
            Some(pr) => (Some(pr.mount().clone()), Some(pr.dentry().clone())),
            None => (None, None),
        };

        if parent.is_none() && VFS.root_mount.read_irqsave().is_some() {
            // Mounting a new root when one already exists is not allowed.
            return Err(FsError::AlreadyExists);
        }

        let MountedFileSystem { sb, root_ino } = fs.mount(source, flags)?;

        let root_inode = sb.iget(root_ino).expect("root inode must be loadable");
        let root_dentry = Arc::new(Dentry::new("/".to_string(), None, root_inode));

        let mnt = Arc::new(Mount::new(
            root_dentry,
            sb,
            parent.as_ref(),
            mp_dentry.as_ref(),
            flags,
        ));

        VFS.mounts.write_irqsave().push(mnt.clone());

        if let Some(parent) = parent {
            parent.add_child(&mnt);
        }

        {
            let mut root = VFS.root_mount.write_irqsave();
            if root.is_none() {
                *root = Some(mnt);
            }
        }

        Ok(())
    }

    /// Get the root [PathRef].
    ///
    /// # Panics
    ///
    /// Panics if the root mount has not been established yet. This should never
    /// happen after the initial filesystem has been mounted during boot.
    pub fn root_pathref() -> PathRef {
        let root = VFS.root_mount.read_irqsave();
        root.as_ref()
            .map(|m| PathRef::new(m.clone(), m.root().clone()))
            .expect("root mount must be established")
    }
}
pub use vfs::*;

mod vfs_ops {
    use crate::{
        fs::namei::{resolve, resolve_parent},
        prelude::*,
    };

    pub fn vfs_lookup(path: &PathBuf) -> Result<PathRef, FsError> {
        resolve(path)
    }

    pub fn vfs_open(path: &PathBuf) -> Result<File, FsError> {
        let pathref = resolve(path)?;
        let inode = pathref.inode();
        let OpenedFile { file_ops, prv } = inode.open()?;

        Ok(File::new(pathref, file_ops, prv))
    }

    pub fn vfs_mkdir(path: &PathBuf) -> Result<PathRef, FsError> {
        let (parent, name) = resolve_parent(path)?;

        let inode = parent.inode().mkdir(&name)?;
        Ok(PathRef::new(
            parent.mount().clone(),
            Arc::new(Dentry::new(name, Some(parent.dentry()), inode)),
        ))
    }

    pub fn vfs_link(old_path: &PathBuf, new_path: &PathBuf) -> Result<(), FsError> {
        let target = resolve(old_path)?;
        let (parent, name) = resolve_parent(new_path)?;
        parent.inode().link(&name, target.inode())
    }

    pub fn vfs_unlink(path: &PathBuf) -> Result<(), FsError> {
        let (parent, name) = resolve_parent(path)?;
        parent.inode().unlink(&name)
    }

    pub fn vfs_rmdir(path: &PathBuf) -> Result<(), FsError> {
        let (parent, name) = resolve_parent(path)?;
        parent.inode().rmdir(&name)
    }
}
pub use vfs_ops::*;

use crate::initcall::{InitCallLevel, run_initcalls};

pub fn init() {
    unsafe {
        run_initcalls(InitCallLevel::Fs);
    }
}
