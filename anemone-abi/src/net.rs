pub mod native {}

pub mod linux {
    //! Linux socket ABI shared by RV64 and LA64.

    use core::mem::{align_of, offset_of, size_of};

    use crate::RawUserAddr64;

    pub const AF_UNSPEC: i32 = 0;
    pub const AF_UNIX: i32 = 1;
    pub const AF_LOCAL: i32 = AF_UNIX;
    pub const PF_UNIX: i32 = AF_UNIX;
    pub const AF_INET: i32 = 2;
    pub const SOCK_STREAM: i32 = 1;
    pub const SOCK_DGRAM: i32 = 2;
    pub const SOCK_RAW: i32 = 3;
    pub const SOCK_SEQPACKET: i32 = 5;
    pub const SOCK_NONBLOCK: i32 = 0x0800;
    pub const SOCK_CLOEXEC: i32 = 0x0008_0000;
    pub const IPPROTO_IP: i32 = 0;
    pub const IPPROTO_ICMP: i32 = 1;
    pub const IPPROTO_TCP: i32 = 6;
    pub const IPPROTO_UDP: i32 = 17;
    pub const IP_TOS: i32 = 1;
    pub const IP_TTL: i32 = 2;
    pub const IP_RECVERR: i32 = 11;
    pub const SOL_SOCKET: i32 = 1;
    pub const SOL_RAW: i32 = 255;
    pub const ICMP_FILTER: i32 = 1;
    pub const TCP_NODELAY: i32 = 1;
    pub const SO_REUSEADDR: i32 = 2;
    pub const SO_TYPE: i32 = 3;
    pub const SO_ERROR: i32 = 4;
    pub const SO_ACCEPTCONN: i32 = 30;
    pub const SO_PROTOCOL: i32 = 38;
    pub const SO_DOMAIN: i32 = 39;
    pub const SHUT_RD: i32 = 0;
    pub const SHUT_WR: i32 = 1;
    pub const SHUT_RDWR: i32 = 2;
    pub const MSG_PEEK: i32 = 0x02;
    pub const MSG_CTRUNC: i32 = 0x08;
    pub const MSG_TRUNC: i32 = 0x20;
    pub const MSG_DONTWAIT: i32 = 0x40;
    pub const MSG_ERRQUEUE: i32 = 0x2000;
    pub const MSG_NOSIGNAL: i32 = 0x4000;
    pub const SO_EE_ORIGIN_ICMP: u8 = 2;

    #[allow(non_camel_case_types)]
    pub type socklen_t = u32;

    #[derive(
        Clone,
        Copy,
        Debug,
        Default,
        Eq,
        PartialEq,
        zerocopy::FromBytes,
        zerocopy::Immutable,
        zerocopy::IntoBytes,
    )]
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

    #[derive(
        Clone,
        Copy,
        Debug,
        Eq,
        PartialEq,
        zerocopy::FromBytes,
        zerocopy::Immutable,
        zerocopy::IntoBytes,
    )]
    #[repr(C)]
    pub struct SockAddrIn {
        pub sin_family: u16,
        pub sin_port: u16,
        pub sin_addr: InAddr,
        pub sin_zero: [u8; 8],
    }

    pub const UNIX_PATH_MAX: usize = 108;

    #[derive(
        Clone,
        Copy,
        Debug,
        Eq,
        PartialEq,
        zerocopy::FromBytes,
        zerocopy::Immutable,
        zerocopy::IntoBytes,
    )]
    #[repr(C)]
    pub struct SockAddrUn {
        pub sun_family: u16,
        pub sun_path: [u8; UNIX_PATH_MAX],
    }

    /// 64-bit asm-generic Linux `struct user_msghdr` layout.
    ///
    /// RV64 and LA64 share this representation. Kernel-internal Socket and
    /// family APIs must translate it at the syscall boundary rather than carry
    /// raw pointers or Linux field ordering into their operation types.
    #[derive(
        Clone,
        Copy,
        Debug,
        Default,
        Eq,
        PartialEq,
        zerocopy::FromBytes,
        zerocopy::Immutable,
        zerocopy::IntoBytes,
    )]
    #[repr(C)]
    pub struct MsgHdr {
        pub msg_name: RawUserAddr64,
        pub msg_namelen: i32,
        pub __pad0: u32,
        pub msg_iov: RawUserAddr64,
        pub msg_iovlen: u64,
        pub msg_control: RawUserAddr64,
        pub msg_controllen: u64,
        pub msg_flags: u32,
        pub __pad1: u32,
    }

    /// 64-bit asm-generic Linux `struct mmsghdr` layout.
    #[derive(
        Clone,
        Copy,
        Debug,
        Default,
        Eq,
        PartialEq,
        zerocopy::FromBytes,
        zerocopy::Immutable,
        zerocopy::IntoBytes,
    )]
    #[repr(C)]
    pub struct MMsgHdr {
        pub msg_hdr: MsgHdr,
        pub msg_len: u32,
        pub __pad: u32,
    }

    /// 64-bit asm-generic Linux `struct cmsghdr` layout.
    #[derive(
        Clone,
        Copy,
        Debug,
        Default,
        Eq,
        PartialEq,
        zerocopy::FromBytes,
        zerocopy::Immutable,
        zerocopy::IntoBytes,
    )]
    #[repr(C)]
    pub struct CMsgHdr {
        pub cmsg_len: u64,
        pub cmsg_level: i32,
        pub cmsg_type: i32,
    }

    #[derive(
        Clone,
        Copy,
        Debug,
        Default,
        Eq,
        PartialEq,
        zerocopy::FromBytes,
        zerocopy::Immutable,
        zerocopy::IntoBytes,
    )]
    #[repr(C)]
    pub struct SockExtendedErr {
        pub ee_errno: u32,
        pub ee_origin: u8,
        pub ee_type: u8,
        pub ee_code: u8,
        pub ee_pad: u8,
        pub ee_info: u32,
        pub ee_data: u32,
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
    const _: [(); 56] = [(); size_of::<MsgHdr>()];
    const _: [(); 8] = [(); align_of::<MsgHdr>()];
    const _: [(); 0] = [(); offset_of!(MsgHdr, msg_name)];
    const _: [(); 8] = [(); offset_of!(MsgHdr, msg_namelen)];
    const _: [(); 16] = [(); offset_of!(MsgHdr, msg_iov)];
    const _: [(); 24] = [(); offset_of!(MsgHdr, msg_iovlen)];
    const _: [(); 32] = [(); offset_of!(MsgHdr, msg_control)];
    const _: [(); 40] = [(); offset_of!(MsgHdr, msg_controllen)];
    const _: [(); 48] = [(); offset_of!(MsgHdr, msg_flags)];
    const _: [(); 64] = [(); size_of::<MMsgHdr>()];
    const _: [(); 8] = [(); align_of::<MMsgHdr>()];
    const _: [(); 0] = [(); offset_of!(MMsgHdr, msg_hdr)];
    const _: [(); 56] = [(); offset_of!(MMsgHdr, msg_len)];
    const _: [(); 16] = [(); size_of::<CMsgHdr>()];
    const _: [(); 8] = [(); align_of::<CMsgHdr>()];
    const _: [(); 0] = [(); offset_of!(CMsgHdr, cmsg_len)];
    const _: [(); 8] = [(); offset_of!(CMsgHdr, cmsg_level)];
    const _: [(); 12] = [(); offset_of!(CMsgHdr, cmsg_type)];
    const _: [(); 16] = [(); size_of::<SockExtendedErr>()];
    const _: [(); 4] = [(); align_of::<SockExtendedErr>()];
    const _: [(); 0] = [(); offset_of!(SockExtendedErr, ee_errno)];
    const _: [(); 4] = [(); offset_of!(SockExtendedErr, ee_origin)];
    const _: [(); 8] = [(); offset_of!(SockExtendedErr, ee_info)];
    const _: [(); 12] = [(); offset_of!(SockExtendedErr, ee_data)];
}
