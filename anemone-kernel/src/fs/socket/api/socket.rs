use anemone_abi::{
    net::linux::{AF_INET, IPPROTO_UDP, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK},
    syscall::SYS_SOCKET,
};

use crate::{
    fs::socket::{SocketOps, UDP_SOCKET_OPS, prepare_socket, socket_file_desc_ops},
    prelude::*,
    task::files::{FdFlags, FileDesc, FileStatusFlags, LinuxOpenCompat, OpenAccessMode},
};

const SOCK_TYPE_MASK: i32 = 0xf;
const SUPPORTED_FLAGS: i32 = SOCK_NONBLOCK | SOCK_CLOEXEC;

struct ResolvedSocket {
    ops: &'static SocketOps,
    status_flags: FileStatusFlags,
    fd_flags: FdFlags,
}

fn resolve_socket(
    family: i32,
    socket_type: i32,
    protocol: i32,
) -> Result<ResolvedSocket, SysError> {
    if family != AF_INET {
        return Err(SysError::AddressFamilyNotSupported);
    }
    if socket_type & !(SOCK_TYPE_MASK | SUPPORTED_FLAGS) != 0 {
        return Err(SysError::InvalidArgument);
    }
    if socket_type & SOCK_TYPE_MASK != SOCK_DGRAM {
        return Err(SysError::SocketTypeNotSupported);
    }
    if protocol != 0 && protocol != IPPROTO_UDP {
        return Err(SysError::ProtocolNotSupported);
    }

    let mut status_flags = FileStatusFlags::empty();
    status_flags.set(FileStatusFlags::NONBLOCK, socket_type & SOCK_NONBLOCK != 0);
    let fd_flags = if socket_type & SOCK_CLOEXEC != 0 {
        FdFlags::CLOSE_ON_EXEC
    } else {
        FdFlags::empty()
    };
    Ok(ResolvedSocket {
        ops: &UDP_SOCKET_OPS,
        status_flags,
        fd_flags,
    })
}

#[syscall(SYS_SOCKET)]
fn sys_socket(family: i32, socket_type: i32, protocol: i32) -> Result<u64, SysError> {
    let resolved = resolve_socket(family, socket_type, protocol)?;

    let task = get_current_task();
    let reservation = task.reserve_fd()?;
    let (file, creation) = prepare_socket(resolved.ops)?;

    file.check_status_flags(resolved.status_flags.to_file_op_status_flags())?;
    let file_desc = FileDesc::new_opened(
        file,
        OpenAccessMode::ReadWrite,
        resolved.status_flags,
        LinuxOpenCompat::empty(),
        resolved.fd_flags,
        socket_file_desc_ops(),
    );
    let fd = reservation.commit(file_desc);
    creation.commit();
    Ok(fd.raw() as u64)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn resolver_normalizes_udp_protocol_and_creation_flags() {
        let default = resolve_socket(AF_INET, SOCK_DGRAM, 0).unwrap();
        let explicit = resolve_socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP).unwrap();
        assert!(core::ptr::eq(default.ops, explicit.ops));
        assert!(default.status_flags.is_empty());
        assert!(default.fd_flags.is_empty());

        let flagged = resolve_socket(
            AF_INET,
            SOCK_DGRAM | SOCK_NONBLOCK | SOCK_CLOEXEC,
            IPPROTO_UDP,
        )
        .unwrap();
        assert!(flagged.status_flags.contains(FileStatusFlags::NONBLOCK));
        assert!(flagged.fd_flags.contains(FdFlags::CLOSE_ON_EXEC));
    }

    #[kunit]
    fn resolver_rejects_unsupported_capabilities_before_creation() {
        assert!(matches!(
            resolve_socket(AF_INET + 1, SOCK_DGRAM, 0),
            Err(SysError::AddressFamilyNotSupported)
        ));
        assert!(matches!(
            resolve_socket(AF_INET, SOCK_DGRAM | 0x4000_0000, 0),
            Err(SysError::InvalidArgument)
        ));
        assert!(matches!(
            resolve_socket(AF_INET, 1, 0),
            Err(SysError::SocketTypeNotSupported)
        ));
        assert!(matches!(
            resolve_socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP + 1),
            Err(SysError::ProtocolNotSupported)
        ));
    }
}
