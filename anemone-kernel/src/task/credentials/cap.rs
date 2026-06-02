//! Capability state used by task credentials.

use anemone_abi::capability::linux as abi;

use crate::prelude::*;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Capability: u64 {
        /// Allows changing file owner and group metadata.
        const CHOWN = 1u64 << abi::CAP_CHOWN;
        /// Overrides discretionary access checks except read/search-only checks.
        const DAC_OVERRIDE = 1u64 << abi::CAP_DAC_OVERRIDE;
        /// Overrides discretionary read and directory search checks.
        const DAC_READ_SEARCH = 1u64 << abi::CAP_DAC_READ_SEARCH;
        /// Overrides file-owner checks for operations that require ownership.
        const FOWNER = 1u64 << abi::CAP_FOWNER;
        /// Allows preserving or setting setuid/setgid file mode bits.
        const FSETID = 1u64 << abi::CAP_FSETID;
        /// Allows signaling tasks across normal uid permission boundaries.
        const KILL = 1u64 << abi::CAP_KILL;
        /// Allows setgid, setregid, setresgid, setfsgid, and setgroups changes.
        const SETGID = 1u64 << abi::CAP_SETGID;
        /// Allows setuid, setreuid, setresuid, and setfsuid changes.
        const SETUID = 1u64 << abi::CAP_SETUID;
        /// Allows capability bounding-set, inheritable-set, and securebits changes.
        const SETPCAP = 1u64 << abi::CAP_SETPCAP;
        /// [NYI] Allows modifying immutable and append-only file attributes.
        const LINUX_IMMUTABLE = 1u64 << abi::CAP_LINUX_IMMUTABLE;
        /// [NYI] Allows binding sockets to privileged service ports.
        const NET_BIND_SERVICE = 1u64 << abi::CAP_NET_BIND_SERVICE;
        /// [NYI] Allows network broadcast and multicast listening operations.
        const NET_BROADCAST = 1u64 << abi::CAP_NET_BROADCAST;
        /// [NYI] Allows privileged network administration operations.
        const NET_ADMIN = 1u64 << abi::CAP_NET_ADMIN;
        /// Allows raw and packet socket operations.
        const NET_RAW = 1u64 << abi::CAP_NET_RAW;
        /// Allows mlock and SysV shared-memory lock operations.
        const IPC_LOCK = 1u64 << abi::CAP_IPC_LOCK;
        /// Overrides SysV IPC ownership permission checks.
        const IPC_OWNER = 1u64 << abi::CAP_IPC_OWNER;
        /// [NYI] Allows loading and unloading kernel modules.
        const SYS_MODULE = 1u64 << abi::CAP_SYS_MODULE;
        /// [NYI] Allows raw I/O and privileged device access.
        const SYS_RAWIO = 1u64 << abi::CAP_SYS_RAWIO;
        /// Allows chroot.
        const SYS_CHROOT = 1u64 << abi::CAP_SYS_CHROOT;
        /// [NYI] Allows ptrace across normal task permission boundaries.
        const SYS_PTRACE = 1u64 << abi::CAP_SYS_PTRACE;
        /// [NYI] Allows configuring process accounting.
        const SYS_PACCT = 1u64 << abi::CAP_SYS_PACCT;
        /// Allows broad system administration operations such as mount and umount.
        const SYS_ADMIN = 1u64 << abi::CAP_SYS_ADMIN;
        /// [NYI] Allows reboot and other system boot-control operations.
        const SYS_BOOT = 1u64 << abi::CAP_SYS_BOOT;
        /// [NYI] Allows privileged scheduler priority and affinity changes.
        const SYS_NICE = 1u64 << abi::CAP_SYS_NICE;
        /// Allows overriding and raising resource limits.
        const SYS_RESOURCE = 1u64 << abi::CAP_SYS_RESOURCE;
        /// [NYI] Allows changing system and real-time clocks.
        const SYS_TIME = 1u64 << abi::CAP_SYS_TIME;
        /// [NYI] Allows privileged tty configuration operations.
        const SYS_TTY_CONFIG = 1u64 << abi::CAP_SYS_TTY_CONFIG;
        /// [NYI] Allows privileged mknod operations.
        const MKNOD = 1u64 << abi::CAP_MKNOD;
        /// [NYI] Allows taking file leases.
        const LEASE = 1u64 << abi::CAP_LEASE;
        /// [NYI] Allows writing audit log records.
        const AUDIT_WRITE = 1u64 << abi::CAP_AUDIT_WRITE;
        /// [NYI] Allows configuring the audit subsystem.
        const AUDIT_CONTROL = 1u64 << abi::CAP_AUDIT_CONTROL;
        /// [NYI] Allows setting file capabilities.
        const SETFCAP = 1u64 << abi::CAP_SETFCAP;
        /// [NYI] Allows overriding mandatory access-control policy.
        const MAC_OVERRIDE = 1u64 << abi::CAP_MAC_OVERRIDE;
        /// [NYI] Allows configuring mandatory access-control policy.
        const MAC_ADMIN = 1u64 << abi::CAP_MAC_ADMIN;
        /// [NYI] Allows privileged syslog operations.
        const SYSLOG = 1u64 << abi::CAP_SYSLOG;
        /// [NYI] Allows triggering system wake alarms.
        const WAKE_ALARM = 1u64 << abi::CAP_WAKE_ALARM;
        /// [NYI] Allows preventing system suspend.
        const BLOCK_SUSPEND = 1u64 << abi::CAP_BLOCK_SUSPEND;
        /// [NYI] Allows reading audit logs.
        const AUDIT_READ = 1u64 << abi::CAP_AUDIT_READ;
        /// [NYI] Allows privileged performance monitoring operations.
        const PERFMON = 1u64 << abi::CAP_PERFMON;
        /// [NYI] Allows privileged BPF program and map operations.
        const BPF = 1u64 << abi::CAP_BPF;
        /// [NYI] Allows checkpoint/restore operations such as PID selection.
        const CHECKPOINT_RESTORE = 1u64 << abi::CAP_CHECKPOINT_RESTORE;

        const IMPLEMENTED = Self::CHOWN.bits()
            | Self::DAC_OVERRIDE.bits()
            | Self::DAC_READ_SEARCH.bits()
            | Self::FOWNER.bits()
            | Self::FSETID.bits()
            | Self::KILL.bits()
            | Self::SETGID.bits()
            | Self::SETUID.bits()
            | Self::SETPCAP.bits()
            | Self::NET_RAW.bits()
            | Self::IPC_LOCK.bits()
            | Self::IPC_OWNER.bits()
            | Self::SYS_CHROOT.bits()
            | Self::SYS_ADMIN.bits()
            | Self::SYS_RESOURCE.bits();
    }
}

impl Capability {
    pub fn from_number(raw: u32) -> Result<Self, SysError> {
        let bits = 1u64.checked_shl(raw).ok_or(SysError::InvalidArgument)?;
        let cap = Self::from_bits(bits).ok_or(SysError::InvalidArgument)?;
        if Self::IMPLEMENTED.contains(cap) {
            return Ok(cap);
        }

        knoticeln!("[NYI] linux capability {:?} is not supported yet", cap);
        Err(SysError::NotYetImplemented)
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct SecureBits: u32 {
        const NOROOT = abi::SECBIT_NOROOT;
        const NOROOT_LOCKED = abi::SECBIT_NOROOT_LOCKED;
        const NO_SETUID_FIXUP = abi::SECBIT_NO_SETUID_FIXUP;
        const NO_SETUID_FIXUP_LOCKED = abi::SECBIT_NO_SETUID_FIXUP_LOCKED;
        const KEEP_CAPS = abi::SECBIT_KEEP_CAPS;
        const KEEP_CAPS_LOCKED = abi::SECBIT_KEEP_CAPS_LOCKED;
        const NO_CAP_AMBIENT_RAISE = abi::SECBIT_NO_CAP_AMBIENT_RAISE;
        const NO_CAP_AMBIENT_RAISE_LOCKED = abi::SECBIT_NO_CAP_AMBIENT_RAISE_LOCKED;

        const BASE = Self::NOROOT.bits()
            | Self::NO_SETUID_FIXUP.bits()
            | Self::KEEP_CAPS.bits()
            | Self::NO_CAP_AMBIENT_RAISE.bits();
        const LOCKS = Self::NOROOT_LOCKED.bits()
            | Self::NO_SETUID_FIXUP_LOCKED.bits()
            | Self::KEEP_CAPS_LOCKED.bits()
            | Self::NO_CAP_AMBIENT_RAISE_LOCKED.bits();
    }
}

impl SecureBits {
    pub fn from_number(raw: u32) -> Result<Self, SysError> {
        Self::from_bits(raw).ok_or(SysError::InvalidArgument)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileCapabilities {
    permitted: Capability,
    effective: bool,
    inheritable: Capability,
}

impl FileCapabilities {
    pub const fn new(permitted: Capability, effective: bool, inheritable: Capability) -> Self {
        Self {
            permitted,
            effective,
            inheritable,
        }
    }

    pub const fn empty() -> Self {
        Self {
            permitted: Capability::empty(),
            effective: false,
            inheritable: Capability::empty(),
        }
    }

    pub fn permitted(self) -> Capability {
        self.permitted
    }

    pub fn effective(self) -> bool {
        self.effective
    }

    pub fn inheritable(self) -> Capability {
        self.inheritable
    }

    pub fn is_empty(self) -> bool {
        self.permitted.is_empty() && !self.effective && self.inheritable.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredCapabilities {
    permitted: Capability,
    effective: Capability,
    inheritable: Capability,
    bounding: Capability,
    ambient: Capability,
    securebits: SecureBits,
}

impl CredCapabilities {
    pub fn new_root() -> Self {
        let supported = Capability::IMPLEMENTED;
        Self {
            permitted: supported,
            effective: supported,
            bounding: supported,
            inheritable: Capability::empty(),
            ambient: Capability::empty(),
            securebits: SecureBits::empty(),
        }
    }

    pub fn permitted(&self) -> Capability {
        self.permitted
    }

    pub fn effective(&self) -> Capability {
        self.effective
    }

    pub fn inheritable(&self) -> Capability {
        self.inheritable
    }

    pub fn bounding(&self) -> Capability {
        self.bounding
    }

    pub fn ambient(&self) -> Capability {
        self.ambient
    }

    pub fn securebits(&self) -> SecureBits {
        self.securebits
    }

    pub fn set_securebits(&mut self, securebits: SecureBits) {
        self.securebits = securebits;
    }

    pub fn set_effective(&mut self, effective: Capability) {
        self.effective = effective;
    }

    pub fn set_permitted(&mut self, permitted: Capability) {
        self.permitted = permitted;
    }

    pub fn set_inheritable(&mut self, inheritable: Capability) {
        self.inheritable = inheritable;
    }

    pub fn set_bounding(&mut self, bounding: Capability) {
        self.bounding = bounding;
    }

    pub fn set_ambient(&mut self, ambient: Capability) {
        self.ambient = ambient;
    }
}
