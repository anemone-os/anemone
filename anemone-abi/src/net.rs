//! Socket-related definitions aligned with the Linux ABI.

/// Linux `socket(2)`, `send(2)`, and related constants (`AF_*`, `SOCK_*`, `IPPROTO_*`, …).
pub mod linux {
    /// Address families
    pub mod af {
        pub const AF_INET: u32 = 2;
    }

    /// Socket types (`socket(2)` type argument, before masking flags).
    pub mod sock {
        use crate::fs::linux::open::{O_CLOEXEC, O_NONBLOCK};

        pub const SOCK_STREAM: u32 = 1;
        pub const SOCK_DGRAM: u32 = 2;
        pub const SOCK_RAW: u32 = 3;

        /// Linux defines these equal to the `O_*` bits used with `open(2)`.
        pub const SOCK_NONBLOCK: u32 = O_NONBLOCK;
        pub const SOCK_CLOEXEC: u32 = O_CLOEXEC;
    }

    /// `send`, `recv` flags (subset).
    pub mod msg {
        pub const MSG_DONTWAIT: u32 = 0x40;
    }

    /// `socket(2)` protocol argument (subset; Linux `IPPROTO_*`).
    pub mod ipproto {
        pub const IP: i32 = 0;
        pub const TCP: i32 = 6;
        pub const UDP: i32 = 17;
    }
}
