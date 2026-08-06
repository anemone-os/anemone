//! Typed socket wrappers plus narrow raw Linux ABI conformance entries.

use anemone_abi::{
    errno::{EINVAL, Errno},
    net::linux::{
        AF_INET, AF_UNIX, IPPROTO_ICMP, IPPROTO_UDP, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK,
        SOCK_RAW, SOCK_STREAM, SOL_SOCKET, MsgHdr, SockAddrIn, SockAddrUn, UNIX_PATH_MAX, socklen_t,
    },
};
use bitflags::bitflags;

use crate::{os::linux::fs::Fd, sys::linux::net};

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct SocketFlags: i32 {
        const NONBLOCK = SOCK_NONBLOCK;
        const CLOEXEC = SOCK_CLOEXEC;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct MessageFlags: i32 {
        const DONTWAIT = anemone_abi::net::linux::MSG_DONTWAIT;
        const PEEK = anemone_abi::net::linux::MSG_PEEK;
        const TRUNC = anemone_abi::net::linux::MSG_TRUNC;
        const NOSIGNAL = anemone_abi::net::linux::MSG_NOSIGNAL;
    }
}

pub fn udp_socket(flags: SocketFlags) -> Result<Fd, Errno> {
    net::socket(
        AF_INET as u64,
        (SOCK_DGRAM | flags.bits()) as u64,
        IPPROTO_UDP as u64,
    )
    .map(|fd| fd as Fd)
}

pub fn icmp_raw_socket(flags: SocketFlags) -> Result<Fd, Errno> {
    net::socket(
        AF_INET as u64,
        (SOCK_RAW | flags.bits()) as u64,
        IPPROTO_ICMP as u64,
    )
    .map(|fd| fd as Fd)
}

pub fn unix_stream_pair(flags: SocketFlags) -> Result<(Fd, Fd), Errno> {
    let mut pair = [0i32; 2];
    net::socketpair(
        AF_UNIX as u64,
        (SOCK_STREAM | flags.bits()) as u64,
        0,
        pair.as_mut_ptr() as u64,
    )
    .map(|_| (pair[0] as Fd, pair[1] as Fd))
}

pub fn unix_stream_socket(flags: SocketFlags) -> Result<Fd, Errno> {
    net::socket(
        AF_UNIX as u64,
        (SOCK_STREAM | flags.bits()) as u64,
        0,
    )
    .map(|fd| fd as Fd)
}

pub fn unix_path_address(pathname: &[u8]) -> Result<(SockAddrUn, socklen_t), Errno> {
    if pathname.is_empty() || pathname.len() > UNIX_PATH_MAX {
        return Err(EINVAL);
    }
    let mut address = SockAddrUn::default();
    address.sun_path[..pathname.len()].copy_from_slice(pathname);
    let len = core::mem::offset_of!(SockAddrUn, sun_path) + pathname.len();
    Ok((address, len as socklen_t))
}

pub fn bind_unix_path(fd: Fd, pathname: &[u8]) -> Result<(), Errno> {
    let (address, len) = unix_path_address(pathname)?;
    net::bind(
        fd as u64,
        &address as *const SockAddrUn as u64,
        len as u64,
    )
    .map(|_| ())
}

pub fn listen(fd: Fd, backlog: i32) -> Result<(), Errno> {
    net::listen(fd as u64, backlog).map(|_| ())
}

pub fn connect_unix_path(fd: Fd, pathname: &[u8]) -> Result<(), Errno> {
    let (address, len) = unix_path_address(pathname)?;
    net::connect(
        fd as u64,
        &address as *const SockAddrUn as u64,
        len as u64,
    )
    .map(|_| ())
}

pub fn accept_unix(fd: Fd) -> Result<Fd, Errno> {
    net::accept(fd as u64, 0, 0).map(|accepted| accepted as Fd)
}

pub fn accept4_unix(fd: Fd, flags: SocketFlags) -> Result<Fd, Errno> {
    net::accept4(fd as u64, 0, 0, flags.bits()).map(|accepted| accepted as Fd)
}

pub fn accept4_unix_raw(
    fd: Fd,
    address: *mut SockAddrUn,
    len: *mut socklen_t,
    flags: i32,
) -> Result<Fd, Errno> {
    net::accept4(
        fd as u64,
        address as u64,
        len as u64,
        flags,
    )
    .map(|accepted| accepted as Fd)
}

pub fn getsockname_unix_raw(
    fd: Fd,
    address: *mut SockAddrUn,
    len: *mut socklen_t,
) -> Result<(), Errno> {
    net::getsockname(fd as u64, address as u64, len as u64).map(|_| ())
}

pub fn getpeername_unix_raw(
    fd: Fd,
    address: *mut SockAddrUn,
    len: *mut socklen_t,
) -> Result<(), Errno> {
    net::getpeername(fd as u64, address as u64, len as u64).map(|_| ())
}

/// Raw socketpair entry for pointer and tuple conformance tests.
pub unsafe fn socketpair_raw(
    family: i32,
    socket_type: i32,
    protocol: i32,
    pair: *mut i32,
) -> Result<(), Errno> {
    net::socketpair(
        family as i64 as u64,
        socket_type as i64 as u64,
        protocol as i64 as u64,
        pair as u64,
    )
    .map(|_| ())
}

pub fn bind_ipv4(fd: Fd, address: SockAddrIn) -> Result<(), Errno> {
    net::bind(
        fd as u64,
        &address as *const SockAddrIn as u64,
        core::mem::size_of::<SockAddrIn>() as u64,
    )
    .map(|_| ())
}

pub fn connect_ipv4(fd: Fd, address: SockAddrIn) -> Result<(), Errno> {
    net::connect(
        fd as u64,
        &address as *const SockAddrIn as u64,
        core::mem::size_of::<SockAddrIn>() as u64,
    )
    .map(|_| ())
}

pub fn disconnect_ipv4(fd: Fd) -> Result<(), Errno> {
    let family = anemone_abi::net::linux::AF_UNSPEC as u16;
    net::connect(
        fd as u64,
        &family as *const u16 as u64,
        core::mem::size_of::<u16>() as u64,
    )
    .map(|_| ())
}

pub fn getsockname_ipv4(fd: Fd) -> Result<SockAddrIn, Errno> {
    let mut address = SockAddrIn::default();
    let mut len = core::mem::size_of::<SockAddrIn>() as socklen_t;
    net::getsockname(
        fd as u64,
        &mut address as *mut SockAddrIn as u64,
        &mut len as *mut socklen_t as u64,
    )?;
    assert_eq!(len as usize, core::mem::size_of::<SockAddrIn>());
    Ok(address)
}

pub fn getpeername_ipv4(fd: Fd) -> Result<SockAddrIn, Errno> {
    let mut address = SockAddrIn::default();
    let mut len = core::mem::size_of::<SockAddrIn>() as socklen_t;
    net::getpeername(
        fd as u64,
        &mut address as *mut SockAddrIn as u64,
        &mut len as *mut socklen_t as u64,
    )?;
    assert_eq!(len as usize, core::mem::size_of::<SockAddrIn>());
    Ok(address)
}

pub fn sendto_ipv4(
    fd: Fd,
    payload: &[u8],
    flags: MessageFlags,
    peer: SockAddrIn,
) -> Result<usize, Errno> {
    net::sendto(
        fd as u64,
        payload.as_ptr() as u64,
        payload.len() as u64,
        flags.bits() as u64,
        &peer as *const SockAddrIn as u64,
        core::mem::size_of::<SockAddrIn>() as u64,
    )
    .map(|written| written as usize)
}

pub fn recvfrom_ipv4(
    fd: Fd,
    payload: &mut [u8],
    flags: MessageFlags,
) -> Result<(usize, SockAddrIn), Errno> {
    let mut peer = SockAddrIn::default();
    let mut peer_len = core::mem::size_of::<SockAddrIn>() as socklen_t;
    let received = net::recvfrom(
        fd as u64,
        payload.as_mut_ptr() as u64,
        payload.len() as u64,
        flags.bits() as u64,
        &mut peer as *mut SockAddrIn as u64,
        &mut peer_len as *mut socklen_t as u64,
    )?;
    assert_eq!(peer_len as usize, core::mem::size_of::<SockAddrIn>());
    Ok((received as usize, peer))
}

pub fn shutdown(fd: Fd, how: i32) -> Result<(), Errno> {
    net::shutdown(fd as u64, how as i64 as u64).map(|_| ())
}

pub unsafe fn sendto_raw(
    fd: i32,
    buf: *const u8,
    len: usize,
    flags: i32,
    address: *const u8,
    address_len: u32,
) -> Result<usize, Errno> {
    net::sendto(
        fd as i64 as u64,
        buf as u64,
        len as u64,
        flags as i64 as u64,
        address as u64,
        address_len as u64,
    )
    .map(|written| written as usize)
}

pub unsafe fn recvfrom_raw(
    fd: i32,
    buf: *mut u8,
    len: usize,
    flags: i32,
    address: *mut u8,
    address_len: *mut socklen_t,
) -> Result<usize, Errno> {
    net::recvfrom(
        fd as i64 as u64,
        buf as u64,
        len as u64,
        flags as i64 as u64,
        address as u64,
        address_len as u64,
    )
    .map(|read| read as usize)
}

/// Raw `sendmsg(2)` entry for focused message-layout and fault conformance.
pub unsafe fn sendmsg_raw(fd: i32, message: *const MsgHdr, flags: i32) -> Result<usize, Errno> {
    net::sendmsg(fd as i64 as u64, message as u64, flags as i64 as u64)
        .map(|written| written as usize)
}

/// Raw `recvmsg(2)` entry for focused message-layout and output-order conformance.
pub unsafe fn recvmsg_raw(fd: i32, message: *mut MsgHdr, flags: i32) -> Result<usize, Errno> {
    net::recvmsg(fd as i64 as u64, message as u64, flags as i64 as u64)
        .map(|read| read as usize)
}

pub unsafe fn getsockopt_raw(
    fd: i32,
    option: i32,
    value: *mut u8,
    len: *mut i32,
) -> Result<(), Errno> {
    net::getsockopt(
        fd as i64 as u64,
        SOL_SOCKET as u64,
        option as i64 as u64,
        value as u64,
        len as u64,
    )
    .map(|_| ())
}

pub unsafe fn getsockopt_level_raw(
    fd: i32,
    level: i32,
    option: i32,
    value: *mut u8,
    len: *mut i32,
) -> Result<(), Errno> {
    net::getsockopt(
        fd as i64 as u64,
        level as i64 as u64,
        option as i64 as u64,
        value as u64,
        len as u64,
    )
    .map(|_| ())
}

pub unsafe fn setsockopt_raw(
    fd: i32,
    option: i32,
    value: *const u8,
    len: i32,
) -> Result<(), Errno> {
    net::setsockopt(
        fd as i64 as u64,
        SOL_SOCKET as u64,
        option as i64 as u64,
        value as u64,
        len as i64 as u64,
    )
    .map(|_| ())
}

pub unsafe fn setsockopt_level_raw(
    fd: i32,
    level: i32,
    option: i32,
    value: *const u8,
    len: i32,
) -> Result<(), Errno> {
    net::setsockopt(
        fd as i64 as u64,
        level as i64 as u64,
        option as i64 as u64,
        value as u64,
        len as i64 as u64,
    )
    .map(|_| ())
}

/// Raw socket creation for ABI rejection and flag-conformance tests.
pub unsafe fn socket_raw(family: i32, socket_type: i32, protocol: i32) -> Result<Fd, Errno> {
    net::socket(
        family as i64 as u64,
        socket_type as i64 as u64,
        protocol as i64 as u64,
    )
    .map(|fd| fd as Fd)
}

/// Raw bind entry. Pointer and length may intentionally be invalid in a test.
pub unsafe fn bind_raw(fd: i32, address: *const u8, len: u32) -> Result<(), Errno> {
    net::bind(fd as i64 as u64, address as u64, len as u64).map(|_| ())
}

/// Raw connect entry. Pointer and length may intentionally be invalid in a test.
pub unsafe fn connect_raw(fd: i32, address: *const u8, len: u32) -> Result<(), Errno> {
    net::connect(fd as i64 as u64, address as u64, len as u64).map(|_| ())
}

/// Raw getsockname entry preserving Linux pointer/length copy ordering.
pub unsafe fn getsockname_raw(
    fd: i32,
    address: *mut u8,
    len: *mut socklen_t,
) -> Result<(), Errno> {
    net::getsockname(fd as i64 as u64, address as u64, len as u64).map(|_| ())
}

pub use anemone_abi::net::linux::{InAddr, SockAddrIn as Ipv4SocketAddress};
