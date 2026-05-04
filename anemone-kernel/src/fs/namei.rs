//! Path resolution and name lookup.

use crate::{fs::root_pathref, prelude::*};
use typed_path::UnixComponent;

bitflags! {
    /// Flags to control path resolution behavior.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct ResolveFlags: u32 {
        /// If set, encountering a symlink during resolution results in an error.
        const DENY_SYMLINKS = 1 << 0;
        /// If set, encountering a symlink as the last component of the path
        /// results in an error.
        const DENY_LAST_SYMLINK = 1 << 1;
        /// If set, the last component of the path is not followed if it is a symlink.
        const UNFOLLOW_LAST_SYMLINK = 1 << 2;
    }
}

impl ResolveFlags {
    /// Remove those flags that control the behavior of the last path component.
    pub fn remove_last_symlink_flags(self) -> Self {
        self & !(Self::DENY_LAST_SYMLINK | Self::UNFOLLOW_LAST_SYMLINK)
    }
}

/// Materialize a child dentry under `parent`.
///
/// This is the only entry point that should instantiate a visible namespace
/// child dentry from an existing `(parent, name, inode)` triple. If a live
/// dentry is already cached under the same parent/name, it is reused.
///
/// **This way, we ensure the uniqueness of dentries within a namespace.**
pub(super) fn materialize_child_dentry(
    parent: &Arc<Dentry>,
    name: &str,
    inode: InodeRef,
) -> Result<Arc<Dentry>, SysError> {
    if let Ok(child) = parent.lookup_child(name) {
        return Ok(child);
    }

    let child_name = name.to_string();
    let dentry = Arc::new(Dentry::new(child_name.clone(), Some(parent.clone()), inode));

    match parent.insert_child(child_name, &dentry) {
        Ok(()) => Ok(dentry),
        Err(SysError::AlreadyExists) => parent.lookup_child(name),
        Err(err) => Err(err),
    }
}

#[derive(Debug)]
enum PendingComponent {
    CurDir,
    ParentDir,
    Normal(String),
}

/// Resolve a path to a [PathRef], starting from the root of the namespace.
///
/// Starts from global root, regardless of the current task's root or cwd.
///
/// See [resolve_from_with_root] for more details.
pub fn resolve(path: &Path, flags: ResolveFlags) -> Result<PathRef, SysError> {
    if path.components().next().is_none() {
        // empty path
        return Err(SysError::InvalidArgument);
    }

    let root = root_pathref();
    resolve_from_with_root(&root, &root, path, flags)
}

/// Resolve a path to a [PathRef], starting from a given [PathRef].
///
/// If `path` is absolute, `from` is ignored and resolution starts from the
/// root of the namespace.
///
/// Starts from global root, regardless of the current task's root or cwd.
///
/// See [resolve_from_with_root] for more details.
pub fn resolve_from(from: &PathRef, path: &Path, flags: ResolveFlags) -> Result<PathRef, SysError> {
    let root = root_pathref();
    resolve_from_with_root(&root, from, path, flags)
}

/// Resolve a path to a [PathRef] with an explicit logical root.
///
/// If `path` is absolute, `from` is ignored and resolution starts from the
/// provided `root`.
///
/// Absolute input paths, and absolute symlink targets encountered during
/// resolution, are both interpreted relative to `root`.
pub fn resolve_from_with_root(
    root: &PathRef,
    from: &PathRef,
    path: &Path,
    flags: ResolveFlags,
) -> Result<PathRef, SysError> {
    if path.components().next().is_none() {
        // empty path
        return Err(SysError::InvalidArgument);
    }

    let pending = collect_components(path)?;
    let cur_path = if path.is_absolute() {
        root.clone()
    } else {
        from.clone()
    };

    resolve_components(root.clone(), cur_path, pending, flags)
}

/// Resolve the parent directory of a path, returning the parent [PathRef] and
/// the final component as a string.
///
/// This does not make any guarantee on the existence of the final component,
/// and it is the caller's responsibility to check it if necessary.
///
/// Starts from global root, regardless of the current task's root or cwd.
///
/// See [resolve_parent_from_with_root] for more details.
pub fn resolve_parent(path: &Path, flags: ResolveFlags) -> Result<(PathRef, String), SysError> {
    if path.components().next().is_none() {
        // empty path
        return Err(SysError::InvalidArgument);
    }

    let root = root_pathref();
    resolve_parent_from_with_root(&root, &root, path, flags)
}

/// Resolve the parent directory of a path, starting from a given [PathRef], and
/// returning the parent [PathRef] and the final component as a string.
///
/// If `path` is absolute, `from` is ignored and resolution starts from the root
/// of the namespace.
///
/// This does not make any guarantee on the existence of the final component,
/// and it is the caller's responsibility to check it if necessary.
///
/// Starts from global root, regardless of the current task's root or cwd.
///
/// See [resolve_parent_from_with_root] for more details.
pub fn resolve_parent_from(
    from: &PathRef,
    path: &Path,
    flags: ResolveFlags,
) -> Result<(PathRef, String), SysError> {
    let root = root_pathref();
    resolve_parent_from_with_root(&root, from, path, flags)
}

/// Resolve the parent directory of a path, starting from a given [PathRef], and
/// returning the parent [PathRef] and the final component as a string.
///
/// If `path` is absolute, `from` is ignored and resolution starts from the
/// provided `root`.
///
/// This does not make any guarantee on the existence of the final component,
/// and it is the caller's responsibility to check it if necessary.
///
/// **Flags apply to the resolution of the parent directory. Last component
/// won't be resolved.**
///
/// Absolute input paths, and absolute symlink targets encountered during
/// resolution, are both interpreted relative to `root`.
pub fn resolve_parent_from_with_root(
    root: &PathRef,
    from: &PathRef,
    path: &Path,
    flags: ResolveFlags,
) -> Result<(PathRef, String), SysError> {
    let mut pending = collect_components(path)?;
    let Some(last) = pending.pop_back() else {
        return Err(SysError::InvalidArgument);
    };

    let name = match last {
        PendingComponent::Normal(name) => name,
        PendingComponent::CurDir | PendingComponent::ParentDir => {
            return Err(SysError::InvalidArgument);
        },
    };

    let cur_path = if path.is_absolute() {
        root.clone()
    } else {
        from.clone()
    };

    Ok((
        resolve_components(root.clone(), cur_path, pending, flags)?,
        name,
    ))
}

/// Helper function to collect components of a path into a queue for resolution.
///
/// This function seems a bit long to make it an inline? idk.
fn collect_components(path: &Path) -> Result<VecDeque<PendingComponent>, SysError> {
    let mut pending = VecDeque::new();
    let mut it = path.components();

    if path.is_absolute() {
        assert!(matches!(it.next().unwrap(), UnixComponent::RootDir));
    }

    for component in it {
        match component {
            UnixComponent::RootDir => unreachable!(/* handled explicitly above */),
            UnixComponent::CurDir => pending.push_back(PendingComponent::CurDir),
            UnixComponent::ParentDir => pending.push_back(PendingComponent::ParentDir),
            UnixComponent::Normal(raw) => {
                let name = core::str::from_utf8(raw).map_err(|_| SysError::InvalidArgument)?;
                pending.push_back(PendingComponent::Normal(name.to_string()));
            },
        }
    }

    Ok(pending)
}

/// Helper function to prepend components of a path to the pending queue during
/// symlink resolution.
#[inline]
fn prepend_components(
    pending: &mut VecDeque<PendingComponent>,
    path: &Path,
) -> Result<(), SysError> {
    let mut prefix = collect_components(path)?;
    while let Some(component) = prefix.pop_back() {
        pending.push_front(component);
    }
    Ok(())
}

/// Our core path resolution function. Internally, this is just a simple state
/// machine that processes components one by one.
///
/// We shouldn't use recursion here cz too many nested symlinks might cause
/// stack overflow.
fn resolve_components(
    logical_root: PathRef,
    mut cur_path: PathRef,
    mut pending: VecDeque<PendingComponent>,
    flags: ResolveFlags,
) -> Result<PathRef, SysError> {
    let mut remaining_links = SYMLINK_RESOLVE_LIMIT;

    while let Some(component) = pending.pop_front() {
        let is_last = pending.is_empty();

        match component {
            PendingComponent::CurDir => continue,
            PendingComponent::ParentDir => {
                // prevent escaping logical root via '..' components
                if cur_path.location_eq(&logical_root) {
                    kdebugln!("prevent escaping logical root via '..'");
                    continue;
                }
                cur_path = walk_parent(&cur_path);
            },
            PendingComponent::Normal(name) => {
                let child = lookup_child(&cur_path, &name)?;
                let child_ty = child.inode().ty();

                if child_ty != InodeType::Symlink {
                    // for non-directory child, follow_mount is actually unnecessary, but it doesn't
                    // hurt either and it simplifies code a bit.
                    cur_path = follow_mount(child);
                    continue;
                }

                if flags.contains(ResolveFlags::DENY_SYMLINKS) {
                    return Err(SysError::LinkEncountered);
                }

                if is_last && flags.contains(ResolveFlags::DENY_LAST_SYMLINK) {
                    return Err(SysError::LinkEncountered);
                }

                if is_last && flags.contains(ResolveFlags::UNFOLLOW_LAST_SYMLINK) {
                    cur_path = follow_mount(child);
                    continue;
                }

                if remaining_links == 0 {
                    return Err(SysError::TooManyLinks);
                }
                remaining_links -= 1;

                let target = child.inode().read_link()?;
                // empty symlink. invalid.
                // actually we should check this in upper layers (e.g. vfs_symlink). but some
                // redundant checks won't hurt and it can prevent some weird edge cases.
                if target.components().next().is_none() {
                    return Err(SysError::InvalidArgument);
                }

                if target.is_absolute() {
                    cur_path = logical_root.clone();
                }

                prepend_components(&mut pending, &target)?;
            },
        }
    }

    Ok(cur_path)
}

fn lookup_child(path: &PathRef, name: &str) -> Result<PathRef, SysError> {
    if let Ok(dentry) = path.dentry().lookup_child(name) {
        return Ok(PathRef::new(path.mount().clone(), dentry));
    }

    let dir = path.inode();
    if dir.ty() != InodeType::Dir {
        return Err(SysError::NotDir);
    }

    let inode = dir.lookup(name)?;
    let dentry = materialize_child_dentry(path.dentry(), name, inode)?;

    Ok(PathRef::new(path.mount().clone(), dentry))
}

fn follow_mount(mut path: PathRef) -> PathRef {
    // here we use while loop instead of a single if statement to handle the case
    // where there are multiple mounts in a row (e.g. mount A on top of root, then
    // mount B on top of A, etc.)
    while let Some(child_mount) = path.mount().child_at(path.dentry()) {
        path = PathRef::new(child_mount.clone(), child_mount.root().clone());
    }

    path
}

fn walk_parent(path: &PathRef) -> PathRef {
    let mut mount = path.mount().clone();
    let mut dentry = path.dentry().clone();

    loop {
        if let Some(parent) = dentry.parent() {
            return PathRef::new(mount, parent);
        }

        let Some(parent_mount) = mount.parent() else {
            return root_pathref();
        };

        // see `follow_mount` for the rationale of using a loop here.
        dentry = mount
            .mountpoint()
            .expect("non-root mount must have a mountpoint");
        mount = parent_mount;
    }
}
