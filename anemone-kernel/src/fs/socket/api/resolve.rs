//! Linux socket tuple normalization shared by creation syscalls.

use anemone_abi::net::linux::{
    AF_INET, AF_UNIX, IPPROTO_UDP, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK, SOCK_STREAM,
};

use crate::{
    fs::socket::{SocketOps, UDP_SOCKET_OPS, UNIX_STREAM_SOCKET_OPS},
    prelude::*,
    task::files::{FdFlags, FileStatusFlags},
};

const SOCK_TYPE_MASK: i32 = 0xf;
const SUPPORTED_FLAGS: i32 = SOCK_NONBLOCK | SOCK_CLOEXEC;

pub(super) struct ResolvedSocket {
    pub(super) ops: &'static SocketOps,
    pub(super) status_flags: FileStatusFlags,
    pub(super) fd_flags: FdFlags,
}

pub(super) fn resolve_socket(
    family: i32,
    socket_type: i32,
    protocol: i32,
) -> Result<ResolvedSocket, SysError> {
    if socket_type & !(SOCK_TYPE_MASK | SUPPORTED_FLAGS) != 0 {
        return Err(SysError::InvalidArgument);
    }

    let ops = match (family, socket_type & SOCK_TYPE_MASK) {
        (AF_INET, SOCK_DGRAM) if protocol == 0 || protocol == IPPROTO_UDP => &UDP_SOCKET_OPS,
        (AF_INET, SOCK_DGRAM) => return Err(SysError::ProtocolNotSupported),
        (AF_INET, _) => return Err(SysError::SocketTypeNotSupported),
        (AF_UNIX, SOCK_STREAM) if protocol == 0 => &UNIX_STREAM_SOCKET_OPS,
        (AF_UNIX, SOCK_STREAM) => return Err(SysError::ProtocolNotSupported),
        (AF_UNIX, _) => return Err(SysError::SocketTypeNotSupported),
        _ => return Err(SysError::AddressFamilyNotSupported),
    };

    let mut status_flags = FileStatusFlags::empty();
    status_flags.set(FileStatusFlags::NONBLOCK, socket_type & SOCK_NONBLOCK != 0);
    let fd_flags = if socket_type & SOCK_CLOEXEC != 0 {
        FdFlags::CLOSE_ON_EXEC
    } else {
        FdFlags::empty()
    };
    Ok(ResolvedSocket {
        ops,
        status_flags,
        fd_flags,
    })
}
