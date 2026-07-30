use anemone_abi::{
    net::linux::{AF_INET, IPPROTO_UDP, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK},
    syscall::SYS_SOCKET,
};

use crate::{
    fs::socket::{prepare_udp_socket, udp_file_desc_ops},
    prelude::*,
    task::files::{FdFlags, FileDesc, FileStatusFlags, LinuxOpenCompat, OpenAccessMode},
};

const SOCK_TYPE_MASK: i32 = 0xf;
const SUPPORTED_FLAGS: i32 = SOCK_NONBLOCK | SOCK_CLOEXEC;

#[syscall(SYS_SOCKET)]
fn sys_socket(family: i32, socket_type: i32, protocol: i32) -> Result<u64, SysError> {
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

    let task = get_current_task();
    let reservation = task.reserve_fd()?;
    let (file, creation) = prepare_udp_socket()?;

    let mut status_flags = FileStatusFlags::empty();
    status_flags.set(FileStatusFlags::NONBLOCK, socket_type & SOCK_NONBLOCK != 0);
    file.check_status_flags(status_flags.to_file_op_status_flags())?;
    let fd_flags = if socket_type & SOCK_CLOEXEC != 0 {
        FdFlags::CLOSE_ON_EXEC
    } else {
        FdFlags::empty()
    };
    let file_desc = FileDesc::new_opened(
        file,
        OpenAccessMode::ReadWrite,
        status_flags,
        LinuxOpenCompat::empty(),
        fd_flags,
        udp_file_desc_ops(),
    );
    let fd = reservation.commit(file_desc);
    creation.commit();
    Ok(fd.raw() as u64)
}
