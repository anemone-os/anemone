//! prctl credential-related operations.

mod cap;

use anemone_abi::capability::linux as abi;

use crate::prelude::*;

use bitflags::Flags;

use cap::{
    cap_from_prctl_arg, parse_ambient_command, parse_bool_arg, parse_securebits, prctl_cap_ambient,
    prctl_capbset_drop, prctl_capbset_read, prctl_get_keepcaps, prctl_get_no_new_privs,
    prctl_get_securebits, prctl_set_keepcaps, prctl_set_no_new_privs, prctl_set_securebits,
};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) struct PrctlOption: u32 {
        const SET_PDEATHSIG = abi::PR_SET_PDEATHSIG;
        const GET_PDEATHSIG = abi::PR_GET_PDEATHSIG;
        const GET_DUMPABLE = abi::PR_GET_DUMPABLE;
        const SET_DUMPABLE = abi::PR_SET_DUMPABLE;
        const GET_UNALIGN = abi::PR_GET_UNALIGN;
        const SET_UNALIGN = abi::PR_SET_UNALIGN;
        /// Return whether capability retention across uid changes is enabled.
        const GET_KEEPCAPS = abi::PR_GET_KEEPCAPS;
        /// Enable or disable retaining permitted capabilities across uid changes.
        const SET_KEEPCAPS = abi::PR_SET_KEEPCAPS;
        const GET_FPEMU = abi::PR_GET_FPEMU;
        const SET_FPEMU = abi::PR_SET_FPEMU;
        const GET_FPEXC = abi::PR_GET_FPEXC;
        const SET_FPEXC = abi::PR_SET_FPEXC;
        const GET_TIMING = abi::PR_GET_TIMING;
        const SET_TIMING = abi::PR_SET_TIMING;
        const SET_NAME = abi::PR_SET_NAME;
        const GET_NAME = abi::PR_GET_NAME;
        const GET_ENDIAN = abi::PR_GET_ENDIAN;
        const SET_ENDIAN = abi::PR_SET_ENDIAN;
        const GET_SECCOMP = abi::PR_GET_SECCOMP;
        const SET_SECCOMP = abi::PR_SET_SECCOMP;
        /// Test whether a capability is present in the task capability bounding set.
        const CAPBSET_READ = abi::PR_CAPBSET_READ;
        /// Remove a capability from the task capability bounding set.
        const CAPBSET_DROP = abi::PR_CAPBSET_DROP;
        const GET_TSC = abi::PR_GET_TSC;
        const SET_TSC = abi::PR_SET_TSC;
        /// Return the securebits that constrain capability privilege transitions.
        const GET_SECUREBITS = abi::PR_GET_SECUREBITS;
        /// Update securebits and their one-way lock bits.
        const SET_SECUREBITS = abi::PR_SET_SECUREBITS;
        const SET_TIMERSLACK = abi::PR_SET_TIMERSLACK;
        const GET_TIMERSLACK = abi::PR_GET_TIMERSLACK;
        const TASK_PERF_EVENTS_DISABLE = abi::PR_TASK_PERF_EVENTS_DISABLE;
        const TASK_PERF_EVENTS_ENABLE = abi::PR_TASK_PERF_EVENTS_ENABLE;
        const MCE_KILL = abi::PR_MCE_KILL;
        const MCE_KILL_GET = abi::PR_MCE_KILL_GET;
        const SET_MM = abi::PR_SET_MM;
        const SET_PTRACER = abi::PR_SET_PTRACER;
        const SET_CHILD_SUBREAPER = abi::PR_SET_CHILD_SUBREAPER;
        const GET_CHILD_SUBREAPER = abi::PR_GET_CHILD_SUBREAPER;
        /// Irreversibly block future privilege gains for the current task.
        const SET_NO_NEW_PRIVS = abi::PR_SET_NO_NEW_PRIVS;
        /// Return whether future privilege gains are blocked for the current task.
        const GET_NO_NEW_PRIVS = abi::PR_GET_NO_NEW_PRIVS;
        const GET_TID_ADDRESS = abi::PR_GET_TID_ADDRESS;
        const SET_THP_DISABLE = abi::PR_SET_THP_DISABLE;
        const GET_THP_DISABLE = abi::PR_GET_THP_DISABLE;
        const MPX_ENABLE_MANAGEMENT = abi::PR_MPX_ENABLE_MANAGEMENT;
        const MPX_DISABLE_MANAGEMENT = abi::PR_MPX_DISABLE_MANAGEMENT;
        const SET_FP_MODE = abi::PR_SET_FP_MODE;
        const GET_FP_MODE = abi::PR_GET_FP_MODE;
        /// Query and update the task ambient capability set.
        const CAP_AMBIENT = abi::PR_CAP_AMBIENT;
        const SVE_SET_VL = abi::PR_SVE_SET_VL;
        const SVE_GET_VL = abi::PR_SVE_GET_VL;
        const GET_SPECULATION_CTRL = abi::PR_GET_SPECULATION_CTRL;
        const SET_SPECULATION_CTRL = abi::PR_SET_SPECULATION_CTRL;
        const PAC_RESET_KEYS = abi::PR_PAC_RESET_KEYS;
        const SET_TAGGED_ADDR_CTRL = abi::PR_SET_TAGGED_ADDR_CTRL;
        const GET_TAGGED_ADDR_CTRL = abi::PR_GET_TAGGED_ADDR_CTRL;
        const SET_IO_FLUSHER = abi::PR_SET_IO_FLUSHER;
        const GET_IO_FLUSHER = abi::PR_GET_IO_FLUSHER;
        const SET_SYSCALL_USER_DISPATCH = abi::PR_SET_SYSCALL_USER_DISPATCH;
        const PAC_SET_ENABLED_KEYS = abi::PR_PAC_SET_ENABLED_KEYS;
        const PAC_GET_ENABLED_KEYS = abi::PR_PAC_GET_ENABLED_KEYS;
        const SCHED_CORE = abi::PR_SCHED_CORE;
        const SME_SET_VL = abi::PR_SME_SET_VL;
        const SME_GET_VL = abi::PR_SME_GET_VL;
        const SET_MDWE = abi::PR_SET_MDWE;
        const GET_MDWE = abi::PR_GET_MDWE;
        const SET_VMA = abi::PR_SET_VMA;
        const GET_AUXV = abi::PR_GET_AUXV;
        const SET_MEMORY_MERGE = abi::PR_SET_MEMORY_MERGE;
        const GET_MEMORY_MERGE = abi::PR_GET_MEMORY_MERGE;
        const RISCV_V_SET_CONTROL = abi::PR_RISCV_V_SET_CONTROL;
        const RISCV_V_GET_CONTROL = abi::PR_RISCV_V_GET_CONTROL;

        const IMPLEMENTED = Self::GET_KEEPCAPS.bits()
            | Self::SET_KEEPCAPS.bits()
            | Self::CAPBSET_READ.bits()
            | Self::CAPBSET_DROP.bits()
            | Self::GET_SECUREBITS.bits()
            | Self::SET_SECUREBITS.bits()
            | Self::SET_NO_NEW_PRIVS.bits()
            | Self::GET_NO_NEW_PRIVS.bits()
            | Self::CAP_AMBIENT.bits();
    }
}

#[derive(Clone, Copy)]
pub(super) struct PrctlArgs {
    pub(super) arg2: u64,
    pub(super) arg3: u64,
    pub(super) arg4: u64,
    pub(super) arg5: u64,
}

impl PrctlArgs {
    fn new(arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> Self {
        Self {
            arg2,
            arg3,
            arg4,
            arg5,
        }
    }

    pub(super) fn expect_no_args(self, option: PrctlOption) -> Result<(), SysError> {
        if self.arg2 | self.arg3 | self.arg4 | self.arg5 != 0 {
            return Err(invalid_prctl_args(
                option,
                self,
                "expected all extra args to be zero",
            ));
        }
        Ok(())
    }

    pub(super) fn expect_arg4_to_arg5_zero(self, option: PrctlOption) -> Result<(), SysError> {
        if self.arg4 | self.arg5 != 0 {
            return Err(invalid_prctl_args(
                option,
                self,
                "expected arg4 and arg5 to be zero",
            ));
        }
        Ok(())
    }
}

pub(super) fn invalid_prctl_args(option: PrctlOption, args: PrctlArgs, reason: &str) -> SysError {
    knoticeln!(
        "prctl: invalid arguments for option {}: {}; arg2={:#x}, arg3={:#x}, arg4={:#x}, arg5={:#x}",
        option.bits(),
        reason,
        args.arg2,
        args.arg3,
        args.arg4,
        args.arg5
    );
    SysError::InvalidArgument
}

fn prctl_not_implemented(option: PrctlOption) -> Result<u64, SysError> {
    knoticeln!("[NYI] prctl option {} is not supported yet", option.bits());
    Err(SysError::NotYetImplemented)
}

fn prctl_option_from_raw(raw: u32) -> Result<PrctlOption, SysError> {
    PrctlOption::FLAGS
        .iter()
        .filter(|flag| flag.name() != "IMPLEMENTED")
        .find_map(|flag| {
            let value = *flag.value();
            (value.bits() == raw).then_some(value)
        })
        .ok_or_else(|| {
            knoticeln!("prctl: invalid option {}", raw);
            SysError::InvalidArgument
        })
}

fn dispatch_prctl(option: PrctlOption, args: PrctlArgs) -> Result<u64, SysError> {
    match option {
        // Purpose: report whether a capability is still in the bounding set.
        // Permission check: no extra privilege; the capability number must be valid.
        // Man page: https://man7.org/linux/man-pages/man2/PR_CAPBSET_READ.2const.html
        PrctlOption::CAPBSET_READ => prctl_capbset_read(cap_from_prctl_arg(option, args.arg2)?),
        // Purpose: permanently remove a capability from the bounding set.
        // Permission check: caller must hold CAP_SETPCAP.
        // Man page: https://man7.org/linux/man-pages/man2/PR_CAPBSET_DROP.2const.html
        PrctlOption::CAPBSET_DROP => prctl_capbset_drop(cap_from_prctl_arg(option, args.arg2)?),
        // Purpose: report whether future privilege gains are blocked.
        // Permission check: no extra privilege; all extra arguments must be zero.
        // Man page: https://man7.org/linux/man-pages/man2/PR_GET_NO_NEW_PRIVS.2const.html
        PrctlOption::GET_NO_NEW_PRIVS => {
            args.expect_no_args(option)?;
            prctl_get_no_new_privs()
        },
        // Purpose: irreversibly block future privilege gains for this task.
        // Permission check: no extra privilege; only arg2=1 with zero tail args is accepted.
        // Man page: https://man7.org/linux/man-pages/man2/PR_SET_NO_NEW_PRIVS.2const.html
        PrctlOption::SET_NO_NEW_PRIVS => {
            if args.arg2 != 1 || args.arg3 | args.arg4 | args.arg5 != 0 {
                return Err(invalid_prctl_args(
                    option,
                    args,
                    "SET_NO_NEW_PRIVS only accepts arg2=1 and zero tail args",
                ));
            }
            prctl_set_no_new_privs()
        },
        // Purpose: return the securebits that constrain capability transitions.
        // Permission check: no extra privilege.
        // Man page: https://man7.org/linux/man-pages/man2/PR_GET_SECUREBITS.2const.html
        PrctlOption::GET_SECUREBITS => prctl_get_securebits(),
        // Purpose: update securebits and their one-way lock bits.
        // Permission check: caller must hold CAP_SETPCAP; locked bits cannot be changed.
        // Man page: https://man7.org/linux/man-pages/man2/PR_SET_SECUREBITS.2const.html
        PrctlOption::SET_SECUREBITS => prctl_set_securebits(parse_securebits(option, args)?),
        // Purpose: report whether capabilities are retained across uid changes.
        // Permission check: no extra privilege.
        // Man page: https://man7.org/linux/man-pages/man2/PR_GET_KEEPCAPS.2const.html
        PrctlOption::GET_KEEPCAPS => prctl_get_keepcaps(),
        // Purpose: enable or disable retaining permitted capabilities across uid changes.
        // Permission check: denied when KEEP_CAPS is locked.
        // Man page: https://man7.org/linux/man-pages/man2/PR_SET_KEEPCAPS.2const.html
        PrctlOption::SET_KEEPCAPS => prctl_set_keepcaps(parse_bool_arg(option, args)?),
        // Purpose: query, raise, lower, or clear ambient capabilities.
        // Permission check: raise requires the capability in permitted and inheritable sets,
        // and ambient raising must not be blocked by securebits.
        // Man page: https://man7.org/linux/man-pages/man2/PR_CAP_AMBIENT.2const.html
        PrctlOption::CAP_AMBIENT => prctl_cap_ambient(parse_ambient_command(option, args)?),
        _ => prctl_not_implemented(option),
    }
}

/// Handles credential-affecting `prctl` operations.
///
/// Permission check: read-only operations do not require extra privileges.
/// Dropping the capability bounding set and changing locked securebits require
/// `CAP_SETPCAP`; raising ambient capabilities also requires the target
/// capability to be present in both permitted and inheritable sets and not be
/// blocked by securebits. `PR_SET_NO_NEW_PRIVS` accepts only the irreversible
/// enable operation.
///
/// Reference: <https://man7.org/linux/man-pages/man2/prctl.2.html>.
#[syscall(SYS_PRCTL)]
fn sys_prctl(option: u32, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> Result<u64, SysError> {
    kdebugln!(
        "prctl: option={}, arg2={:#x}, arg3={:#x}, arg4={:#x}, arg5={:#x}",
        option,
        arg2,
        arg3,
        arg4,
        arg5,
    );

    let option = prctl_option_from_raw(option)?;
    dispatch_prctl(option, PrctlArgs::new(arg2, arg3, arg4, arg5))
}
