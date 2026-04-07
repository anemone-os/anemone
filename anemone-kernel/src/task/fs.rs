//! File System related structures and functions for a task.
//!
//! Reference:
//! - https://elixir.bootlin.com/linux/v6.6.32/source/include/linux/fs_struct.h

use crate::prelude::*;

// #[derive(Debug)]
// pub struct FsState {
//     root: PathRef,
//     cwd: PathRef,
//     // TODO: umask
// }

#[derive(Debug)]
pub enum FsState {
    Hanging,
    Ready { root: PathRef, cwd: PathRef },
}

impl FsState {
    /// Create a hanging FsState, which is used for kernel threads that do not
    /// have a filesystem context.
    ///
    /// All operations on a hanging FsState will panic, so it should only be
    /// used for kernel threads that do not perform any filesystem operations.
    pub fn new_hanging() -> Self {
        Self::Hanging
    }

    pub fn new(root: PathRef, cwd: PathRef) -> Self {
        Self::Ready { root, cwd }
    }

    pub fn new_root() -> Self {
        Self::Ready {
            root: root_pathref(),
            cwd: root_pathref(),
        }
    }

    pub fn root(&self) -> &PathRef {
        match self {
            Self::Hanging => panic!("FsState is hanging"),
            Self::Ready { root, .. } => root,
        }
    }

    pub fn cwd(&self) -> &PathRef {
        match self {
            Self::Hanging => panic!("FsState is hanging"),
            Self::Ready { cwd, .. } => cwd,
        }
    }

    pub fn set_root(&mut self, root: PathRef) {
        match self {
            Self::Hanging => panic!("FsState is hanging"),
            Self::Ready { root: r, .. } => *r = root,
        }
    }

    pub fn set_cwd(&mut self, cwd: PathRef) {
        match self {
            Self::Hanging => panic!("FsState is hanging"),
            Self::Ready { cwd: c, .. } => *c = cwd,
        }
    }
}

impl Task {
    /// Get the filesystem state of this task.
    pub fn fs_state(&self) -> Arc<RwLock<FsState>> {
        self.fs_state.clone()
    }

    /// Set a brand new filesystem state for this task.
    pub fn set_fs_state(&self, fs_state: FsState) {
        *self.fs_state.write() = fs_state;
    }

    pub fn root(&self) -> PathRef {
        self.fs_state().read().root().clone()
    }

    pub fn cwd(&self) -> PathRef {
        self.fs_state().read().cwd().clone()
    }

    pub fn set_root(&self, root: PathRef) {
        self.fs_state().write().set_root(root);
    }

    pub fn set_cwd(&self, cwd: PathRef) {
        self.fs_state().write().set_cwd(cwd);
    }

    pub fn lookup_path(&self, path: &Path) -> Result<PathRef, FsError> {
        let fs_state = self.fs_state();
        let fs_state = fs_state.read();
        if path.is_absolute() {
            vfs_lookup_from(&fs_state.root(), path)
        } else {
            vfs_lookup_from(&fs_state.cwd(), path)
        }
    }

    /// Get the current working directory of this task, relative to its root.
    pub fn rel_cwd(&self) -> PathBuf {
        let fs_state = self.fs_state();
        let fs_state = fs_state.read();
        let cwd_str = fs_state.cwd().to_pathbuf();
        let root_str = fs_state.root().to_pathbuf();

        if let Ok(rel) = cwd_str.strip_prefix(&root_str) {
            // add '/' back if rel is empty.
            if rel.as_bytes().is_empty() {
                "/".into()
            } else {
                rel.into()
            }
        } else {
            // this may happen as user might deliberately set root to a path that is not an
            // ancestor of cwd. In this case we just return the absolute path of cwd.
            cwd_str.into()
        }
    }

    /// Make a path relative to this task's root and cwd in global namespace.
    ///
    /// If the input path is absolute, it will be resolved relative to this
    /// task's root.
    ///
    /// If the input path is relative, it will be resolved relative to this
    /// task's cwd.
    pub fn make_global_path(&self, path: &Path) -> PathBuf {
        let fs_state = self.fs_state();
        let fs_state = fs_state.read();
        if path.is_absolute() {
            let root = fs_state.root().to_pathbuf();
            root.join(path.strip_prefix("/").unwrap())
        } else {
            let cwd = fs_state.cwd().to_pathbuf();
            cwd.join(path)
        }
    }
}
