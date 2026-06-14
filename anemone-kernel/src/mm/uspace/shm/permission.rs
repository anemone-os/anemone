use crate::{prelude::*, task::credentials::cap::Capability};

use super::{ShmSegment, segment::ShmPerm};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShmPermissionClass {
    Owner,
    Group,
    Other,
}

#[derive(Debug, Clone)]
pub(super) struct ShmCredView {
    euid: Uid,
    egid: Gid,
    groups: Vec<Gid>,
    has_ipc_owner: bool,
    has_ipc_lock: bool,
    has_sys_admin: bool,
}

impl ShmCredView {
    pub(super) fn from_cred(cred: CredentialSet) -> Self {
        let has_ipc_owner = cred.has_cap_effective(Capability::IPC_OWNER);
        let has_ipc_lock = cred.has_cap_effective(Capability::IPC_LOCK);
        let has_sys_admin = cred.has_cap_effective(Capability::SYS_ADMIN);

        // SysV IPC DAC follows Linux effective uid/gid and supplementary
        // groups, not the VFS fsuid/fsgid view used by filesystem checks.
        Self {
            euid: cred.uid.effective,
            egid: cred.gid.effective,
            groups: cred.groups,
            has_ipc_owner,
            has_ipc_lock,
            has_sys_admin,
        }
    }

    pub(super) fn euid(&self) -> Uid {
        self.euid
    }

    pub(super) fn egid(&self) -> Gid {
        self.egid
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct ShmPermAccess: u16 {
        const EXECUTE = 0o1;
        const WRITE = 0o2;
        const READ = 0o4;
    }
}

impl ShmPermAccess {
    pub(super) fn from_mode_bits(mode: u16) -> Self {
        let requested = ((mode >> 6) | (mode >> 3) | mode) & 0o7;
        Self::from_bits_truncate(requested)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShmControlAccess {
    OwnerAdmin,
    LockAdmin,
}

pub(super) fn check_perm_access(
    segment: &ShmSegment,
    view: &ShmCredView,
    access: ShmPermAccess,
) -> Result<(), SysError> {
    let perm = segment.perm();
    if class_allows(&perm, view, access) || view.has_ipc_owner {
        Ok(())
    } else {
        Err(SysError::AccessDenied)
    }
}

pub(super) fn check_control_access(
    segment: &ShmSegment,
    view: &ShmCredView,
    access: ShmControlAccess,
) -> Result<(), SysError> {
    let perm = segment.perm();
    let permitted = match access {
        ShmControlAccess::OwnerAdmin => owner_or_creator(&perm, view) || view.has_sys_admin,
        ShmControlAccess::LockAdmin => owner_or_creator(&perm, view) || view.has_ipc_lock,
    };
    if permitted {
        Ok(())
    } else {
        Err(SysError::PermissionDenied)
    }
}

fn class_allows(perm: &ShmPerm, view: &ShmCredView, access: ShmPermAccess) -> bool {
    let granted = match selected_class(perm, view) {
        ShmPermissionClass::Owner => (perm.mode >> 6) & 0o7,
        ShmPermissionClass::Group => (perm.mode >> 3) & 0o7,
        ShmPermissionClass::Other => perm.mode & 0o7,
    };
    (access.bits() & !granted) == 0
}

fn selected_class(perm: &ShmPerm, view: &ShmCredView) -> ShmPermissionClass {
    if owner_or_creator(perm, view) {
        ShmPermissionClass::Owner
    } else if group_matches(perm, view) {
        ShmPermissionClass::Group
    } else {
        ShmPermissionClass::Other
    }
}

fn owner_or_creator(perm: &ShmPerm, view: &ShmCredView) -> bool {
    view.euid == perm.uid || view.euid == perm.cuid
}

fn group_matches(perm: &ShmPerm, view: &ShmCredView) -> bool {
    view.egid == perm.gid
        || view.egid == perm.cgid
        || view.groups.contains(&perm.gid)
        || view.groups.contains(&perm.cgid)
}
