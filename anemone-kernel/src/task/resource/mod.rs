//! POSIX resource management.

mod api;
pub use api::*;

use crate::{prelude::*, syscall::handler::TryFromSyscallArg};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RLimitResource {
    Cpu,
    Fsize,
    Data,
    Stack,
    Core,
    Rss,
    Nproc,
    NoFile,
    Memlock,
    As,
    Locks,
    Sigpending,
    Msgqueue,
    Nice,
    Rtprio,
    Rttime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RLimitPair {
    soft: u64,
    hard: u64,
}

impl RLimitPair {
    const fn new(soft: u64, hard: u64) -> Self {
        Self { soft, hard }
    }

    fn validate_nofile(self) -> Result<Self, SysError> {
        if self.soft > self.hard || self.hard > MAX_FD_PER_PROCESS as u64 {
            return Err(SysError::InvalidArgument);
        }
        Ok(self)
    }

    fn into_abi(self) -> anemone_abi::process::linux::resource::RLimit {
        anemone_abi::process::linux::resource::RLimit {
            rlim_cur: self.soft,
            rlim_max: self.hard,
        }
    }
}

impl From<anemone_abi::process::linux::resource::RLimit> for RLimitPair {
    fn from(value: anemone_abi::process::linux::resource::RLimit) -> Self {
        Self::new(value.rlim_cur, value.rlim_max)
    }
}

/// Process-level resource policy owned by one user thread group.
///
/// Live resource usage remains with each resource subsystem. In particular,
/// this value never caches fd-table occupancy or a file-table identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UserResourceLimits {
    nofile: RLimitPair,
}

impl UserResourceLimits {
    pub(crate) const fn new_default() -> Self {
        Self {
            nofile: RLimitPair::new(MAX_FD_PER_PROCESS as u64, MAX_FD_PER_PROCESS as u64),
        }
    }

    fn update_nofile(
        &mut self,
        proposed: RLimitPair,
        may_raise_hard: bool,
    ) -> Result<RLimitPair, SysError> {
        let proposed = proposed.validate_nofile()?;
        let old = self.nofile;
        if proposed.hard > old.hard && !may_raise_hard {
            return Err(SysError::PermissionDenied);
        }
        self.nofile = proposed;
        Ok(old)
    }
}

impl ThreadGroup {
    fn user_resource_limits(&self) -> Result<&NoIrqRwLock<UserResourceLimits>, SysError> {
        match (self.ty(), self.resource_limits.as_ref()) {
            (ThreadGroupType::User, Some(limits)) => Ok(limits),
            (ThreadGroupType::KThread, None) => Err(SysError::NoSuchProcess),
            _ => panic!("thread-group type/resource-policy shape diverged"),
        }
    }

    pub(crate) fn fork_resource_limits(&self) -> UserResourceLimits {
        *self
            .user_resource_limits()
            .expect("only user thread groups can be fork parents")
            .read()
    }

    pub(crate) fn nofile_alloc_ceiling(&self) -> crate::task::files::FdAllocCeiling {
        let soft = self
            .user_resource_limits()
            .expect("only user tasks can allocate file descriptors")
            .read()
            .nofile
            .soft;
        crate::task::files::FdAllocCeiling::new(soft as usize)
            .expect("validated RLIMIT_NOFILE must fit the fd table")
    }

    fn read_rlimit(&self, resource: RLimitResource) -> Result<RLimitPair, SysError> {
        match resource {
            RLimitResource::Cpu | RLimitResource::Fsize | RLimitResource::Nproc => {
                Ok(RLimitPair::new(u64::MAX, u64::MAX))
            },
            RLimitResource::Stack => {
                let stack = 1 << (USER_STACK_SHIFT_KB + 10);
                Ok(RLimitPair::new(stack, stack))
            },
            RLimitResource::Core => Ok(RLimitPair::new(0, 0)),
            RLimitResource::NoFile => Ok(self.user_resource_limits()?.read().nofile),
            _ => Err(SysError::NotYetImplemented),
        }
    }

    fn update_rlimit(
        &self,
        resource: RLimitResource,
        proposed: RLimitPair,
        may_raise_hard: bool,
    ) -> Result<RLimitPair, SysError> {
        match resource {
            RLimitResource::NoFile => self
                .user_resource_limits()?
                .write()
                .update_nofile(proposed, may_raise_hard),
            // These resources intentionally provide fixed compatibility
            // readback only. Accepting an inert update would falsely report a
            // policy change that no resource owner enforces.
            _ => Err(SysError::NotYetImplemented),
        }
    }
}

impl TryFromSyscallArg for RLimitResource {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        use anemone_abi::process::linux::resource::*;

        match raw as u32 {
            RLIMIT_CPU => Ok(Self::Cpu),
            RLIMIT_FSIZE => Ok(Self::Fsize),
            RLIMIT_DATA => Ok(Self::Data),
            RLIMIT_STACK => Ok(Self::Stack),
            RLIMIT_CORE => Ok(Self::Core),
            RLIMIT_RSS => Ok(Self::Rss),
            RLIMIT_NPROC => Ok(Self::Nproc),
            RLIMIT_NOFILE => Ok(Self::NoFile),
            RLIMIT_MEMLOCK => Ok(Self::Memlock),
            RLIMIT_AS => Ok(Self::As),
            RLIMIT_LOCKS => Ok(Self::Locks),
            RLIMIT_SIGPENDING => Ok(Self::Sigpending),
            RLIMIT_MSGQUEUE => Ok(Self::Msgqueue),
            RLIMIT_NICE => Ok(Self::Nice),
            RLIMIT_RTPRIO => Ok(Self::Rtprio),
            RLIMIT_RTTIME => Ok(Self::Rttime),
            _ => {
                knoticeln!("getrlimit: unknown resource ID {:#x}", raw);
                Err(SysError::InvalidArgument)
            },
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn nofile_policy_validates_pair_and_system_ceiling() {
        assert_eq!(
            RLimitPair::new(8, 7).validate_nofile(),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(
            RLimitPair::new(0, MAX_FD_PER_PROCESS as u64 + 1).validate_nofile(),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(
            RLimitPair::new(0, MAX_FD_PER_PROCESS as u64).validate_nofile(),
            Ok(RLimitPair::new(0, MAX_FD_PER_PROCESS as u64))
        );
    }

    #[kunit]
    fn nofile_update_returns_preupdate_snapshot_and_checks_hard_raise() {
        let mut limits = UserResourceLimits::new_default();
        let initial = limits.nofile;
        let lowered = RLimitPair::new(8, 16);
        assert_eq!(limits.update_nofile(lowered, false), Ok(initial));
        assert_eq!(limits.nofile, lowered);

        assert_eq!(
            limits.update_nofile(RLimitPair::new(8, 17), false),
            Err(SysError::PermissionDenied)
        );
        assert_eq!(limits.nofile, lowered);

        assert_eq!(
            limits.update_nofile(RLimitPair::new(9, 17), true),
            Ok(lowered)
        );
        assert_eq!(limits.nofile, RLimitPair::new(9, 17));
    }

    #[kunit]
    fn nofile_fork_snapshot_is_an_independent_complete_pair() {
        let parent = UserResourceLimits {
            nofile: RLimitPair::new(5, 9),
        };
        let mut child = parent;
        child.update_nofile(RLimitPair::new(3, 7), false).unwrap();
        assert_eq!(parent.nofile, RLimitPair::new(5, 9));
        assert_eq!(child.nofile, RLimitPair::new(3, 7));
    }
}
