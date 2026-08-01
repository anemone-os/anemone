//! Byte-level Linux sockaddr handling and UDP outcome projection.

use alloc::{vec, vec::Vec};
use core::mem::size_of;

use anemone_abi::net::linux::{AF_INET, MSG_DONTWAIT, SockAddrIn, socklen_t};
use anemone_net_api::Ipv4Address;

use crate::{
    fs::socket::{
        SocketAddress, SocketBindError, SocketQueryError, SocketReceiveError, SocketSendError,
    },
    kconfig_defs::NET_UDP_MAX_PAYLOAD_BYTES,
    prelude::*,
    syscall::user_access::{UserReadSlice, UserWriteSlice, user_addr},
};

const SOCKADDR_IN_LEN: usize = size_of::<SockAddrIn>();
const MAX_SOCKADDR_INPUT_LEN: usize = 128;

pub(super) fn read_sockaddr_in(addr: u64, len: u32) -> Result<SocketAddress, SysError> {
    let len = len as usize;
    if !(SOCKADDR_IN_LEN..=MAX_SOCKADDR_INPUT_LEN).contains(&len) {
        return Err(SysError::InvalidArgument);
    }
    let addr = user_addr(addr)?;
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut bytes = [0u8; SOCKADDR_IN_LEN];
    UserReadSlice::<u8>::try_new(addr, SOCKADDR_IN_LEN, &mut uspace.lock())?
        .copy_to_slice(&mut bytes)?;
    let family = u16::from_ne_bytes(bytes[0..2].try_into().unwrap());
    if family != AF_INET as u16 {
        return Err(SysError::AddressFamilyNotSupported);
    }
    let port = u16::from_be_bytes(bytes[2..4].try_into().unwrap());
    let address = Ipv4Address::new(bytes[4..8].try_into().unwrap());
    if !address.is_unspecified() && !address.is_unicast() {
        return Err(SysError::InvalidArgument);
    }
    Ok(SocketAddress::Ipv4 { address, port })
}

fn write_sockaddr_value(
    addr: u64,
    addrlen: u64,
    address: Ipv4Address,
    port: u16,
) -> Result<(), SysError> {
    let addrlen_addr = user_addr(addrlen)?;
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();

    let mut len_bytes = [0u8; size_of::<socklen_t>()];
    UserReadSlice::<u8>::try_new(addrlen_addr, len_bytes.len(), &mut uspace.lock())?
        .copy_to_slice(&mut len_bytes)?;
    let user_len = socklen_t::from_ne_bytes(len_bytes);
    if (user_len as i32) < 0 {
        return Err(SysError::InvalidArgument);
    }

    let mut bytes = [0u8; SOCKADDR_IN_LEN];
    bytes[0..2].copy_from_slice(&(AF_INET as u16).to_ne_bytes());
    bytes[2..4].copy_from_slice(&port.to_be_bytes());
    bytes[4..8].copy_from_slice(&address.octets());

    let copy_len = (user_len as usize).min(SOCKADDR_IN_LEN);
    if copy_len != 0 {
        let addr = user_addr(addr)?;
        UserWriteSlice::<u8>::try_new(addr, copy_len, &mut uspace.lock())?
            .copy_from_slice(&bytes[..copy_len])?;
    }

    // Linux move_addr_to_user exposes any successful prefix copy before this
    // actual-length store. A fault here must not roll that copy back.
    let actual = (SOCKADDR_IN_LEN as socklen_t).to_ne_bytes();
    UserWriteSlice::<u8>::try_new(addrlen_addr, actual.len(), &mut uspace.lock())?
        .copy_from_slice(&actual)?;
    Ok(())
}

pub(super) fn write_sockaddr_in(
    addr: u64,
    addrlen: u64,
    address: Option<SocketAddress>,
) -> Result<(), SysError> {
    let (address, port) = match address {
        None => (Ipv4Address::UNSPECIFIED, 0),
        Some(SocketAddress::Ipv4 { address, port }) => (address, port),
    };
    write_sockaddr_value(addr, addrlen, address, port)
}

pub(super) fn write_peer(addr: u64, addrlen: u64, peer: SocketAddress) -> Result<(), SysError> {
    let SocketAddress::Ipv4 { address, port } = peer;
    write_sockaddr_value(addr, addrlen, address, port)
}

pub(super) fn read_payload(addr: u64, len: usize) -> Result<Vec<u8>, SysError> {
    // sendto commits any implicit binding before reaching this allocation
    // guard. Stack admission still rechecks the same configured Endpoint limit
    // together with the selected interface MTU before reporting success.
    if len > NET_UDP_MAX_PAYLOAD_BYTES {
        return Err(SysError::MessageTooLong);
    }
    if len == 0 {
        return Ok(Vec::new());
    }
    let addr = user_addr(addr)?;
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut payload = vec![0; len];
    UserReadSlice::<u8>::try_new(addr, len, &mut uspace.lock())?.copy_to_slice(&mut payload)?;
    Ok(payload)
}

pub(super) fn write_payload(addr: u64, payload: &[u8], len: usize) -> Result<usize, SysError> {
    let copied = payload.len().min(len);
    if copied == 0 {
        return Ok(0);
    }
    let addr = user_addr(addr)?;
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    UserWriteSlice::<u8>::try_new(addr, copied, &mut uspace.lock())?
        .copy_from_slice(&payload[..copied])?;
    Ok(copied)
}

pub(super) fn validate_message_flags(flags: i32) -> Result<bool, SysError> {
    if flags & !MSG_DONTWAIT != 0 {
        knoticeln!("socket: unsupported sendto/recvfrom flags {:#x}", flags);
        return Err(SysError::NotSupported);
    }
    Ok(flags & MSG_DONTWAIT != 0)
}

pub(super) fn map_bind_error(error: SocketBindError) -> SysError {
    match error {
        SocketBindError::Unsupported => SysError::NotSupported,
        SocketBindError::Retired => SysError::BadFileDescriptor,
        SocketBindError::AlreadyBound => SysError::InvalidArgument,
        SocketBindError::AddressInUse => SysError::AddressInUse,
        SocketBindError::AddressUnavailable => SysError::AddressNotAvailable,
        SocketBindError::ResourceExhausted => SysError::Again,
    }
}

pub(super) fn map_query_error(error: SocketQueryError) -> SysError {
    match error {
        SocketQueryError::Unsupported => SysError::NotSupported,
        SocketQueryError::Retired => SysError::BadFileDescriptor,
        SocketQueryError::Copy(error) => error,
    }
}

pub(super) fn map_send_error(error: SocketSendError) -> SysError {
    match error {
        SocketSendError::Unsupported => SysError::NotSupported,
        SocketSendError::Retired => SysError::BadFileDescriptor,
        SocketSendError::InvalidState => SysError::InvalidArgument,
        SocketSendError::AddressInUse => SysError::AddressInUse,
        SocketSendError::AddressUnavailable => SysError::AddressNotAvailable,
        SocketSendError::ResourceExhausted | SocketSendError::WouldBlock => SysError::Again,
        SocketSendError::NetworkUnreachable => SysError::NetworkUnreachable,
        SocketSendError::InvalidDestination => SysError::InvalidArgument,
        SocketSendError::MessageTooLong => SysError::MessageTooLong,
        SocketSendError::Copy(error) => error,
    }
}

pub(super) fn map_receive_error(error: SocketReceiveError) -> SysError {
    match error {
        SocketReceiveError::Unsupported => SysError::NotSupported,
        SocketReceiveError::Retired => SysError::BadFileDescriptor,
        SocketReceiveError::WouldBlock => SysError::Again,
        SocketReceiveError::Copy(error) => error,
    }
}
