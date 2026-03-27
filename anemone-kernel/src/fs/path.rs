use core::fmt::{Debug, Display};

use crate::prelude::*;

/// A single [PathRef] determines the absolute location of a file in the
/// namespace.
///
/// Reference: https://elixir.bootlin.com/linux/v6.6.32/source/include/linux/path.h
#[derive(Clone)]
pub struct PathRef {
    mount: Arc<Mount>,
    dentry: Arc<Dentry>,
}

impl Debug for PathRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PathRef")
            .field("mount", &self.mount)
            .field("dentry", &self.dentry)
            .finish()
    }
}

impl Display for PathRef {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.to_pathbuf().display())
    }
}

impl PathRef {
    pub fn new(mount: Arc<Mount>, dentry: Arc<Dentry>) -> Self {
        Self { mount, dentry }
    }

    /// Get the mount this path is under.
    pub fn mount(&self) -> &Arc<Mount> {
        &self.mount
    }

    /// Get the dentry this path points to.
    pub fn dentry(&self) -> &Arc<Dentry> {
        &self.dentry
    }

    /// Get the inode this path points to.
    pub fn inode(&self) -> &InodeRef {
        &self.dentry.inode()
    }

    /// Convert this path to a [PathBuf] by walking up the dentry tree and
    /// concatenating the names of the dentries.
    pub fn to_pathbuf(&self) -> PathBuf {
        let mut components = vec![];

        let mut cur_mount = self.mount.clone();
        let mut cur_dentry = Some(self.dentry.clone());

        while let Some(dentry) = cur_dentry {
            if let Some(parent) = dentry.parent() {
                components.push(dentry.name());
                cur_dentry = Some(parent);
            } else {
                // reached the root of current mount.
                if let Some(parent_mount) = cur_mount.parent() {
                    let mountpoint = cur_mount
                        .mountpoint()
                        .expect("non-root mount must have a mountpoint");
                    cur_mount = parent_mount;
                    cur_dentry = Some(mountpoint);
                } else {
                    // reached the root of the entire namespace.
                    components.push("/".to_string());
                    break;
                }
            }
        }

        components.reverse();
        PathBuf::from_iter(components)
    }
}
