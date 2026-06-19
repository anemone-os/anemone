use crate::prelude::*;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MountAttrFlags: u32 {
        // First legacy mount API stage only closes per-mount read-only
        // enforcement. Operation bits such as MS_BIND/MS_MOVE/MS_REMOUNT must
        // never be stored here; they are syscall parser requests.
        const RDONLY = 1 << 0;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MountFlags: u32 {
        // The filesystem is mounted read-only. Kernel will enforce this by
        // disallowing any write operations on the mount.
        const RDONLY = 1 << 0;
    }
}

impl From<MountAttrFlags> for MountFlags {
    fn from(value: MountAttrFlags) -> Self {
        let mut flags = Self::empty();
        if value.contains(MountAttrFlags::RDONLY) {
            flags |= Self::RDONLY;
        }
        flags
    }
}
