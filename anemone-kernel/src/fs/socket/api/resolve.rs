//! Linux socket tuple normalization shared by creation syscalls.

use anemone_abi::net::linux::{SOCK_CLOEXEC, SOCK_NONBLOCK};

use crate::{
    fs::socket::SocketOps,
    prelude::*,
    task::{
        credentials::cap::Capability,
        files::{FdFlags, FileStatusFlags},
    },
};

use super::profile::resolve_socket_profile;

const SOCK_TYPE_MASK: i32 = 0xf;
const SUPPORTED_FLAGS: i32 = SOCK_NONBLOCK | SOCK_CLOEXEC;

pub(super) struct ResolvedSocket {
    pub(super) ops: &'static SocketOps,
    pub(super) status_flags: FileStatusFlags,
    pub(super) fd_flags: FdFlags,
    pub(super) required_capability: Option<Capability>,
}

pub(super) fn resolve_socket(
    family: i32,
    socket_type: i32,
    protocol: i32,
) -> Result<ResolvedSocket, SysError> {
    if socket_type & !(SOCK_TYPE_MASK | SUPPORTED_FLAGS) != 0 {
        return Err(SysError::InvalidArgument);
    }

    let profile = resolve_socket_profile(family, socket_type & SOCK_TYPE_MASK, protocol)?;

    let mut status_flags = FileStatusFlags::empty();
    status_flags.set(FileStatusFlags::NONBLOCK, socket_type & SOCK_NONBLOCK != 0);
    let fd_flags = if socket_type & SOCK_CLOEXEC != 0 {
        FdFlags::CLOSE_ON_EXEC
    } else {
        FdFlags::empty()
    };
    Ok(ResolvedSocket {
        ops: profile.ops(),
        status_flags,
        fd_flags,
        required_capability: profile.required_capability(),
    })
}
