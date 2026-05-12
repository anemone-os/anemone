//! File System related structures and functions for a task.
//!
//! Reference:
//! - https://elixir.bootlin.com/linux/v6.6.32/source/include/linux/fs_struct.h

use crate::prelude::*;

#[derive(Debug, Clone)]
pub enum FsState {
    Hanging,
    Ready { root: PathRef, cwd: PathRef },
}

impl FsState {
    /// Create a hanging [FsState], which is used for kernel threads that do not
    /// have a filesystem context.
    ///
    /// All operations on a hanging [FsState] will panic, so it should only be
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

    /// Currently this implementation is the same as default `clone`. But it's
    /// still necessary to have a separate function to emphasize the semantic of
    /// this operation.
    pub fn fork(&self) -> Self {
        match self {
            Self::Hanging => Self::Hanging,
            Self::Ready { root, cwd } => Self::Ready {
                root: root.clone(),
                cwd: cwd.clone(),
            },
        }
    }
}

impl Task {
    /// Get the filesystem state of this task.
    pub fn fs_state(&self) -> Arc<RwLock<FsState>> {
        self.fs_state.clone()
    }

    /// Replace the contents of the current filesystem state object.
    ///
    /// If this task is sharing the same fs state handle with other tasks, they
    /// will observe the updated contents as well.
    ///
    /// Note the semantic difference between this function and
    /// [`Self::replace_fs_state_handle`].
    pub fn set_fs_state(&self, fs_state: FsState) {
        *self.fs_state.write() = fs_state;
    }

    /// Replace the shared filesystem state handle.
    ///
    /// This should only be used while the task is still uniquely owned, such
    /// as during task construction or clone setup.
    pub fn replace_fs_state_handle(&mut self, fs_state: Arc<RwLock<FsState>>) {
        self.fs_state = fs_state;
    }

    pub fn root(&self) -> PathRef {
        self.fs_state.read().root().clone()
    }

    pub fn cwd(&self) -> PathRef {
        self.fs_state.read().cwd().clone()
    }

    pub fn set_root(&self, root: PathRef) {
        self.fs_state.write().set_root(root);
    }

    pub fn set_cwd(&self, cwd: PathRef) {
        self.fs_state.write().set_cwd(cwd);
    }

    /// Lookup a path in this task's filesystem context.
    pub fn lookup_path(&self, path: &Path, flags: ResolveFlags) -> Result<PathRef, SysError> {
        let fs_state = self.fs_state.read();
        resolve_from_with_root(fs_state.root(), fs_state.cwd(), path, flags)
    }

    /// Lookup a path in this task's filesystem context, relative to an
    /// explicitly provided starting directory.
    pub fn lookup_path_from(
        &self,
        from: &PathRef,
        path: &Path,
        flags: ResolveFlags,
    ) -> Result<PathRef, SysError> {
        let fs_state = self.fs_state.read();
        resolve_from_with_root(fs_state.root(), from, path, flags)
    }

    /// Lookup the parent directory of a path in this task's filesystem context,
    /// and return the parent directory and the final component separately.
    ///
    /// flags will be applied to parent directory lookup, but not the final
    /// component, since final component may not exist and we're to create it.
    pub fn lookup_parent_path(
        &self,
        path: &Path,
        flags: ResolveFlags,
    ) -> Result<(PathRef, String), SysError> {
        let fs_state = self.fs_state.read();
        resolve_parent_from_with_root(fs_state.root(), fs_state.cwd(), path, flags)
    }

    /// Lookup the parent directory of a path in this task's filesystem context,
    /// relative to an explicitly provided starting directory.
    pub fn lookup_parent_path_from(
        &self,
        from: &PathRef,
        path: &Path,
        flags: ResolveFlags,
    ) -> Result<(PathRef, String), SysError> {
        let fs_state = self.fs_state.read();
        resolve_parent_from_with_root(fs_state.root(), from, path, flags)
    }

    /// Get the current working directory of this task, relative to its root.
    pub fn rel_cwd(&self) -> PathBuf {
        let fs_state = self.fs_state.read();
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

    /// Turn a absolute path in global namespace into a absolute path relative
    /// to this task's root.
    ///
    /// If [None] is returned, it means the path is not under this task's root,
    /// and thus cannot be made relative.
    ///
    /// Panics if the input path is not absolute.
    pub fn rel_abs_path(&self, path: &Path) -> Option<PathBuf> {
        if path.is_relative() {
            panic!(
                "rel_abs_path: expected absolute path, got relative path '{}'",
                path.display()
            );
        }

        let fs_state = self.fs_state.read();
        let root = fs_state.root().to_pathbuf();
        let path = path.to_path_buf();

        if let Ok(rel) = path.strip_prefix(&root) {
            // a '/' must be added back whether or not rel is empty.
            Some(PathBuf::from("/").join(rel))
        } else {
            kdebugln!(
                "failed to make path '{}' relative to root '{}'",
                root.display(),
                path.display()
            );
            None
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
