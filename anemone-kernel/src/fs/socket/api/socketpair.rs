use core::mem::size_of;

#[cfg(feature = "kunit")]
use anemone_abi::net::linux::{
    AF_UNIX, IPPROTO_UDP, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK, SOCK_STREAM,
};
use anemone_abi::{net::linux::AF_INET, syscall::SYS_SOCKETPAIR};

use crate::{
    fs::socket::{prepare_socket_pair, socket_file_desc_ops},
    prelude::*,
    syscall::user_access::{UserWritePtr, user_addr},
    task::files::{FileDesc, LinuxOpenCompat, OpenAccessMode},
};

use super::resolve::{ResolvedSocket, resolve_socket};

fn resolve_socket_pair(
    family: i32,
    socket_type: i32,
    protocol: i32,
) -> Result<ResolvedSocket, SysError> {
    resolve_socket(family, socket_type, protocol).map_err(|error| {
        // Linux reports EOPNOTSUPP for an AF_INET socketpair type that is not
        // available, while socket(2) keeps ESOCKTNOSUPPORT. Tuple/type
        // normalization remains shared; this is only syscall errno projection.
        if family == AF_INET && error == SysError::SocketTypeNotSupported {
            SysError::NotSupported
        } else {
            error
        }
    })
}

fn copy_fd_pair(addr: u64, first: i32, second: i32) -> Result<(), SysError> {
    let first_addr = user_addr(addr)?;
    let second_addr = user_addr(
        addr.checked_add(size_of::<i32>() as u64)
            .ok_or(SysError::BadAddress)?,
    )?;
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut guard = uspace.lock();

    // Linux exposes the first fd number when the second store faults. Both
    // reservations still roll back because neither slot is published yet.
    UserWritePtr::<i32>::try_new(first_addr, &mut guard)?.write(first)?;
    UserWritePtr::<i32>::try_new(second_addr, &mut guard)?.write(second)?;
    Ok(())
}

#[syscall(SYS_SOCKETPAIR)]
fn sys_socketpair(
    family: i32,
    socket_type: i32,
    protocol: i32,
    pair: u64,
) -> Result<u64, SysError> {
    let resolved = resolve_socket_pair(family, socket_type, protocol)?;
    let task = get_current_task();
    let first_reservation = task.reserve_fd()?;
    let second_reservation = task.reserve_fd()?;

    copy_fd_pair(
        pair,
        first_reservation.fd().raw() as i32,
        second_reservation.fd().raw() as i32,
    )?;

    let (first_file, second_file) = prepare_socket_pair(resolved.ops)?;
    first_file.check_status_flags(resolved.status_flags.to_file_op_status_flags())?;
    second_file.check_status_flags(resolved.status_flags.to_file_op_status_flags())?;

    let first_desc = FileDesc::new_opened(
        first_file,
        OpenAccessMode::ReadWrite,
        resolved.status_flags,
        LinuxOpenCompat::empty(),
        resolved.fd_flags,
        socket_file_desc_ops(),
    );
    let second_desc = FileDesc::new_opened(
        second_file,
        OpenAccessMode::ReadWrite,
        resolved.status_flags,
        LinuxOpenCompat::empty(),
        resolved.fd_flags,
        socket_file_desc_ops(),
    );

    // Both descriptions and the paired state are fully prepared. Reservation
    // commit is infallible, so no returnable failure exists after the first
    // slot becomes visible.
    first_reservation.commit(first_desc);
    second_reservation.commit(second_desc);
    Ok(0)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{
        fs::socket::{UDP_SOCKET_OPS, UNIX_STREAM_SOCKET_OPS},
        task::files::{FdFlags, FileStatusFlags},
    };

    #[kunit]
    fn resolver_accepts_unix_stream_flags_and_rejects_udp_pair_capability() {
        let unix =
            resolve_socket_pair(AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0).unwrap();
        assert!(core::ptr::eq(unix.ops, &UNIX_STREAM_SOCKET_OPS));
        assert!(unix.status_flags.contains(FileStatusFlags::NONBLOCK));
        assert!(unix.fd_flags.contains(FdFlags::CLOSE_ON_EXEC));

        let udp = resolve_socket_pair(AF_INET, SOCK_DGRAM, IPPROTO_UDP).unwrap();
        assert!(core::ptr::eq(udp.ops, &UDP_SOCKET_OPS));
        assert!(matches!(
            prepare_socket_pair(udp.ops),
            Err(SysError::NotSupported)
        ));
    }
}
