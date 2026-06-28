//! User-ID credential syscalls.

pub mod geteuid;
pub mod getresuid;
pub mod getuid;
pub mod setfsuid;
pub mod setresuid;
pub mod setreuid;
pub mod setuid;

use crate::{
    prelude::*,
    task::credentials::{Uid, cap::SecureBits},
};

const FS_CAPS: Capability = Capability::CHOWN
    .union(Capability::DAC_OVERRIDE)
    .union(Capability::DAC_READ_SEARCH)
    .union(Capability::FOWNER)
    .union(Capability::FSETID);

/// Updates the current task's capabilities based on the change of
/// real/effective/saved user IDs. If [SecureBits::NO_SETUID_FIXUP] is set,
/// capabilities are not modified. If [SecureBits::KEEP_CAPS] is set, permitted
/// & effective capabilities are not cleared when leaving privileged (root) user
/// ID.
pub(super) fn update_caps_by_uid(new: &mut CredentialSet, old_uid: Credentials<Uid>) {
    if new.caps.securebits().contains(SecureBits::NO_SETUID_FIXUP) {
        return;
    }

    let was_privileged = old_uid.matches_any_res(Uid::ROOT);
    let leaves_privileged = !new.uid.matches_any_res(Uid::ROOT);

    // leave ROOT
    if was_privileged && leaves_privileged {
        if !new.caps.securebits().contains(SecureBits::KEEP_CAPS) {
            new.caps.set_permitted(Capability::empty());
            new.caps.set_effective(Capability::empty());
        }
        new.caps.set_ambient(Capability::empty());
    }

    // effective leave ROOT
    if old_uid.effective == Uid::ROOT && new.uid.effective != Uid::ROOT {
        new.caps.set_effective(Capability::empty());
    }

    // effective become ROOT
    if old_uid.effective != Uid::ROOT && new.uid.effective == Uid::ROOT {
        new.caps.set_effective(new.caps.permitted());
    }
}

/// Updates the current task's capabilities based on the change of filesystem
/// user ID. If [SecureBits::NO_SETUID_FIXUP] is set, capabilities are not
/// modified.
pub(super) fn update_caps_by_fsuid(new: &mut CredentialSet, old_fsuid: Uid) {
    if new.caps.securebits().contains(SecureBits::NO_SETUID_FIXUP) {
        return;
    }

    // fs leave ROOT
    if old_fsuid == Uid::ROOT && new.uid.fs != Uid::ROOT {
        new.caps.set_effective(new.caps.effective() - FS_CAPS);
    }

    // fs become ROOT
    if old_fsuid != Uid::ROOT && new.uid.fs == Uid::ROOT {
        new.caps
            .set_effective(new.caps.effective() | (new.caps.permitted() & FS_CAPS));
    }
}
