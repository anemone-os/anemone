pub mod native {}

pub mod linux {
    //! Linux socket ABI shared by RV64 and LA64.

    use core::mem::{align_of, offset_of, size_of};

    pub const AF_UNIX: i32 = 1;
    pub const AF_LOCAL: i32 = AF_UNIX;
    pub const PF_UNIX: i32 = AF_UNIX;
    pub const AF_INET: i32 = 2;
    pub const SOCK_STREAM: i32 = 1;
    pub const SOCK_DGRAM: i32 = 2;
    pub const SOCK_NONBLOCK: i32 = 0x0800;
    pub const SOCK_CLOEXEC: i32 = 0x0008_0000;
    pub const IPPROTO_UDP: i32 = 17;
    pub const MSG_DONTWAIT: i32 = 0x40;

    #[allow(non_camel_case_types)]
    pub type socklen_t = u32;

    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    #[repr(C)]
    pub struct InAddr {
        pub s_addr: u32,
    }

    impl InAddr {
        pub const ANY: Self = Self::new([0, 0, 0, 0]);

        pub const fn new(octets: [u8; 4]) -> Self {
            Self {
                // Preserve network-order bytes in memory on either supported
                // little-endian architecture.
                s_addr: u32::from_ne_bytes(octets),
            }
        }

        pub const fn octets(self) -> [u8; 4] {
            self.s_addr.to_ne_bytes()
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    #[repr(C)]
    pub struct SockAddrIn {
        pub sin_family: u16,
        pub sin_port: u16,
        pub sin_addr: InAddr,
        pub sin_zero: [u8; 8],
    }

    pub const UNIX_PATH_MAX: usize = 108;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    #[repr(C)]
    pub struct SockAddrUn {
        pub sun_family: u16,
        pub sun_path: [u8; UNIX_PATH_MAX],
    }

    impl Default for SockAddrUn {
        fn default() -> Self {
            Self {
                sun_family: AF_UNIX as u16,
                sun_path: [0; UNIX_PATH_MAX],
            }
        }
    }

    impl SockAddrIn {
        pub const fn new(address: [u8; 4], port: u16) -> Self {
            Self {
                sin_family: AF_INET as u16,
                sin_port: port.to_be(),
                sin_addr: InAddr::new(address),
                sin_zero: [0; 8],
            }
        }

        pub const fn address(self) -> [u8; 4] {
            self.sin_addr.octets()
        }

        pub const fn port(self) -> u16 {
            u16::from_be(self.sin_port)
        }
    }

    impl Default for SockAddrIn {
        fn default() -> Self {
            Self::new([0; 4], 0)
        }
    }

    const _: [(); 4] = [(); size_of::<InAddr>()];
    const _: [(); 4] = [(); align_of::<InAddr>()];
    const _: [(); 16] = [(); size_of::<SockAddrIn>()];
    const _: [(); 4] = [(); align_of::<SockAddrIn>()];
    const _: [(); 0] = [(); offset_of!(SockAddrIn, sin_family)];
    const _: [(); 2] = [(); offset_of!(SockAddrIn, sin_port)];
    const _: [(); 4] = [(); offset_of!(SockAddrIn, sin_addr)];
    const _: [(); 8] = [(); offset_of!(SockAddrIn, sin_zero)];
    const _: [(); 110] = [(); size_of::<SockAddrUn>()];
    const _: [(); 2] = [(); align_of::<SockAddrUn>()];
    const _: [(); 0] = [(); offset_of!(SockAddrUn, sun_family)];
    const _: [(); 2] = [(); offset_of!(SockAddrUn, sun_path)];
}
