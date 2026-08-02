mod accept;
mod accept4;

use anemone_abi::net::linux::{SOCK_CLOEXEC, SOCK_NONBLOCK};

use crate::{
    fs::{
        iomux::PollEvent,
        socket::{SocketAcceptError, socket_file_desc_ops, socket_from_file},
    },
    prelude::*,
    task::files::{Fd, FdFlags, FileDesc, FileStatusFlags, LinuxOpenCompat, OpenAccessMode},
};

use super::{abi::write_socket_address, wait_for_socket_operation};

const ACCEPT_FLAGS: i32 = SOCK_NONBLOCK | SOCK_CLOEXEC;

fn accept_flags(flags: i32) -> Result<(FileStatusFlags, FdFlags), SysError> {
    if flags & !ACCEPT_FLAGS != 0 {
        return Err(SysError::InvalidArgument);
    }
    let mut status = FileStatusFlags::empty();
    status.set(FileStatusFlags::NONBLOCK, flags & SOCK_NONBLOCK != 0);
    let fd_flags = if flags & SOCK_CLOEXEC != 0 {
        FdFlags::CLOSE_ON_EXEC
    } else {
        FdFlags::empty()
    };
    Ok((status, fd_flags))
}

fn accept_with_flags(
    context: &'static str,
    fd: Fd,
    addr: u64,
    addrlen: u64,
    flags: i32,
) -> Result<u64, SysError> {
    let (accepted_status, accepted_fd_flags) = accept_flags(flags)?;
    let task = get_current_task();
    let desc = task.get_fd(fd)?;
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    let listener_nonblocking = desc.file_flags().contains(FileStatusFlags::NONBLOCK);
    let reservation = task.reserve_fd()?;

    let accepted = loop {
        match socket.accept() {
            Ok(accepted) => break accepted,
            Err(SocketAcceptError::WouldBlock(_)) if listener_nonblocking => {
                return Err(SysError::Again);
            },
            Err(SocketAcceptError::WouldBlock(wait)) => {
                wait_for_socket_operation(context, &task, &wait, PollEvent::READABLE)?;
            },
            Err(SocketAcceptError::Unsupported) => return Err(SysError::NotSupported),
            Err(SocketAcceptError::Retired) => return Err(SysError::BadFileDescriptor),
            Err(SocketAcceptError::InvalidState) => return Err(SysError::InvalidArgument),
        }
    };

    // Linux consumes the child before peer-address copyout. Any copy failure
    // drops `accepted`, which closes the child without requeue or fd publication.
    if addr != 0 {
        write_socket_address(socket.socket_type(), addr, addrlen, accepted.peer_address())?;
    }
    let file = accepted.prepare_file()?;
    file.check_status_flags(accepted_status.to_file_op_status_flags())
        .expect("validated accept flags must fit Socket FileOps");
    let file_desc = FileDesc::new_opened(
        file,
        OpenAccessMode::ReadWrite,
        accepted_status,
        LinuxOpenCompat::empty(),
        accepted_fd_flags,
        socket_file_desc_ops(),
    );
    Ok(reservation.commit(file_desc).raw() as u64)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn accept_flags_do_not_inherit_listener_status() {
        let (status, fd_flags) = accept_flags(0).unwrap();
        assert!(status.is_empty());
        assert!(fd_flags.is_empty());

        let (status, fd_flags) = accept_flags(SOCK_NONBLOCK | SOCK_CLOEXEC).unwrap();
        assert!(status.contains(FileStatusFlags::NONBLOCK));
        assert!(fd_flags.contains(FdFlags::CLOSE_ON_EXEC));
        assert!(matches!(
            accept_flags(SOCK_NONBLOCK | 0x4000_0000),
            Err(SysError::InvalidArgument)
        ));
    }
}
