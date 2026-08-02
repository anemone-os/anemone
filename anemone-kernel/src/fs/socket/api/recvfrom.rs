use anemone_abi::syscall::SYS_RECVFROM;

use crate::{
    fs::{
        iomux::PollEvent,
        socket::{
            SocketAddress, SocketAddressSink, SocketReceiveError, SocketReceiveFlags,
            SocketReceiveRequest, SocketReceiveSink, SocketType, socket_from_file,
        },
    },
    prelude::*,
    syscall::user_access::user_addr,
    task::files::{Fd, FileStatusFlags},
};

use super::{
    abi::{
        map_query_error, map_receive_error, validate_receive_message_flags, write_payload,
        write_peer, write_socket_address,
    },
    wait_for_socket_file,
};

struct ReceiveSink {
    buf: u64,
    len: usize,
    peer: u64,
    addrlen: u64,
}

#[derive(Default)]
struct PeerCapture(Option<SocketAddress>);

impl SocketAddressSink for PeerCapture {
    fn copy_address(&mut self, address: Option<SocketAddress>) -> Result<(), SysError> {
        self.0 = address;
        Ok(())
    }
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
    let message_flags = validate_receive_message_flags(socket.socket_type(), flags)?;
    let nonblocking =
        message_flags.nonblocking || desc.file_flags().contains(FileStatusFlags::NONBLOCK);

    if socket.socket_type() == SocketType::UnixStream {
        let segment = if len == 0 {
            None
        } else {
            Some(UserBufferSegment::new(user_addr(buf)?, len))
        };
        let segments = segment.as_ref().map_or(&[][..], core::slice::from_ref);
        let uspace = task.clone_uspace_handle();
        let mut stream_sink = UserBufferSink::new(&uspace, segments);
        loop {
            match socket.receive(SocketReceiveRequest::Stream {
                sink: &mut stream_sink,
                flags: SocketReceiveFlags {
                    peek: message_flags.peek,
                },
            }) {
                Ok(copied) => {
                    if peer != 0 {
                        let mut peer_address = PeerCapture::default();
                        socket
                            .copy_peer_address(&mut peer_address)
                            .map_err(map_query_error)?;
                        write_socket_address(
                            SocketType::UnixStream,
                            peer,
                            addrlen,
                            peer_address.0,
                        )?;
                    }
                    return Ok(copied as u64);
                },
                Err(SocketReceiveError::WouldBlock) if !nonblocking => {},
                Err(error) => return Err(map_receive_error(error)),
            }
            wait_for_socket_file("sys_recvfrom", &task, desc.vfs_file(), PollEvent::READABLE)?;
        }
    }

    let mut sink = ReceiveSink {
        buf,
        len,
        peer,
        addrlen,
    };
    loop {
        match socket.receive(SocketReceiveRequest::Datagram(&mut sink)) {
            Ok(copied) => return Ok(copied as u64),
            Err(SocketReceiveError::WouldBlock) if !nonblocking => {},
            Err(error) => return Err(map_receive_error(error)),
        }

        // The family attempt released its operation guard on WouldBlock;
        // the shared wait owner schedules with only the source route alive.
        wait_for_socket_file("sys_recvfrom", &task, desc.vfs_file(), PollEvent::READABLE)?;
    }
}
