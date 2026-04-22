//! Error type for the network subsystem.

use crate::prelude::*;

/// Network-subsystem errors.
///
/// Each variant maps to a semantically meaningful network condition. The
/// [`AsErrno`] impl translates them to POSIX errno values that are returned to
/// user space by the syscall layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetError {
    /// The address family (e.g. `AF_INET6`) is not supported.
    AddressFamilyNotSupported,
    /// The protocol (e.g. `IPPROTO_SCTP`) is not supported for this socket type.
    ProtocolNotSupported,
    /// The socket type (e.g. `SOCK_RAW`) is not supported.
    SocketTypeNotSupported,
    /// The operation is not supported on this socket (e.g. `listen` on UDP).
    OperationNotSupported,
    /// The local address is already in use.
    AddressInUse,
    /// No destination address was supplied for a connection-less send.
    DestinationAddressRequired,
    /// The message is too large to be sent atomically.
    MessageTooLong,
    /// The operation would block and non-blocking mode is active.
    WouldBlock,
    /// The socket is not connected.
    NotConnected,
    /// The connection attempt was refused by the remote end.
    ConnectionRefused,
    /// The connection was closed unexpectedly (write on a half-closed TCP socket).
    BrokenPipe,
    /// No usable network stack is attached (interface down or not yet initialised).
    NetworkDown,
    /// Invalid argument (e.g. malformed `sockaddr`, zero port on bind).
    InvalidArgument,
}

impl AsErrno for NetError {
    fn as_errno(&self) -> Errno {
        match self {
            NetError::AddressFamilyNotSupported => EAFNOSUPPORT,
            NetError::ProtocolNotSupported => EPROTONOSUPPORT,
            NetError::SocketTypeNotSupported => ESOCKTNOSUPPORT,
            NetError::OperationNotSupported => EOPNOTSUPP,
            NetError::AddressInUse => EADDRINUSE,
            NetError::DestinationAddressRequired => EDESTADDRREQ,
            NetError::MessageTooLong => EMSGSIZE,
            NetError::WouldBlock => EAGAIN,
            NetError::NotConnected => ENOTCONN,
            NetError::ConnectionRefused => ECONNREFUSED,
            NetError::BrokenPipe => EPIPE,
            NetError::NetworkDown => ENETDOWN,
            NetError::InvalidArgument => EINVAL,
        }
    }
}

impl From<NetError> for SysError {
    fn from(e: NetError) -> SysError {
        match e {
            NetError::AddressFamilyNotSupported  => SysError::NotSupported,
            NetError::ProtocolNotSupported       => SysError::NotSupported,
            NetError::SocketTypeNotSupported     => SysError::NotSupported,
            NetError::OperationNotSupported      => SysError::NotSupported,
            NetError::AddressInUse               => SysError::AlreadyExists,
            NetError::DestinationAddressRequired => SysError::InvalidArgument,
            NetError::MessageTooLong             => SysError::InvalidArgument,
            NetError::WouldBlock                 => SysError::Again,
            NetError::NotConnected               => SysError::InvalidArgument,
            NetError::ConnectionRefused          => SysError::IO,
            NetError::BrokenPipe                 => SysError::BrokenPipe,
            NetError::NetworkDown                => SysError::IO,
            NetError::InvalidArgument            => SysError::InvalidArgument,
        }
    }
}
