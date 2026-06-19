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
