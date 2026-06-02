//! Filesystem discretionary access checks.

use crate::{
    prelude::*,
    task::credentials::cap::{Capability, SecureBits},
};

bitflags! {
    /// Read/write/execute access requested by a filesystem operation.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct FsAccess: u8 {
        const READ = 1 << 0;
        const WRITE = 1 << 1;
        const EXECUTE = 1 << 2;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PermissionClass {
    Owner,
    Group,
    Other,
}

/// Reusable DAC checker for filesystem permission decisions.
#[derive(Debug, Clone)]
pub struct FsPermChecker {
    cred: CredentialSet,
}

impl FsPermChecker {
    /// Build a checker for ordinary filesystem operations, which use fsuid,
    /// fsgid, supplementary groups, and effective capabilities.
    pub fn for_current_fs() -> Self {
        Self::new(get_current_task().cred())
    }

    /// Build a checker for access(2)-style checks without AT_EACCESS, which
    /// use real uid/gid and the capability view derived from the real uid.
    pub fn for_access_real_ids() -> Self {
        let mut cred = get_current_task().cred();
        cred.uid.fs = cred.uid.real;
        cred.gid.fs = cred.gid.real;
        if !cred.caps.securebits().contains(SecureBits::NO_SETUID_FIXUP) {
            if cred.uid.real == Uid::ROOT {
                cred.caps.set_effective(cred.caps.permitted());
            } else {
                cred.caps.set_effective(Capability::empty());
            }
        }
        Self::new(cred)
    }

    /// Build a checker for faccessat(2) AT_EACCESS checks, which use effective
    /// uid/gid and effective capabilities.
    pub fn for_access_effective_ids() -> Self {
        let mut cred = get_current_task().cred();
        cred.uid.fs = cred.uid.effective;
        cred.gid.fs = cred.gid.effective;
        Self::new(cred)
    }

    /// Build a checker from an explicit credential snapshot.
    pub fn new(cred: CredentialSet) -> Self {
        Self { cred }
    }

    /// Check read/write/execute DAC permission on a path's inode.
    pub fn check_path(&self, path: &PathRef, access: FsAccess) -> Result<(), SysError> {
        self.check_inode(path.inode(), access)
    }

    /// Check read/write/execute DAC permission on an inode.
    pub fn check_inode(&self, inode: &InodeRef, access: FsAccess) -> Result<(), SysError> {
        if access.is_empty()
            || self.class_allows(inode, access)
            || self.capability_bypasses(inode, access)
        {
            Ok(())
        } else {
            Err(SysError::AccessDenied)
        }
    }

    /// Check whether the checker's fsuid owns the inode.
    pub fn is_owner(&self, inode: &InodeRef) -> bool {
        self.cred.uid.fs == inode.uid()
    }

    /// Check whether the checker's filesystem group set matches `gid`.
    pub fn fs_group_allowed(&self, gid: Gid) -> bool {
        self.cred.gid.fs == gid || self.cred.groups.contains(&gid)
    }

    /// Check whether the checker owns the inode or has CAP_FOWNER.
    pub fn owner_or_capable(&self, inode: &InodeRef) -> bool {
        self.is_owner(inode) || self.has_cap(Capability::FOWNER)
    }

    /// Check whether the checker has an effective capability.
    pub fn has_cap(&self, cap: Capability) -> bool {
        self.cred.has_cap_effective(cap)
    }

    /// Select the POSIX owner/group/other permission class for an inode.
    fn selected_class(&self, inode: &InodeRef) -> PermissionClass {
        if self.cred.uid.fs == inode.uid() {
            PermissionClass::Owner
        } else if self.fs_group_allowed(inode.gid()) {
            PermissionClass::Group
        } else {
            PermissionClass::Other
        }
    }

    /// Check whether the selected permission class grants all requested bits.
    fn class_allows(&self, inode: &InodeRef, access: FsAccess) -> bool {
        let class = self.selected_class(inode);
        let perm = inode.perm();

        if access.contains(FsAccess::READ) && !class_can_read(class, perm) {
            return false;
        }
        if access.contains(FsAccess::WRITE) && !class_can_write(class, perm) {
            return false;
        }
        if access.contains(FsAccess::EXECUTE) && !class_can_execute(class, perm) {
            return false;
        }
        true
    }

    /// Apply DAC capability bypass rules for filesystem access checks.
    fn capability_bypasses(&self, inode: &InodeRef, access: FsAccess) -> bool {
        let perm = inode.perm();

        if inode.ty() == InodeType::Dir {
            if !access.contains(FsAccess::WRITE) && self.has_cap(Capability::DAC_READ_SEARCH) {
                return true;
            }
            return self.has_cap(Capability::DAC_OVERRIDE);
        }

        if access == FsAccess::READ && self.has_cap(Capability::DAC_READ_SEARCH) {
            return true;
        }

        self.has_cap(Capability::DAC_OVERRIDE)
            && (!access.contains(FsAccess::EXECUTE) || has_any_execute_bit(perm))
    }
}

/// Check read permission for one POSIX permission class.
fn class_can_read(class: PermissionClass, perm: InodePerm) -> bool {
    match class {
        PermissionClass::Owner => perm.contains(InodePerm::IRUSR),
        PermissionClass::Group => perm.contains(InodePerm::IRGRP),
        PermissionClass::Other => perm.contains(InodePerm::IROTH),
    }
}

/// Check write permission for one POSIX permission class.
fn class_can_write(class: PermissionClass, perm: InodePerm) -> bool {
    match class {
        PermissionClass::Owner => perm.contains(InodePerm::IWUSR),
        PermissionClass::Group => perm.contains(InodePerm::IWGRP),
        PermissionClass::Other => perm.contains(InodePerm::IWOTH),
    }
}

/// Check execute/search permission for one POSIX permission class.
fn class_can_execute(class: PermissionClass, perm: InodePerm) -> bool {
    match class {
        PermissionClass::Owner => perm.contains(InodePerm::IXUSR),
        PermissionClass::Group => perm.contains(InodePerm::IXGRP),
        PermissionClass::Other => perm.contains(InodePerm::IXOTH),
    }
}

/// Check whether any execute bit is set on the inode.
fn has_any_execute_bit(perm: InodePerm) -> bool {
    perm.intersects(InodePerm::IXUSR | InodePerm::IXGRP | InodePerm::IXOTH)
}
