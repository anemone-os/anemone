use anemone_abi::syscall::SYS_RECVFROM;

use crate::{
    fs::{
        iomux::PollEvent,
        socket::{receive_udp_socket, udp_socket_from_file},
    },
    prelude::*,
    task::files::{Fd, FileStatusFlags},
};

use super::{
    abi::{map_receive_error, validate_message_flags, write_payload, write_peer},
    wait_for_udp_file,
};

#[syscall(SYS_RECVFROM)]
fn sys_recvfrom(
    fd: Fd,
    buf: u64,
    len: usize,
    flags: i32,
    peer: u64,
    addrlen: u64,
) -> Result<u64, SysError> {
    let task = get_current_task();
    let desc = task.get_fd(fd)?;
    let socket = udp_socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    let per_call_nonblocking = validate_message_flags(flags)?;
    let nonblocking = per_call_nonblocking || desc.file_flags().contains(FileStatusFlags::NONBLOCK);

    let (_operation, datagram) = loop {
        match receive_udp_socket(socket) {
            Ok(received) => break received,
            Err(anemone_net_api::udp::UdpReceiveError::WouldBlock) if !nonblocking => {},
            Err(error) => return Err(map_receive_error(error)),
        }

        // receive_udp_socket released the File operation guard on WouldBlock;
        // the shared wait owner schedules with only the source route alive.
        wait_for_udp_file("sys_recvfrom", &task, desc.vfs_file(), PollEvent::READABLE)?;
    };
    // The successful operation guard remains held through copyout, but the
    // Stack lock was released after atomically detaching this datagram.
    let copied = write_payload(buf, datagram.payload(), len)?;
    if peer != 0 {
        write_peer(peer, addrlen, datagram.peer())?;
    }
    Ok(copied as u64)
}
