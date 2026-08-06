#[cfg(feature = "kunit")]
use anemone_abi::net::linux::{
    AF_INET, AF_UNIX, IPPROTO_ICMP, IPPROTO_UDP, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK, SOCK_RAW,
    SOCK_SEQPACKET, SOCK_STREAM,
};
use anemone_abi::syscall::SYS_SOCKET;

use crate::{
    fs::socket::{prepare_socket, socket_file_desc_ops},
    prelude::*,
    task::files::{FileDesc, LinuxOpenCompat, OpenAccessMode},
};

use super::resolve::resolve_socket;

#[syscall(SYS_SOCKET)]
fn sys_socket(family: i32, socket_type: i32, protocol: i32) -> Result<u64, SysError> {
    let resolved = resolve_socket(family, socket_type, protocol)?;

    let task = get_current_task();
    if resolved
        .required_capability
        .is_some_and(|capability| !task.has_cap(capability))
    {
        return Err(SysError::PermissionDenied);
    }
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
    use crate::{
        fs::socket::UNIX_STREAM_SOCKET_OPS,
        task::files::{FdFlags, FileStatusFlags},
    };

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
            resolve_socket(AF_INET, SOCK_SEQPACKET, 0),
            Err(SysError::SocketTypeNotSupported)
        ));
        assert!(matches!(
            resolve_socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP + 1),
            Err(SysError::ProtocolNotSupported)
        ));

        let unix = resolve_socket(AF_UNIX, SOCK_STREAM, 0).unwrap();
        assert!(core::ptr::eq(unix.ops, &UNIX_STREAM_SOCKET_OPS));
        let (file, creation) = prepare_socket(unix.ops).unwrap();
        assert_eq!(
            crate::fs::socket::socket_from_file(&file)
                .unwrap()
                .socket_type(),
            crate::fs::socket::SocketType::UnixStream
        );
        creation.commit();
    }

    #[kunit]
    fn resolver_publishes_only_the_privileged_icmp_raw_tuple() {
        let raw = resolve_socket(AF_INET, SOCK_RAW, IPPROTO_ICMP).unwrap();
        assert!(core::ptr::eq(
            raw.ops,
            &crate::fs::socket::ICMP_RAW_SOCKET_OPS
        ));
        assert_eq!(
            raw.required_capability,
            Some(crate::task::credentials::cap::Capability::NET_RAW)
        );
        assert!(matches!(
            resolve_socket(AF_INET, SOCK_RAW, IPPROTO_ICMP + 1),
            Err(SysError::ProtocolNotSupported)
        ));
    }
}
