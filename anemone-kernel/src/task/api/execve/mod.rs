pub mod binfmt;
pub mod kernel;
pub mod syscall;

use crate::{
    prelude::*,
    task::credentials::cap::{Capability, FileCapabilities, SecureBits},
};

#[derive(Debug)]
struct ExecCredentials {
    cred: CredentialSet,
    secure_exec: bool,
}

fn compute_exec_credentials(
    old: &CredentialSet,
    attr: InodeStat,
    file_caps: FileCapabilities,
    no_new_privs: bool,
) -> Result<ExecCredentials, SysError> {
    let mut new = old.clone();
    let old_caps = old.caps.clone();
    let securebits = old_caps.securebits();
    let suppress_privilege_gain = no_new_privs;
    let file_caps = if suppress_privilege_gain {
        FileCapabilities::empty()
    } else {
        file_caps
    };
    let has_file_caps = !file_caps.is_empty();

    if !suppress_privilege_gain {
        let perm = attr.mode.perm();
        if perm.contains(InodePerm::ISUID) {
            new.uid.effective = attr.uid;
        }
        if perm.contains(InodePerm::ISGID) && perm.contains(InodePerm::IXGRP) {
            new.gid.effective = attr.gid;
        }
    }

    let is_setid = new.uid.effective != old.uid.real || new.gid.effective != old.gid.real;
    let root_exec = new.uid.real == Uid::ROOT || new.uid.effective == Uid::ROOT;
    let setuid_root = new.uid.real != Uid::ROOT && new.uid.effective == Uid::ROOT;

    let mut file_effective = file_caps.effective();
    let mut permitted = (old_caps.bounding() & file_caps.permitted())
        | (old_caps.inheritable() & file_caps.inheritable());

    let missing_file_permitted = file_caps.permitted() - permitted;
    if file_effective && !missing_file_permitted.is_empty() {
        return Err(deny_permission!(
            "execve denied: file capabilities exceed bounding set: missing={:?}",
            missing_file_permitted
        ));
    }

    if !securebits.contains(SecureBits::NOROOT) && root_exec && !(has_file_caps && setuid_root) {
        permitted = old_caps.bounding() | old_caps.inheritable();
        if new.uid.effective == Uid::ROOT {
            file_effective = true;
        }
    }

    if no_new_privs {
        permitted &= old_caps.permitted();
    }

    new.uid.saved = new.uid.effective;
    new.uid.fs = new.uid.effective;
    new.gid.saved = new.gid.effective;
    new.gid.fs = new.gid.effective;

    if has_file_caps || is_setid {
        new.caps.set_ambient(Capability::empty());
    }

    permitted |= new.caps.ambient();
    new.caps.set_permitted(permitted);
    if file_effective {
        new.caps.set_effective(permitted);
    } else {
        new.caps.set_effective(new.caps.ambient());
    }

    let mut securebits = new.caps.securebits();
    securebits.remove(SecureBits::KEEP_CAPS);
    new.caps.set_securebits(securebits);

    let secure_exec = is_setid
        || (new.uid.real != Uid::ROOT
            && (file_effective || !new.caps.ambient().contains(new.caps.permitted())));

    Ok(ExecCredentials {
        cred: new,
        secure_exec,
    })
}

pub(super) fn prepare_credentials_for_exec(
    old: &CredentialSet,
    file: &PathRef,
    no_new_privs: bool,
) -> Result<(CredentialSet, bool), SysError> {
    let attr = file.inode().get_attr()?;
    let file_caps = file.inode().get_file_cap()?;
    let exec_cred = compute_exec_credentials(old, attr, file_caps, no_new_privs)?;
    Ok((exec_cred.cred, exec_cred.secure_exec))
}
