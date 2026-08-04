use anemone_abi::fs::linux::{
    dev_t as linux_dev_t, mode as linux_mode,
    stat::Stat as LinuxStat,
    statx::{self as linux_statx, StatX as LinuxStatX, StatXTimestamp as LinuxStatXTimestamp},
};
use core::{
    fmt::{Debug, Display},
    time::Duration,
};

use crate::{
    fs::{
        file::FileMode,
        lock::{FlockDomain, PosixLockDomain},
        permission::FsPermChecker,
        pipe::FifoAnchor,
    },
    prelude::{vmo::VmObject, *},
    task::credentials::cap::{Capability, FileCapabilities},
    utils::any_opaque::AnyOpaque,
};

mod metadata;
mod object;
mod ops;

pub(super) use self::object::Inode;
pub(crate) use self::ops::{RenameFlags, reject_make_node};
pub use self::{
    metadata::*,
    object::InodeRef,
    ops::{InodeOps, MakeNodeDescription, OpenedFile},
};
