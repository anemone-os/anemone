//! Typed socket wrappers plus narrow raw Linux ABI conformance entries.

use anemone_abi::{
    errno::{EINVAL, Errno},
    net::linux::{
        AF_INET, AF_UNIX, IPPROTO_UDP, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK, SOCK_STREAM,
        SockAddrIn, SockAddrUn, UNIX_PATH_MAX, socklen_t,
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

/// Raw getsockname entry preserving Linux pointer/length copy ordering.
pub unsafe fn getsockname_raw(
    fd: i32,
    address: *mut u8,
    len: *mut socklen_t,
) -> Result<(), Errno> {
    net::getsockname(fd as i64 as u64, address as u64, len as u64).map(|_| ())
}

/// Raw six-argument sendto ABI. The kernel registers it in Stage 3C.
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

/// Raw six-argument recvfrom ABI. The kernel registers it in Stage 3C.
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

pub use anemone_abi::net::linux::{InAddr, SockAddrIn as Ipv4SocketAddress};
