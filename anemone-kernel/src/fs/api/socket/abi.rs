//! Byte-level Linux sockaddr handling and UDP outcome projection.

use core::mem::size_of;

use anemone_abi::net::{AF_INET, SockAddrIn, socklen_t};
use anemone_net_api::{
    Ipv4Address,
    udp::{UdpBindError, UdpLocalBinding, UdpQueryError},
};

use crate::{
    net::udp::BindError,
    prelude::*,
    syscall::user_access::{UserReadSlice, UserWriteSlice, user_addr},
};

const SOCKADDR_IN_LEN: usize = size_of::<SockAddrIn>();
const MAX_SOCKADDR_INPUT_LEN: usize = 128;

pub(super) fn read_sockaddr_in(addr: u64, len: u32) -> Result<(Ipv4Address, u16), SysError> {
    let len = len as usize;
    if !(SOCKADDR_IN_LEN..=MAX_SOCKADDR_INPUT_LEN).contains(&len) {
        return Err(SysError::InvalidArgument);
    }
    let addr = user_addr(addr)?;
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut bytes = [0u8; SOCKADDR_IN_LEN];
    UserReadSlice::<u8>::try_new(addr, SOCKADDR_IN_LEN, &mut uspace.lock())?
        .copy_to_slice(&mut bytes);
    let family = u16::from_ne_bytes(bytes[0..2].try_into().unwrap());
    if family != AF_INET as u16 {
        return Err(SysError::AddressFamilyNotSupported);
    }
    let port = u16::from_be_bytes(bytes[2..4].try_into().unwrap());
    let address = Ipv4Address::new(bytes[4..8].try_into().unwrap());
    if !address.is_unspecified() && !address.is_unicast() {
        return Err(SysError::InvalidArgument);
    }
    Ok((address, port))
}

pub(super) fn write_sockaddr_in(
    addr: u64,
    addrlen: u64,
    binding: Option<UdpLocalBinding>,
) -> Result<(), SysError> {
    let addrlen_addr = user_addr(addrlen)?;
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();

    let mut len_bytes = [0u8; size_of::<socklen_t>()];
    UserReadSlice::<u8>::try_new(addrlen_addr, len_bytes.len(), &mut uspace.lock())?
        .copy_to_slice(&mut len_bytes);
    let user_len = socklen_t::from_ne_bytes(len_bytes);
    if (user_len as i32) < 0 {
        return Err(SysError::InvalidArgument);
    }

    let (address, port) = binding
        .map(|binding| (binding.address(), binding.port()))
        .unwrap_or((Ipv4Address::UNSPECIFIED, 0));
    let mut bytes = [0u8; SOCKADDR_IN_LEN];
    bytes[0..2].copy_from_slice(&(AF_INET as u16).to_ne_bytes());
    bytes[2..4].copy_from_slice(&port.to_be_bytes());
    bytes[4..8].copy_from_slice(&address.octets());

    let copy_len = (user_len as usize).min(SOCKADDR_IN_LEN);
    if copy_len != 0 {
        let addr = user_addr(addr)?;
        UserWriteSlice::<u8>::try_new(addr, copy_len, &mut uspace.lock())?
            .copy_from_slice(&bytes[..copy_len]);
    }

    // Linux move_addr_to_user exposes any successful prefix copy before this
    // actual-length store. A fault here must not roll that copy back.
    let actual = (SOCKADDR_IN_LEN as socklen_t).to_ne_bytes();
    UserWriteSlice::<u8>::try_new(addrlen_addr, actual.len(), &mut uspace.lock())?
        .copy_from_slice(&actual);
    Ok(())
}

pub(super) fn map_bind_error(error: BindError) -> SysError {
    match error {
        BindError::AddressUnavailable => SysError::AddressNotAvailable,
        BindError::Stack(UdpBindError::UnknownEndpoint) => SysError::BadFileDescriptor,
        BindError::Stack(UdpBindError::AlreadyBound) => SysError::InvalidArgument,
        BindError::Stack(UdpBindError::PortInUse) => SysError::AddressInUse,
        BindError::Stack(UdpBindError::EphemeralPortsExhausted) => SysError::Again,
    }
}

pub(super) fn map_query_error(error: UdpQueryError) -> SysError {
    match error {
        UdpQueryError::UnknownEndpoint => SysError::BadFileDescriptor,
    }
}
