use anemone_abi::syscall::SYS_RECVFROM;

use crate::{
    fs::socket::{
        SocketAddress, SocketAddressSink, SocketIoOps, SocketReceiveFlags, SocketReceiveRequest,
        SocketReceiveSink, SocketType, retry_socket_receive, socket_from_file,
    },
    prelude::*,
    syscall::user_access::user_addr,
    task::files::{Fd, FileStatusFlags},
};

use super::abi::{
    map_query_error, map_receive_error, validate_receive_message_flags, write_empty_socket_address,
    write_payload, write_socket_address,
};

struct ReceiveSink {
    socket_type: SocketType,
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
            write_socket_address(self.socket_type, self.peer, self.addrlen, Some(peer))?;
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

    let io = socket.io();
    if matches!(
        io,
        SocketIoOps::ByteStream { .. } | SocketIoOps::Seqpacket { .. }
    ) {
        let segment = if len == 0 {
            None
        } else {
            Some(UserBufferSegment::new(user_addr(buf)?, len))
        };
        let segments = segment.as_ref().map_or(&[][..], core::slice::from_ref);
        let uspace = task.clone_uspace_handle();
        let mut stream_sink = UserBufferSink::new(&uspace, segments);
        let outcome = retry_socket_receive(
            "sys_recvfrom",
            &task,
            desc.vfs_file(),
            nonblocking,
            || {
                socket.receive(match io {
                    SocketIoOps::ByteStream { .. } => SocketReceiveRequest::Stream {
                        sink: &mut stream_sink,
                        flags: SocketReceiveFlags {
                            peek: message_flags.peek,
                        },
                    },
                    SocketIoOps::Seqpacket { .. } => SocketReceiveRequest::Seqpacket {
                        sink: &mut stream_sink,
                        flags: SocketReceiveFlags {
                            peek: message_flags.peek,
                        },
                    },
                    SocketIoOps::Datagram { .. } => {
                        unreachable!("connection-oriented receive branch used datagram I/O")
                    },
                })
            },
            map_receive_error,
        )?;
        if peer != 0 {
            if socket.socket_type() == SocketType::Ipv4Tcp {
                write_empty_socket_address(peer, addrlen)?;
            } else {
                let mut peer_address = PeerCapture::default();
                socket
                    .copy_peer_address(&mut peer_address)
                    .map_err(map_query_error)?;
                write_socket_address(socket.socket_type(), peer, addrlen, peer_address.0)?;
            }
        }
        return Ok(if message_flags.truncate_result {
            outcome
                .packet_length()
                .expect("seqpacket truncation omitted record length") as u64
        } else {
            outcome.copied() as u64
        });
    }

    let mut sink = ReceiveSink {
        socket_type: socket.socket_type(),
        buf,
        len,
        peer,
        addrlen,
    };
    // Each family attempt releases its operation guard on WouldBlock; the
    // shared retry owner schedules with only the source route alive.
    retry_socket_receive(
        "sys_recvfrom",
        &task,
        desc.vfs_file(),
        nonblocking,
        || {
            socket.receive(SocketReceiveRequest::Datagram {
                sink: &mut sink,
                flags: SocketReceiveFlags {
                    peek: message_flags.peek,
                },
            })
        },
        map_receive_error,
    )
    .map(|outcome| {
        if message_flags.truncate_result {
            outcome
                .packet_length()
                .expect("datagram receive omitted its full packet length") as u64
        } else {
            outcome.copied() as u64
        }
    })
}
