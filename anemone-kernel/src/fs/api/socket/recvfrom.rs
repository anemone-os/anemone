use anemone_abi::syscall::SYS_RECVFROM;

use crate::{
    fs::socket::{receive_udp_socket, udp_socket_from_file},
    prelude::*,
    task::files::{Fd, FileStatusFlags},
};

use super::abi::{map_receive_error, validate_message_flags, write_payload, write_peer};

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

    // The operation guard remains held through copyout, but the Stack lock was
    // released by receive_udp_socket after atomically detaching this datagram.
    let (_operation, datagram) =
        receive_udp_socket(socket).map_err(|error| map_receive_error(error, nonblocking))?;
    let copied = write_payload(buf, datagram.payload(), len)?;
    if peer != 0 {
        write_peer(peer, addrlen, datagram.peer())?;
    }
    Ok(copied as u64)
}
