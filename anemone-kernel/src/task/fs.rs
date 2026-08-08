//! File System related structures and functions for a task.
//!
//! Reference:
//! - https://elixir.bootlin.com/linux/v6.6.32/source/include/linux/fs_struct.h

use crate::prelude::*;

const _: () = assert!(INITIAL_UMASK <= InodePerm::all_rwx().bits());

#[derive(Debug, Clone)]
pub enum FsState {
    Hanging,
    Ready {
        root: PathRef,
        cwd: PathRef,
        /// The task filesystem context is the single owner of this mask.
        /// Only ordinary rwx bits are stored; special inode bits are never mask
        /// state.
        umask: InodePerm,
    },
}

/// Coherent namespace origins captured for one path operation.
///
/// These `PathRef` clones are stable lifetime capabilities, not a second source
/// of filesystem-context state. The owning `FsState` guard must be released
/// before namei because a backend lookup may take a sleepable lock or perform
/// synchronous I/O.
struct FsPathSnapshot {
    root: PathRef,
    cwd: PathRef,
}

impl FsState {
    fn initial_umask() -> InodePerm {
        InodePerm::from_bits_retain(INITIAL_UMASK)
    }

    /// Create a hanging [FsState], which is used for kernel threads that do not
    /// have a filesystem context.
    ///
    /// All operations on a hanging [FsState] will panic, so it should only be
    /// used for kernel threads that do not perform any filesystem operations.
    pub fn new_hanging() -> Self {
        Self::Hanging
    }

    pub fn new(root: PathRef, cwd: PathRef) -> Self {
        Self::Ready {
            root,
            cwd,
            umask: Self::initial_umask(),
        }
    }

    pub fn new_root() -> Self {
        Self::Ready {
            root: root_pathref(),
            cwd: root_pathref(),
            umask: Self::initial_umask(),
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

    fn replace_umask(&mut self, umask: InodePerm) -> InodePerm {
        let umask = umask & InodePerm::all_rwx();
        match self {
            Self::Hanging => panic!("FsState is hanging"),
            Self::Ready { umask: current, .. } => core::mem::replace(current, umask),
        }
    }

    fn mask_creation_perm(&self, requested: InodePerm) -> InodePerm {
        match self {
            Self::Hanging => panic!("FsState is hanging"),
            Self::Ready { umask, .. } => requested & !*umask,
        }
    }

    fn path_snapshot(&self) -> FsPathSnapshot {
        FsPathSnapshot {
            root: self.root().clone(),
            cwd: self.cwd().clone(),
        }
    }

    /// Currently this implementation is the same as default `clone`. But it's
    /// still necessary to have a separate function to emphasize the semantic of
    /// this operation.
    pub fn fork(&self) -> Self {
        match self {
            Self::Hanging => Self::Hanging,
            Self::Ready { root, cwd, umask } => Self::Ready {
                root: root.clone(),
                cwd: cwd.clone(),
                umask: *umask,
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

    /// Atomically install a new file creation mask and return the previous
    /// mask.
    pub fn replace_umask(&self, umask: InodePerm) -> InodePerm {
        self.fs_state.write().replace_umask(umask)
    }

    /// Apply a snapshot of this filesystem context's umask to a requested mode.
    pub fn mask_creation_perm(&self, requested: InodePerm) -> InodePerm {
        self.fs_state.read().mask_creation_perm(requested)
    }

    fn path_snapshot(&self) -> FsPathSnapshot {
        self.fs_state.read().path_snapshot()
    }

    /// Lookup a path in this task's filesystem context.
    pub fn lookup_path(&self, path: &Path, flags: ResolveFlags) -> Result<PathRef, SysError> {
        let origins = self.path_snapshot();
        let checker = FsPermChecker::new(self.cred());
        resolve_from_with_root_checked(&origins.root, &origins.cwd, path, flags, &checker)
    }

    /// Lookup a path in this task's filesystem context using an explicit
    /// permission checker for directory search checks.
    pub fn lookup_path_with_checker(
        &self,
        path: &Path,
        flags: ResolveFlags,
        checker: &FsPermChecker,
    ) -> Result<PathRef, SysError> {
        let origins = self.path_snapshot();
        resolve_from_with_root_checked(&origins.root, &origins.cwd, path, flags, checker)
    }

    /// Lookup a path in this task's filesystem context, relative to an
    /// explicitly provided starting directory.
    pub fn lookup_path_from(
        &self,
        from: &PathRef,
        path: &Path,
        flags: ResolveFlags,
    ) -> Result<PathRef, SysError> {
        let root = self.root();
        let checker = FsPermChecker::new(self.cred());
        resolve_from_with_root_checked(&root, from, path, flags, &checker)
    }

    /// Lookup a path relative to an explicit starting directory using an
    /// explicit permission checker for directory search checks.
    pub fn lookup_path_from_with_checker(
        &self,
        from: &PathRef,
        path: &Path,
        flags: ResolveFlags,
        checker: &FsPermChecker,
    ) -> Result<PathRef, SysError> {
        let root = self.root();
        resolve_from_with_root_checked(&root, from, path, flags, checker)
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
        let checker = FsPermChecker::new(self.cred());
        self.lookup_parent_path_with_checker(path, flags, &checker)
    }

    /// Look up a parent directory using an explicit credential snapshot.
    pub(crate) fn lookup_parent_path_with_checker(
        &self,
        path: &Path,
        flags: ResolveFlags,
        checker: &FsPermChecker,
    ) -> Result<(PathRef, String), SysError> {
        let origins = self.path_snapshot();
        resolve_parent_from_with_root_checked(&origins.root, &origins.cwd, path, flags, checker)
    }

    /// Lookup the parent directory of a path in this task's filesystem context,
    /// relative to an explicitly provided starting directory.
    pub fn lookup_parent_path_from(
        &self,
        from: &PathRef,
        path: &Path,
        flags: ResolveFlags,
    ) -> Result<(PathRef, String), SysError> {
        let checker = FsPermChecker::new(self.cred());
        self.lookup_parent_path_from_with_checker(from, path, flags, &checker)
    }

    /// Look up a parent directory from an explicit start using an explicit
    /// credential snapshot.
    pub(crate) fn lookup_parent_path_from_with_checker(
        &self,
        from: &PathRef,
        path: &Path,
        flags: ResolveFlags,
        checker: &FsPermChecker,
    ) -> Result<(PathRef, String), SysError> {
        let root = self.root();
        resolve_parent_from_with_root_checked(&root, from, path, flags, checker)
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
                PathBuf::from("/").join(rel)
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

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn perm(bits: u16) -> InodePerm {
        InodePerm::from_bits(bits).unwrap()
    }

    fn fs_state() -> FsState {
        let root = root_pathref();
        FsState::new(root.clone(), root)
    }

    #[kunit]
    fn test_umask_masks_creation_permissions_and_preserves_special_bits() {
        let mut state = fs_state();
        assert_eq!(state.replace_umask(perm(0o022)).bits(), INITIAL_UMASK);
        assert_eq!(state.mask_creation_perm(perm(0o666)).bits(), 0o644);
        assert_eq!(state.mask_creation_perm(perm(0o777)).bits(), 0o755);
        assert_eq!(state.mask_creation_perm(perm(0o600)).bits(), 0o600);

        state.replace_umask(InodePerm::all_rwx() | InodePerm::ISVTX);
        assert_eq!(
            state
                .mask_creation_perm(InodePerm::all_rwx() | InodePerm::ISVTX)
                .bits(),
            InodePerm::ISVTX.bits()
        );
    }

    #[kunit]
    fn test_fork_copies_umask_without_sharing_mutations() {
        let mut parent = fs_state();
        parent.replace_umask(perm(0o022));
        let mut child = parent.fork();

        child.replace_umask(perm(0o077));

        assert_eq!(parent.mask_creation_perm(perm(0o777)).bits(), 0o755);
        assert_eq!(child.mask_creation_perm(perm(0o777)).bits(), 0o700);
    }

    #[kunit]
    fn test_shared_fs_state_observes_umask_mutations() {
        let shared = Arc::new(RwLock::new(fs_state()));
        let peer = shared.clone();

        shared.write().replace_umask(perm(0o027));

        assert_eq!(peer.read().mask_creation_perm(perm(0o777)).bits(), 0o750);
    }
}
