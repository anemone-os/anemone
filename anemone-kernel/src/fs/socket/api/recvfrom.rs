use anemone_abi::syscall::SYS_RECVFROM;

use crate::{
    fs::{
        iomux::PollEvent,
        socket::{SocketAddress, SocketReceiveError, SocketReceiveSink, socket_from_file},
    },
    prelude::*,
    task::files::{Fd, FileStatusFlags},
};

use super::{
    abi::{map_receive_error, validate_message_flags, write_payload, write_peer},
    wait_for_socket_file,
};

struct ReceiveSink {
    buf: u64,
    len: usize,
    peer: u64,
    addrlen: u64,
}

impl SocketReceiveSink for ReceiveSink {
    fn copy_datagram(&mut self, payload: &[u8], peer: SocketAddress) -> Result<usize, SysError> {
        let copied = write_payload(self.buf, payload, self.len)?;
        if self.peer != 0 {
            write_peer(self.peer, self.addrlen, peer)?;
        }
        Ok(copied)
    }
}

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
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    let per_call_nonblocking = validate_message_flags(flags)?;
    let nonblocking = per_call_nonblocking || desc.file_flags().contains(FileStatusFlags::NONBLOCK);

    let mut sink = ReceiveSink {
        buf,
        len,
        peer,
        addrlen,
    };
    loop {
        match socket.receive(&mut sink) {
            Ok(copied) => return Ok(copied as u64),
            Err(SocketReceiveError::WouldBlock) if !nonblocking => {},
            Err(error) => return Err(map_receive_error(error)),
        }

        // The family attempt released its operation guard on WouldBlock;
        // the shared wait owner schedules with only the source route alive.
        wait_for_socket_file("sys_recvfrom", &task, desc.vfs_file(), PollEvent::READABLE)?;
    }
}
