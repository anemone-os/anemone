//! Linux capability and capability-related prctl ABI constants.
//!
//! Values are copied from Linux 6.6.32 `include/uapi/linux/capability.h`
//! and `include/uapi/linux/prctl.h`.

pub mod linux {
    pub const _LINUX_CAPABILITY_VERSION_1: u32 = 0x19980330;
    pub const _LINUX_CAPABILITY_U32S_1: usize = 1;

    pub const _LINUX_CAPABILITY_VERSION_2: u32 = 0x20071026;
    pub const _LINUX_CAPABILITY_U32S_2: usize = 2;

    pub const _LINUX_CAPABILITY_VERSION_3: u32 = 0x20080522;
    pub const _LINUX_CAPABILITY_U32S_3: usize = 2;

    pub const _KERNEL_CAPABILITY_VERSION: u32 = _LINUX_CAPABILITY_VERSION_3;
    pub const _KERNEL_CAPABILITY_U32S: usize = _LINUX_CAPABILITY_U32S_3;

    pub const CAP_CHOWN: u32 = 0;
    pub const CAP_DAC_OVERRIDE: u32 = 1;
    pub const CAP_DAC_READ_SEARCH: u32 = 2;
    pub const CAP_FOWNER: u32 = 3;
    pub const CAP_FSETID: u32 = 4;
    pub const CAP_KILL: u32 = 5;
    pub const CAP_SETGID: u32 = 6;
    pub const CAP_SETUID: u32 = 7;
    pub const CAP_SETPCAP: u32 = 8;
    pub const CAP_LINUX_IMMUTABLE: u32 = 9;
    pub const CAP_NET_BIND_SERVICE: u32 = 10;
    pub const CAP_NET_BROADCAST: u32 = 11;
    pub const CAP_NET_ADMIN: u32 = 12;
    pub const CAP_NET_RAW: u32 = 13;
    pub const CAP_IPC_LOCK: u32 = 14;
    pub const CAP_IPC_OWNER: u32 = 15;
    pub const CAP_SYS_MODULE: u32 = 16;
    pub const CAP_SYS_RAWIO: u32 = 17;
    pub const CAP_SYS_CHROOT: u32 = 18;
    pub const CAP_SYS_PTRACE: u32 = 19;
    pub const CAP_SYS_PACCT: u32 = 20;
    pub const CAP_SYS_ADMIN: u32 = 21;
    pub const CAP_SYS_BOOT: u32 = 22;
    pub const CAP_SYS_NICE: u32 = 23;
    pub const CAP_SYS_RESOURCE: u32 = 24;
    pub const CAP_SYS_TIME: u32 = 25;
    pub const CAP_SYS_TTY_CONFIG: u32 = 26;
    pub const CAP_MKNOD: u32 = 27;
    pub const CAP_LEASE: u32 = 28;
    pub const CAP_AUDIT_WRITE: u32 = 29;
    pub const CAP_AUDIT_CONTROL: u32 = 30;
    pub const CAP_SETFCAP: u32 = 31;
    pub const CAP_MAC_OVERRIDE: u32 = 32;
    pub const CAP_MAC_ADMIN: u32 = 33;
    pub const CAP_SYSLOG: u32 = 34;
    pub const CAP_WAKE_ALARM: u32 = 35;
    pub const CAP_BLOCK_SUSPEND: u32 = 36;
    pub const CAP_AUDIT_READ: u32 = 37;
    pub const CAP_PERFMON: u32 = 38;
    pub const CAP_BPF: u32 = 39;
    pub const CAP_CHECKPOINT_RESTORE: u32 = 40;

    pub const CAP_LAST_CAP: u32 = CAP_CHECKPOINT_RESTORE;
    pub const CAP_VALID_MASK: u64 = (1u64 << (CAP_LAST_CAP + 1)) - 1;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(C)]
    pub struct UserCapHeader {
        pub version: u32,
        pub pid: i32,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[repr(C)]
    pub struct UserCapData {
        pub effective: u32,
        pub permitted: u32,
        pub inheritable: u32,
    }

    pub const PR_SET_PDEATHSIG: u32 = 1;
    pub const PR_GET_PDEATHSIG: u32 = 2;
    pub const PR_GET_DUMPABLE: u32 = 3;
    pub const PR_SET_DUMPABLE: u32 = 4;
    pub const PR_GET_UNALIGN: u32 = 5;
    pub const PR_SET_UNALIGN: u32 = 6;
    pub const PR_GET_KEEPCAPS: u32 = 7;
    pub const PR_SET_KEEPCAPS: u32 = 8;
    pub const PR_GET_FPEMU: u32 = 9;
    pub const PR_SET_FPEMU: u32 = 10;
    pub const PR_GET_FPEXC: u32 = 11;
    pub const PR_SET_FPEXC: u32 = 12;
    pub const PR_GET_TIMING: u32 = 13;
    pub const PR_SET_TIMING: u32 = 14;
    pub const PR_SET_NAME: u32 = 15;
    pub const PR_GET_NAME: u32 = 16;
    pub const PR_GET_ENDIAN: u32 = 19;
    pub const PR_SET_ENDIAN: u32 = 20;
    pub const PR_GET_SECCOMP: u32 = 21;
    pub const PR_SET_SECCOMP: u32 = 22;
    pub const PR_CAPBSET_READ: u32 = 23;
    pub const PR_CAPBSET_DROP: u32 = 24;
    pub const PR_GET_TSC: u32 = 25;
    pub const PR_SET_TSC: u32 = 26;
    pub const PR_GET_SECUREBITS: u32 = 27;
    pub const PR_SET_SECUREBITS: u32 = 28;
    pub const PR_SET_TIMERSLACK: u32 = 29;
    pub const PR_GET_TIMERSLACK: u32 = 30;
    pub const PR_TASK_PERF_EVENTS_DISABLE: u32 = 31;
    pub const PR_TASK_PERF_EVENTS_ENABLE: u32 = 32;
    pub const PR_MCE_KILL: u32 = 33;
    pub const PR_MCE_KILL_GET: u32 = 34;
    pub const PR_SET_MM: u32 = 35;
    pub const PR_SET_PTRACER: u32 = 0x59616d61;
    pub const PR_SET_CHILD_SUBREAPER: u32 = 36;
    pub const PR_GET_CHILD_SUBREAPER: u32 = 37;
    pub const PR_SET_NO_NEW_PRIVS: u32 = 38;
    pub const PR_GET_NO_NEW_PRIVS: u32 = 39;
    pub const PR_GET_TID_ADDRESS: u32 = 40;
    pub const PR_SET_THP_DISABLE: u32 = 41;
    pub const PR_GET_THP_DISABLE: u32 = 42;
    pub const PR_MPX_ENABLE_MANAGEMENT: u32 = 43;
    pub const PR_MPX_DISABLE_MANAGEMENT: u32 = 44;
    pub const PR_SET_FP_MODE: u32 = 45;
    pub const PR_GET_FP_MODE: u32 = 46;
    pub const PR_CAP_AMBIENT: u32 = 47;
    pub const PR_CAP_AMBIENT_IS_SET: u32 = 1;
    pub const PR_CAP_AMBIENT_RAISE: u32 = 2;
    pub const PR_CAP_AMBIENT_LOWER: u32 = 3;
    pub const PR_CAP_AMBIENT_CLEAR_ALL: u32 = 4;
    pub const PR_SVE_SET_VL: u32 = 50;
    pub const PR_SVE_GET_VL: u32 = 51;
    pub const PR_GET_SPECULATION_CTRL: u32 = 52;
    pub const PR_SET_SPECULATION_CTRL: u32 = 53;
    pub const PR_PAC_RESET_KEYS: u32 = 54;
    pub const PR_SET_TAGGED_ADDR_CTRL: u32 = 55;
    pub const PR_GET_TAGGED_ADDR_CTRL: u32 = 56;
    pub const PR_SET_IO_FLUSHER: u32 = 57;
    pub const PR_GET_IO_FLUSHER: u32 = 58;
    pub const PR_SET_SYSCALL_USER_DISPATCH: u32 = 59;
    pub const PR_PAC_SET_ENABLED_KEYS: u32 = 60;
    pub const PR_PAC_GET_ENABLED_KEYS: u32 = 61;
    pub const PR_SCHED_CORE: u32 = 62;
    pub const PR_SME_SET_VL: u32 = 63;
    pub const PR_SME_GET_VL: u32 = 64;
    pub const PR_SET_MDWE: u32 = 65;
    pub const PR_GET_MDWE: u32 = 66;
    pub const PR_SET_VMA: u32 = 0x53564d41;
    pub const PR_GET_AUXV: u32 = 0x41555856;
    pub const PR_SET_MEMORY_MERGE: u32 = 67;
    pub const PR_GET_MEMORY_MERGE: u32 = 68;
    pub const PR_RISCV_V_SET_CONTROL: u32 = 69;
    pub const PR_RISCV_V_GET_CONTROL: u32 = 70;

    pub const SECUREBITS_DEFAULT: u32 = 0;

    pub const SECURE_NOROOT: u32 = 0;
    pub const SECURE_NOROOT_LOCKED: u32 = 1;
    pub const SECURE_NO_SETUID_FIXUP: u32 = 2;
    pub const SECURE_NO_SETUID_FIXUP_LOCKED: u32 = 3;
    pub const SECURE_KEEP_CAPS: u32 = 4;
    pub const SECURE_KEEP_CAPS_LOCKED: u32 = 5;
    pub const SECURE_NO_CAP_AMBIENT_RAISE: u32 = 6;
    pub const SECURE_NO_CAP_AMBIENT_RAISE_LOCKED: u32 = 7;

    #[inline(always)]
    pub const fn secure_mask(bit: u32) -> u32 {
        1u32 << bit
    }

    pub const SECBIT_NOROOT: u32 = secure_mask(SECURE_NOROOT);
    pub const SECBIT_NOROOT_LOCKED: u32 = secure_mask(SECURE_NOROOT_LOCKED);
    pub const SECBIT_NO_SETUID_FIXUP: u32 = secure_mask(SECURE_NO_SETUID_FIXUP);
    pub const SECBIT_NO_SETUID_FIXUP_LOCKED: u32 = secure_mask(SECURE_NO_SETUID_FIXUP_LOCKED);
    pub const SECBIT_KEEP_CAPS: u32 = secure_mask(SECURE_KEEP_CAPS);
    pub const SECBIT_KEEP_CAPS_LOCKED: u32 = secure_mask(SECURE_KEEP_CAPS_LOCKED);
    pub const SECBIT_NO_CAP_AMBIENT_RAISE: u32 = secure_mask(SECURE_NO_CAP_AMBIENT_RAISE);
    pub const SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED: u32 =
        secure_mask(SECURE_NO_CAP_AMBIENT_RAISE_LOCKED);

    pub const SECURE_ALL_BITS: u32 =
        SECBIT_NOROOT | SECBIT_NO_SETUID_FIXUP | SECBIT_KEEP_CAPS | SECBIT_NO_CAP_AMBIENT_RAISE;
    pub const SECURE_ALL_LOCKS: u32 = SECURE_ALL_BITS << 1;

    #[inline(always)]
    pub const fn cap_valid(cap: u32) -> bool {
        cap <= CAP_LAST_CAP
    }

    #[inline(always)]
    pub const fn cap_to_index(cap: u32) -> usize {
        (cap >> 5) as usize
    }

    #[inline(always)]
    pub const fn cap_to_mask(cap: u32) -> u32 {
        1u32 << (cap & 31)
    }
}
