//! Hardware probing ABI definitions.

/// Linux-compatible hardware probing ABI.
pub mod linux {
    use core::mem::size_of;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    #[repr(C)]
    pub struct RiscvHwprobe {
        pub key: i64,
        pub value: u64,
    }

    pub const KEY_MVENDORID: i64 = 0;
    pub const KEY_MARCHID: i64 = 1;
    pub const KEY_MIMPID: i64 = 2;
    pub const KEY_BASE_BEHAVIOR: i64 = 3;
    pub const BASE_BEHAVIOR_IMA: u64 = 1 << 0;
    pub const KEY_IMA_EXT_0: i64 = 4;
    pub const IMA_FD: u64 = 1 << 0;
    pub const IMA_C: u64 = 1 << 1;
    pub const KEY_CPUPERF_0: i64 = 5;
    pub const MISALIGNED_UNKNOWN: u64 = 0;

    const _: () = assert!(size_of::<RiscvHwprobe>() == 16);
}
