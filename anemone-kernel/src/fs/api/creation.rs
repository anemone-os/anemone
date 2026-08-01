//! User-thread file creation policy.
//!
//! Syscall adapters normalize Linux ABI input before entering this module.
//! These helpers own current-task DAC, umask, fsuid/fsgid, and setgid-bit
//! formation; VFS primitives only consume the resulting explicit metadata.

use crate::{prelude::*, task::credentials::cap::Capability};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CreationMetadata {
    perm: InodePerm,
    uid: Uid,
    gid: Gid,
}

/// Stable credential snapshot for one user-thread filesystem operation.
///
/// The current task remains the truth source. The snapshot is deliberately
/// operation-local so DAC and inode-formation decisions use the same
/// credentials without carrying `Task` or credential storage into VFS.
pub(super) struct KernelCreationPolicy {
    checker: FsPermChecker,
    /// Snapshot of the task filesystem context's umask, represented as the rwx
    /// bits creation may retain. `FsState` remains the truth source; this value
    /// may become stale if a `CLONE_FS` peer changes umask after the operation
    /// starts, which is intentional for a stable per-operation snapshot.
    allowed_rwx: InodePerm,
}

impl KernelCreationPolicy {
    pub(super) fn for_current() -> Self {
        let task = get_current_task();
        Self {
            checker: FsPermChecker::new(task.cred()),
            allowed_rwx: task.mask_creation_perm(InodePerm::all_rwx()),
        }
    }

    #[cfg(feature = "kunit")]
    pub(super) fn for_kunit_root(umask: InodePerm) -> Self {
        Self {
            checker: FsPermChecker::new(CredentialSet::new_root()),
            allowed_rwx: InodePerm::all_rwx() & !umask,
        }
    }

    pub(super) fn checker(&self) -> &FsPermChecker {
        &self.checker
    }

    fn mask_requested_perm(&self, requested: InodePerm) -> InodePerm {
        let special = requested & !InodePerm::all_rwx();
        special | requested & self.allowed_rwx
    }

    fn masked_metadata(
        &self,
        parent_perm: InodePerm,
        parent_gid: Gid,
        ty: InodeType,
        requested: InodePerm,
    ) -> CreationMetadata {
        // SGID admission must inspect the requested group-execute bit before
        // umask can remove it. This matches Linux's mode_strip_sgid-before-
        // mode_strip_umask ordering and prevents umask from preserving SGID.
        let mut metadata = creation_metadata(&self.checker, parent_perm, parent_gid, ty, requested);
        metadata.perm = self.mask_requested_perm(metadata.perm);
        metadata
    }

    fn exact_metadata(
        &self,
        parent: &InodeRef,
        ty: InodeType,
        perm: InodePerm,
    ) -> CreationMetadata {
        creation_metadata(&self.checker, parent.perm(), parent.gid(), ty, perm)
    }
}

fn creation_metadata(
    checker: &FsPermChecker,
    parent_perm: InodePerm,
    parent_gid: Gid,
    ty: InodeType,
    mut perm: InodePerm,
) -> CreationMetadata {
    if ty == InodeType::Dir {
        perm.remove(InodePerm::ISUID | InodePerm::ISGID);
        if parent_perm.contains(InodePerm::ISGID) {
            perm.insert(InodePerm::ISGID);
        }
    } else if perm.contains(InodePerm::ISGID)
        && perm.contains(InodePerm::IXGRP)
        && parent_perm.contains(InodePerm::ISGID)
        && !checker.fs_group_allowed(parent_gid)
        && !checker.has_cap(Capability::FSETID)
    {
        perm.remove(InodePerm::ISGID);
    }

    let gid = if parent_perm.contains(InodePerm::ISGID) {
        parent_gid
    } else {
        checker.fsgid()
    };

    CreationMetadata {
        perm,
        uid: checker.fsuid(),
        gid,
    }
}

fn admit_create(policy: &KernelCreationPolicy, parent: &PathRef) -> Result<(), SysError> {
    // Keep user-visible EROFS-before-DAC admission here. The exact VFS
    // primitive deliberately rechecks writability so kernel-internal callers
    // cannot bypass the mount invariant.
    parent.mount().ensure_writable()?;
    policy
        .checker()
        .check_path(parent, FsAccess::WRITE | FsAccess::EXECUTE)
}

pub(super) fn kernel_touch_at(
    policy: &KernelCreationPolicy,
    parent: &PathRef,
    name: &str,
    requested_perm: InodePerm,
) -> Result<PathRef, SysError> {
    admit_create(policy, parent)?;
    let metadata = policy.masked_metadata(
        parent.inode().perm(),
        parent.inode().gid(),
        InodeType::Regular,
        requested_perm,
    );
    vfs_touch_at(parent, name, metadata.perm, metadata.uid, metadata.gid)
}

pub(super) fn kernel_mkdir_at(
    policy: &KernelCreationPolicy,
    parent: &PathRef,
    name: &str,
    requested_perm: InodePerm,
) -> Result<PathRef, SysError> {
    admit_create(policy, parent)?;
    let metadata = policy.masked_metadata(
        parent.inode().perm(),
        parent.inode().gid(),
        InodeType::Dir,
        requested_perm,
    );
    vfs_mkdir_at(parent, name, metadata.perm, metadata.uid, metadata.gid)
}

pub(super) fn kernel_make_node_at(
    policy: &KernelCreationPolicy,
    parent: &PathRef,
    name: &str,
    mode: InodeMode,
    rdev: DeviceId,
) -> Result<PathRef, SysError> {
    admit_create(policy, parent)?;
    let metadata = policy.masked_metadata(
        parent.inode().perm(),
        parent.inode().gid(),
        mode.ty(),
        mode.perm(),
    );
    let description = MakeNodeDescription::new(
        InodeMode::new(mode.ty(), metadata.perm),
        metadata.uid,
        metadata.gid,
        rdev,
    );
    vfs_make_node_at(parent, name, description)
}

pub(super) fn kernel_symlink_at(
    policy: &KernelCreationPolicy,
    parent: &PathRef,
    name: &str,
    target: &Path,
) -> Result<PathRef, SysError> {
    admit_create(policy, parent)?;
    // Linux ignores umask for symbolic links, but owner and parent-SGID group
    // inheritance still come from the user-thread creation policy.
    let metadata = policy.exact_metadata(parent.inode(), InodeType::Symlink, InodePerm::all_rwx());
    vfs_symlink_at(parent, target, name, metadata.uid, metadata.gid)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::task::credentials::CredentialSet;

    fn checker(fsuid: Uid, fsgid: Gid, groups: &[Gid], caps: Capability) -> FsPermChecker {
        let mut cred = CredentialSet::new_root();
        cred.uid.fs = fsuid;
        cred.gid.fs = fsgid;
        cred.groups = groups.to_vec();
        cred.caps.set_effective(caps);
        FsPermChecker::new(cred)
    }

    #[kunit]
    fn creation_metadata_selects_explicit_owner_and_parent_sgid_group() {
        let metadata = creation_metadata(
            &checker(Uid::new(1000), Gid::new(100), &[], Capability::empty()),
            InodePerm::ISGID | InodePerm::all_rwx(),
            Gid::new(200),
            InodeType::Regular,
            InodePerm::IRUSR | InodePerm::IWUSR,
        );

        assert_eq!(metadata.uid, Uid::new(1000));
        assert_eq!(metadata.gid, Gid::new(200));
        assert_eq!(metadata.perm, InodePerm::IRUSR | InodePerm::IWUSR);
    }

    #[kunit]
    fn directory_creation_replaces_requested_special_bits_with_parent_sgid() {
        let metadata = creation_metadata(
            &checker(Uid::new(1000), Gid::new(100), &[], Capability::empty()),
            InodePerm::ISGID,
            Gid::new(200),
            InodeType::Dir,
            InodePerm::ISUID | InodePerm::ISGID | InodePerm::all_rwx(),
        );

        assert_eq!(metadata.perm, InodePerm::ISGID | InodePerm::all_rwx());
        assert_eq!(metadata.gid, Gid::new(200));
    }

    #[kunit]
    fn regular_creation_clears_sgid_without_group_or_fsetid() {
        let requested = InodePerm::ISGID | InodePerm::IRUSR | InodePerm::IXUSR | InodePerm::IXGRP;
        let parent_gid = Gid::new(200);
        let parent_perm = InodePerm::ISGID | InodePerm::all_rwx();

        let denied = creation_metadata(
            &checker(Uid::new(1000), Gid::new(100), &[], Capability::empty()),
            parent_perm,
            parent_gid,
            InodeType::Regular,
            requested,
        );
        assert!(!denied.perm.contains(InodePerm::ISGID));

        let grouped = creation_metadata(
            &checker(
                Uid::new(1000),
                Gid::new(100),
                &[parent_gid],
                Capability::empty(),
            ),
            parent_perm,
            parent_gid,
            InodeType::Regular,
            requested,
        );
        assert!(grouped.perm.contains(InodePerm::ISGID));

        let capable = creation_metadata(
            &checker(Uid::new(1000), Gid::new(100), &[], Capability::FSETID),
            parent_perm,
            parent_gid,
            InodeType::Regular,
            requested,
        );
        assert!(capable.perm.contains(InodePerm::ISGID));
    }

    #[kunit]
    fn creation_policy_masks_rwx_once_without_clearing_special_bits() {
        let policy = KernelCreationPolicy {
            checker: checker(Uid::ROOT, Gid::ROOT, &[], Capability::empty()),
            allowed_rwx: InodePerm::from_bits(0o750).unwrap(),
        };
        let requested = InodePerm::ISVTX | InodePerm::all_rwx();

        assert_eq!(
            policy.mask_requested_perm(requested),
            InodePerm::ISVTX | InodePerm::from_bits(0o750).unwrap()
        );
    }

    #[kunit]
    fn creation_policy_strips_sgid_before_umask_removes_group_execute() {
        let parent_gid = Gid::new(200);
        let policy = KernelCreationPolicy {
            checker: checker(Uid::new(1000), Gid::new(100), &[], Capability::empty()),
            allowed_rwx: InodePerm::all_rwx() & !InodePerm::IXGRP,
        };
        let metadata = policy.masked_metadata(
            InodePerm::ISGID | InodePerm::all_rwx(),
            parent_gid,
            InodeType::Regular,
            InodePerm::ISGID | InodePerm::all_rwx(),
        );

        assert_eq!(metadata.gid, parent_gid);
        assert_eq!(metadata.perm, InodePerm::all_rwx() & !InodePerm::IXGRP);
    }
}
