//! Path resolution and name lookup.

use crate::{fs::root_pathref, prelude::*};
use typed_path::{Component, UnixComponent};

/// Get or create a child dentry with the given name under the given parent
/// dentry, and return an [Arc] to it.
pub fn canonicalize_child(
    parent: &Arc<Dentry>,
    name: &str,
    inode: InodeRef,
) -> Result<Arc<Dentry>, FsError> {
    let child = Arc::new(Dentry::new(name.to_string(), Some(parent), inode));

    match parent.insert_child(name.to_string(), &child) {
        Ok(()) => Ok(child),
        Err(FsError::AlreadyExists) => parent.lookup_child(name),
        Err(err) => Err(err),
    }
}

/// Resolve a path to a [PathRef], starting from the root of the namespace.
pub fn resolve(path: &Path) -> Result<PathRef, FsError> {
    if path.components().next().is_none() {
        // empty path
        return Err(FsError::InvalidArgument);
    }

    resolve_from(&root_pathref(), path)
}

/// Resolve a path to a [PathRef], starting from a given [PathRef].
///
/// If `path` is absolute, `from` is ignored and resolution starts from the root
/// of the namespace.
pub fn resolve_from(from: &PathRef, path: &Path) -> Result<PathRef, FsError> {
    let mut it = path.components();

    let mut cur_path = if path.is_absolute() {
        assert!(it.next().unwrap().is_root());
        root_pathref()
    } else {
        from.clone()
    };

    for component in it {
        match component {
            UnixComponent::RootDir => unreachable!(), // already handled above
            UnixComponent::CurDir => continue,
            UnixComponent::ParentDir => {
                cur_path = walk_parent(&cur_path);
                continue;
            },
            UnixComponent::Normal(raw) => {
                let name = core::str::from_utf8(raw).map_err(|_| FsError::InvalidArgument)?;
                cur_path = lookup_child(&cur_path, name)?;
            },
        }
    }

    Ok(cur_path)
}

/// Resolve the parent directory of a path, returning the parent [PathRef] and
/// the final component as a string.
pub fn resolve_parent(path: &Path) -> Result<(PathRef, String), FsError> {
    if path.components().next().is_none() {
        // empty path
        return Err(FsError::InvalidArgument);
    }

    resolve_parent_from(&root_pathref(), path)
}

/// Resolve the parent directory of a path, starting from a given [PathRef], and
/// returning the parent [PathRef] and the final component as a string.
///
/// If `path` is absolute, `from` is ignored and resolution starts from the root
/// of the namespace.
pub fn resolve_parent_from(from: &PathRef, path: &Path) -> Result<(PathRef, String), FsError> {
    let mut it = path.components().peekable();

    let mut cur_path = if path.is_absolute() {
        assert!(it.next().unwrap().is_root());
        root_pathref()
    } else {
        from.clone()
    };

    while let Some(component) = it.next() {
        let is_last = it.peek().is_none();

        match component {
            UnixComponent::RootDir => unreachable!(), // already handled above
            UnixComponent::CurDir => {
                if is_last {
                    return Err(FsError::InvalidArgument);
                }
                continue;
            },
            UnixComponent::ParentDir => {
                if is_last {
                    return Err(FsError::InvalidArgument);
                }
                cur_path = walk_parent(&cur_path);
                continue;
            },
            UnixComponent::Normal(raw) => {
                let name = core::str::from_utf8(raw).map_err(|_| FsError::InvalidArgument)?;
                if is_last {
                    return Ok((cur_path, name.to_string()));
                }
                cur_path = lookup_child(&cur_path, name)?;
            },
        }
    }

    Err(FsError::InvalidArgument)
}

fn lookup_child(path: &PathRef, name: &str) -> Result<PathRef, FsError> {
    if let Ok(dentry) = path.dentry().lookup_child(name) {
        return Ok(follow_mount(PathRef::new(path.mount().clone(), dentry)));
    }

    let dir = path.inode();
    if dir.ty() != InodeType::Dir {
        return Err(FsError::NotDir);
    }

    let inode = dir.lookup(name)?;
    let dentry = canonicalize_child(path.dentry(), name, inode)?;

    Ok(follow_mount(PathRef::new(path.mount().clone(), dentry)))
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
