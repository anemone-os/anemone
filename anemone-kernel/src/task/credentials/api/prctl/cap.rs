use anemone_abi::capability::linux as abi;

use crate::{
    prelude::*,
    task::credentials::cap::{Capability, SecureBits},
};

use bitflags::Flags;

use super::{PrctlArgs, PrctlOption, invalid_prctl_args};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct AmbientOp: u32 {
        const IS_SET = abi::PR_CAP_AMBIENT_IS_SET;
        const RAISE = abi::PR_CAP_AMBIENT_RAISE;
        const LOWER = abi::PR_CAP_AMBIENT_LOWER;
        const CLEAR_ALL = abi::PR_CAP_AMBIENT_CLEAR_ALL;
    }
}

impl AmbientOp {
    fn from_raw(option: PrctlOption, raw: u64, args: PrctlArgs) -> Result<Self, SysError> {
        let raw = raw as u32;
        Self::FLAGS
            .iter()
            .find_map(|flag| {
                let value = *flag.value();
                (value.bits() == raw).then_some(value)
            })
            .ok_or_else(|| invalid_prctl_args(option, args, "unknown ambient capability operation"))
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum AmbientCommand {
    ClearAll,
    IsSet(Capability),
    Raise(Capability),
    Lower(Capability),
}

pub(super) fn cap_from_prctl_arg(option: PrctlOption, raw: u64) -> Result<Capability, SysError> {
    let raw = raw as u32;
    Capability::from_number(raw).map_err(|err| {
        if err == SysError::InvalidArgument {
            knoticeln!(
                "prctl: invalid capability for option {}: raw={}",
                option.bits(),
                raw
            );
        }
        err
    })
}

pub(super) fn parse_bool_arg(option: PrctlOption, args: PrctlArgs) -> Result<bool, SysError> {
    match args.arg2 {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(invalid_prctl_args(
            option,
            args,
            "expected arg2 to be 0 or 1",
        )),
    }
}

pub(super) fn parse_securebits(
    option: PrctlOption,
    args: PrctlArgs,
) -> Result<SecureBits, SysError> {
    let raw = args.arg2 as u32;
    SecureBits::from_number(raw).map_err(|err| {
        if err == SysError::InvalidArgument {
            knoticeln!(
                "prctl: invalid securebits value for option {}: raw={:#x}",
                option.bits(),
                raw
            );
        }
        err
    })
}

pub(super) fn parse_ambient_command(
    option: PrctlOption,
    args: PrctlArgs,
) -> Result<AmbientCommand, SysError> {
    let op = AmbientOp::from_raw(option, args.arg2, args)?;
    if op == AmbientOp::CLEAR_ALL {
        if args.arg3 | args.arg4 | args.arg5 != 0 {
            return Err(invalid_prctl_args(option, args, "tail arguments not zero"));
        }
        return Ok(AmbientCommand::ClearAll);
    }

    args.expect_arg4_to_arg5_zero(option)?;
    let cap = cap_from_prctl_arg(option, args.arg3)?;
    match op {
        AmbientOp::IS_SET => Ok(AmbientCommand::IsSet(cap)),
        AmbientOp::RAISE => Ok(AmbientCommand::Raise(cap)),
        _ => Ok(AmbientCommand::Lower(cap)),
    }
}

pub(super) fn prctl_capbset_read(cap: Capability) -> Result<u64, SysError> {
    Ok(get_current_task().cred().caps.bounding().contains(cap) as u64)
}

pub(super) fn prctl_capbset_drop(cap: Capability) -> Result<u64, SysError> {
    let task = get_current_task();
    if !task.has_cap(Capability::SETPCAP) {
        return Err(deny_permission!(
            "prctl CAPBSET_DROP denied: missing={:?}",
            Capability::SETPCAP
        ));
    }

    task.update_cred_with(|old| {
        let bounding = old.caps.bounding() - cap;
        old.caps.set_bounding(bounding);
        old.caps.set_ambient(old.caps.ambient() & bounding);
        Ok(())
    })?;
    Ok(0)
}

pub(super) fn prctl_get_securebits() -> Result<u64, SysError> {
    Ok(get_current_task().cred().caps.securebits().bits() as u64)
}

pub(super) fn prctl_set_securebits(securebits: SecureBits) -> Result<u64, SysError> {
    get_current_task().update_cred_with(|old| {
        let old_securebits = old.caps.securebits();
        let locked_base_bits = (old_securebits.bits() & SecureBits::LOCKS.bits()) >> 1;
        let changes_locked_base =
            (locked_base_bits & (old_securebits.bits() ^ securebits.bits())) != 0;
        let clears_existing_lock =
            (old_securebits.bits() & SecureBits::LOCKS.bits() & !securebits.bits()) != 0;
        // Locked securebits and lock bits themselves are one-way after SET_SECUREBITS.
        if changes_locked_base
            || clears_existing_lock
            || !old.caps.effective().contains(Capability::SETPCAP)
        {
            return Err(deny_permission!(
                "securebits update denied: old={:#x}, requested={:#x}, has_setpcap={}",
                old_securebits.bits(),
                securebits.bits(),
                old.caps.effective().contains(Capability::SETPCAP)
            ));
        }
        old.caps.set_securebits(securebits);
        Ok(())
    })?;
    Ok(0)
}

pub(super) fn prctl_get_keepcaps() -> Result<u64, SysError> {
    Ok(get_current_task()
        .cred()
        .caps
        .securebits()
        .contains(SecureBits::KEEP_CAPS) as u64)
}

pub(super) fn prctl_set_keepcaps(enabled: bool) -> Result<u64, SysError> {
    get_current_task().update_cred_with(|old| {
        if old.caps.securebits().contains(SecureBits::KEEP_CAPS_LOCKED) {
            return Err(deny_permission!(
                "keepcaps update denied: KEEP_CAPS is locked"
            ));
        }

        let mut securebits = old.caps.securebits();
        if enabled {
            securebits.insert(SecureBits::KEEP_CAPS);
        } else {
            securebits.remove(SecureBits::KEEP_CAPS);
        }
        old.caps.set_securebits(securebits);
        Ok(())
    })?;
    Ok(0)
}

pub(super) fn prctl_get_no_new_privs() -> Result<u64, SysError> {
    Ok(get_current_task().no_new_privs() as u64)
}

pub(super) fn prctl_set_no_new_privs() -> Result<u64, SysError> {
    get_current_task().set_no_new_privs();
    Ok(0)
}

pub(super) fn prctl_cap_ambient(command: AmbientCommand) -> Result<u64, SysError> {
    match command {
        AmbientCommand::ClearAll => {
            get_current_task().update_cred_with(|old| {
                old.caps.set_ambient(Capability::empty());
                Ok(())
            })?;
            Ok(0)
        },
        AmbientCommand::IsSet(cap) => {
            Ok(get_current_task().cred().caps.ambient().contains(cap) as u64)
        },
        AmbientCommand::Raise(cap) => {
            get_current_task().update_cred_with(|old| {
                if !old.caps.permitted().contains(cap)
                    || !old.caps.inheritable().contains(cap)
                    || old
                        .caps
                        .securebits()
                        .contains(SecureBits::NO_CAP_AMBIENT_RAISE)
                {
                    return Err(deny_permission!(
                        "ambient raise denied: cap={:?}, permitted={}, inheritable={}, ambient_raise_locked={}",
                        cap,
                        old.caps.permitted().contains(cap),
                        old.caps.inheritable().contains(cap),
                        old.caps
                            .securebits()
                            .contains(SecureBits::NO_CAP_AMBIENT_RAISE)
                    ));
                }
                old.caps.set_ambient(old.caps.ambient() | cap);
                Ok(())
            })?;
            Ok(0)
        },
        AmbientCommand::Lower(cap) => {
            get_current_task().update_cred_with(|old| {
                old.caps.set_ambient(old.caps.ambient() - cap);
                Ok(())
            })?;
            Ok(0)
        },
    }
}
