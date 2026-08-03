//! Byte-level Linux sockaddr handling and Socket outcome projection.

use alloc::{vec, vec::Vec};
use core::mem::size_of;

use anemone_abi::net::linux::{
    AF_INET, AF_UNIX, MSG_DONTWAIT, MSG_NOSIGNAL, MSG_PEEK, SockAddrIn, SockAddrUn, socklen_t,
};
use anemone_net_api::Ipv4Address;

use crate::{
    fs::socket::{
        SocketAddress, SocketBindError, SocketQueryError, SocketReceiveError, SocketSendError,
        SocketType,
    },
    kconfig_defs::NET_UDP_MAX_PAYLOAD_BYTES,
    prelude::*,
    syscall::user_access::{UserReadSlice, UserWriteSlice, user_addr},
};

const SOCKADDR_IN_LEN: usize = size_of::<SockAddrIn>();
const SOCKADDR_UN_LEN: usize = size_of::<SockAddrUn>();
const SOCKADDR_UN_PATH_OFFSET: usize = 2;
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

fn parse_sockaddr_un(bytes: &[u8]) -> Result<SocketAddress, SysError> {
    let len = bytes.len();
    // The accepted target excludes Linux's family-only autobind and abstract
    // namespace. A filesystem pathname therefore needs at least one byte and
    // must fit the Linux sockaddr_un input object.
    if !(SOCKADDR_UN_PATH_OFFSET + 1..=SOCKADDR_UN_LEN).contains(&len) {
        return Err(SysError::InvalidArgument);
    }
    let family = u16::from_ne_bytes(bytes[0..2].try_into().unwrap());
    if family != AF_UNIX as u16 {
        return Err(SysError::InvalidArgument);
    }

    let raw_path = &bytes[SOCKADDR_UN_PATH_OFFSET..len];
    if raw_path[0] == 0 {
        knoticeln!("unix bind: abstract/autobind address is outside the R0 target");
        return Err(SysError::NotSupported);
    }
    let path_len = raw_path
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(raw_path.len());
    let pathname =
        core::str::from_utf8(&raw_path[..path_len]).map_err(|_| SysError::InvalidPath)?;
    Ok(SocketAddress::UnixPathname(Arc::from(pathname)))
}

fn read_sockaddr_un(addr: u64, len: u32) -> Result<SocketAddress, SysError> {
    let len = len as usize;
    if len > SOCKADDR_UN_LEN {
        return Err(SysError::InvalidArgument);
    }
    let addr = user_addr(addr)?;
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut bytes = [0u8; SOCKADDR_UN_LEN];
    UserReadSlice::<u8>::try_new(addr, len, &mut uspace.lock())?
        .copy_to_slice(&mut bytes[..len])?;
    parse_sockaddr_un(&bytes[..len])
}

pub(super) fn read_socket_address(
    socket_type: SocketType,
    addr: u64,
    len: u32,
) -> Result<SocketAddress, SysError> {
    match socket_type {
        SocketType::Ipv4Udp | SocketType::Ipv4IcmpRaw => read_sockaddr_in(addr, len),
        SocketType::UnixStream => read_sockaddr_un(addr, len),
    }
}

pub(super) fn validate_raw_socket_address(addr: u64, len: u32) -> Result<(), SysError> {
    let len = len as usize;
    if len > MAX_SOCKADDR_INPUT_LEN {
        return Err(SysError::InvalidArgument);
    }
    if len == 0 {
        return Ok(());
    }
    let addr = user_addr(addr)?;
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut bytes = vec![0u8; len];
    UserReadSlice::<u8>::try_new(addr, len, &mut uspace.lock())?.copy_to_slice(&mut bytes)
}

fn write_sockaddr_bytes(addr: u64, addrlen: u64, bytes: &[u8]) -> Result<(), SysError> {
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

    let copy_len = (user_len as usize).min(bytes.len());
    if copy_len != 0 {
        let addr = user_addr(addr)?;
        UserWriteSlice::<u8>::try_new(addr, copy_len, &mut uspace.lock())?
            .copy_from_slice(&bytes[..copy_len])?;
    }

    // Linux move_addr_to_user exposes any successful prefix copy before this
    // actual-length store. A fault here must not roll that copy back.
    let actual = (bytes.len() as socklen_t).to_ne_bytes();
    UserWriteSlice::<u8>::try_new(addrlen_addr, actual.len(), &mut uspace.lock())?
        .copy_from_slice(&actual)?;
    Ok(())
}

fn socket_address_bytes(socket_type: SocketType, address: Option<SocketAddress>) -> Vec<u8> {
    match (socket_type, address) {
        (SocketType::Ipv4Udp | SocketType::Ipv4IcmpRaw, None) => {
            let mut bytes = vec![0u8; SOCKADDR_IN_LEN];
            bytes[0..2].copy_from_slice(&(AF_INET as u16).to_ne_bytes());
            bytes
        },
        (
            SocketType::Ipv4Udp | SocketType::Ipv4IcmpRaw,
            Some(SocketAddress::Ipv4 { address, port }),
        ) => {
            let mut bytes = vec![0u8; SOCKADDR_IN_LEN];
            bytes[0..2].copy_from_slice(&(AF_INET as u16).to_ne_bytes());
            bytes[2..4].copy_from_slice(&port.to_be_bytes());
            bytes[4..8].copy_from_slice(&address.octets());
            bytes
        },
        (SocketType::UnixStream, None) => (AF_UNIX as u16).to_ne_bytes().to_vec(),
        (SocketType::UnixStream, Some(SocketAddress::UnixPathname(pathname))) => {
            let mut bytes = Vec::with_capacity(SOCKADDR_UN_PATH_OFFSET + pathname.len() + 1);
            bytes.extend_from_slice(&(AF_UNIX as u16).to_ne_bytes());
            bytes.extend_from_slice(pathname.as_bytes());
            bytes.push(0);
            bytes
        },
        (_, Some(SocketAddress::Unspecified)) => {
            panic!("Socket family returned AF_UNSPEC from an address query")
        },
        _ => panic!("Socket family returned an address of another semantic type"),
    }
}

pub(super) fn write_socket_address(
    socket_type: SocketType,
    addr: u64,
    addrlen: u64,
    address: Option<SocketAddress>,
) -> Result<(), SysError> {
    let bytes = socket_address_bytes(socket_type, address);
    write_sockaddr_bytes(addr, addrlen, &bytes)
}

pub(super) fn write_peer(addr: u64, addrlen: u64, peer: SocketAddress) -> Result<(), SysError> {
    let SocketAddress::Ipv4 { address, port } = peer else {
        unreachable!("datagram receive returned a non-IPv4 peer")
    };
    let mut bytes = [0u8; SOCKADDR_IN_LEN];
    bytes[0..2].copy_from_slice(&(AF_INET as u16).to_ne_bytes());
    bytes[2..4].copy_from_slice(&port.to_be_bytes());
    bytes[4..8].copy_from_slice(&address.octets());
    write_sockaddr_bytes(addr, addrlen, &bytes)
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

pub(super) struct SendMessageFlags {
    pub(super) nonblocking: bool,
    pub(super) no_signal: bool,
}

pub(super) fn validate_send_message_flags(
    socket_type: SocketType,
    flags: i32,
) -> Result<SendMessageFlags, SysError> {
    let supported = match socket_type {
        SocketType::Ipv4Udp => MSG_DONTWAIT,
        SocketType::UnixStream => MSG_DONTWAIT | MSG_NOSIGNAL,
        // Checkpoint 2A descriptors are not fd-reachable. Checkpoint 2B removes
        // this publication guard when its Linux flag matrix activates.
        SocketType::Ipv4IcmpRaw => 0,
    };
    if flags & !supported != 0 {
        knoticeln!("socket: unsupported sendto flags {:#x}", flags);
        return Err(SysError::NotSupported);
    }
    Ok(SendMessageFlags {
        nonblocking: flags & MSG_DONTWAIT != 0,
        no_signal: flags & MSG_NOSIGNAL != 0,
    })
}

pub(super) struct ReceiveMessageFlags {
    pub(super) nonblocking: bool,
    pub(super) peek: bool,
}

pub(super) fn validate_receive_message_flags(
    socket_type: SocketType,
    flags: i32,
) -> Result<ReceiveMessageFlags, SysError> {
    let supported = match socket_type {
        SocketType::Ipv4Udp => MSG_DONTWAIT,
        SocketType::UnixStream => MSG_DONTWAIT | MSG_PEEK,
        // See the send-side Checkpoint 2A publication guard above.
        SocketType::Ipv4IcmpRaw => 0,
    };
    if flags & !supported != 0 {
        knoticeln!("socket: unsupported recvfrom flags {:#x}", flags);
        return Err(SysError::NotSupported);
    }
    Ok(ReceiveMessageFlags {
        nonblocking: flags & MSG_DONTWAIT != 0,
        peek: flags & MSG_PEEK != 0,
    })
}

pub(super) fn map_bind_error(error: SocketBindError) -> SysError {
    match error {
        SocketBindError::Unsupported => SysError::NotSupported,
        SocketBindError::Retired => SysError::BadFileDescriptor,
        SocketBindError::AlreadyBound => SysError::InvalidArgument,
        SocketBindError::AddressInUse => SysError::AddressInUse,
        SocketBindError::AddressUnavailable => SysError::AddressNotAvailable,
        SocketBindError::ResourceExhausted => SysError::Again,
        SocketBindError::Operation(error) => error,
    }
}

pub(super) fn map_query_error(error: SocketQueryError) -> SysError {
    match error {
        SocketQueryError::Unsupported => SysError::NotSupported,
        SocketQueryError::Retired => SysError::BadFileDescriptor,
        SocketQueryError::NotConnected => SysError::NotConnected,
        SocketQueryError::Copy(error) => error,
    }
}

pub(super) fn map_send_error(error: SocketSendError) -> SysError {
    match error {
        SocketSendError::Unsupported => SysError::NotSupported,
        SocketSendError::Retired => SysError::BadFileDescriptor,
        SocketSendError::NotConnected => SysError::NotConnected,
        SocketSendError::AlreadyConnected => SysError::AlreadyConnected,
        SocketSendError::InvalidState => SysError::InvalidArgument,
        SocketSendError::AddressInUse => SysError::AddressInUse,
        SocketSendError::AddressUnavailable => SysError::AddressNotAvailable,
        SocketSendError::ResourceExhausted | SocketSendError::WouldBlock => SysError::Again,
        SocketSendError::NetworkUnreachable => SysError::NetworkUnreachable,
        SocketSendError::DestinationRequired => SysError::DestinationAddressRequired,
        SocketSendError::InvalidDestination => SysError::InvalidArgument,
        SocketSendError::MessageTooLong => SysError::MessageTooLong,
        SocketSendError::PeerClosed => SysError::BrokenPipe,
        SocketSendError::Copy(error) => error,
    }
}

pub(super) fn map_receive_error(error: SocketReceiveError) -> SysError {
    match error {
        SocketReceiveError::Unsupported => SysError::NotSupported,
        SocketReceiveError::Retired => SysError::BadFileDescriptor,
        SocketReceiveError::InvalidState => SysError::NotConnected,
        SocketReceiveError::WouldBlock => SysError::Again,
        SocketReceiveError::Copy(error) => error,
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    fn unix_bytes(path: &[u8]) -> Vec<u8> {
        let mut bytes = (AF_UNIX as u16).to_ne_bytes().to_vec();
        bytes.extend_from_slice(path);
        bytes
    }

    #[kunit]
    fn unix_input_checks_length_family_abstract_and_nul_boundary() {
        assert_eq!(
            parse_sockaddr_un(&(AF_UNIX as u16).to_ne_bytes()),
            Err(SysError::InvalidArgument)
        );

        let mut wrong_family = unix_bytes(b"path");
        wrong_family[..2].copy_from_slice(&(AF_INET as u16).to_ne_bytes());
        assert_eq!(
            parse_sockaddr_un(&wrong_family),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(
            parse_sockaddr_un(&unix_bytes(b"\0abstract")),
            Err(SysError::NotSupported)
        );
        assert_eq!(
            parse_sockaddr_un(&unix_bytes(b"first\0ignored")),
            Ok(SocketAddress::UnixPathname(Arc::from("first")))
        );

        let full = unix_bytes(&[b'p'; anemone_abi::net::linux::UNIX_PATH_MAX]);
        let SocketAddress::UnixPathname(path) = parse_sockaddr_un(&full).unwrap() else {
            panic!("Unix parser returned another address family")
        };
        assert_eq!(path.len(), anemone_abi::net::linux::UNIX_PATH_MAX);
    }

    #[kunit]
    fn unix_output_preserves_unnamed_and_linux_terminator_lengths() {
        assert_eq!(
            socket_address_bytes(SocketType::UnixStream, None),
            (AF_UNIX as u16).to_ne_bytes()
        );
        let bytes = socket_address_bytes(
            SocketType::UnixStream,
            Some(SocketAddress::UnixPathname(Arc::from("path"))),
        );
        assert_eq!(&bytes[..2], &(AF_UNIX as u16).to_ne_bytes());
        assert_eq!(&bytes[2..], b"path\0");

        let full: Arc<str> = Arc::from("p".repeat(anemone_abi::net::linux::UNIX_PATH_MAX));
        assert_eq!(
            socket_address_bytes(
                SocketType::UnixStream,
                Some(SocketAddress::UnixPathname(full))
            )
            .len(),
            SOCKADDR_UN_LEN + 1,
        );
    }
}
