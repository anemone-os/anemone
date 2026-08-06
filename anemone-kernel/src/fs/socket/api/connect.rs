use anemone_abi::syscall::SYS_CONNECT;

use crate::{
    fs::{
        iomux::PollEvent,
        socket::{SocketConnectError, socket_from_file, wait_for_socket_operation},
    },
    prelude::*,
    task::files::{Fd, FileStatusFlags},
};

use super::abi::read_socket_connect_address;

#[syscall(SYS_CONNECT)]
fn sys_connect(fd: Fd, addr: u64, addrlen: u32) -> Result<u64, SysError> {
    let task = get_current_task();
    let desc = task.get_fd(fd)?;
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    let address = read_socket_connect_address(socket.socket_type(), addr, addrlen)?;
    let nonblocking = desc.file_flags().contains(FileStatusFlags::NONBLOCK);
    connect_socket(&task, socket, address, nonblocking)?;
    Ok(0)
}

fn connect_socket(
    task: &Arc<Task>,
    socket: &crate::fs::socket::front::Socket,
    address: crate::fs::socket::SocketAddress,
    nonblocking: bool,
) -> Result<(), SysError> {
    // This marker belongs only to this syscall invocation. The Stack remains
    // the sole connection-phase owner; it records only that this blocking call
    // actually waited for the current handshake, whether another call started
    // it or this one did. A fresh call on an already-connected socket still
    // returns EISCONN.
    let mut waited_for_handshake = false;

    loop {
        match socket.connect(address.clone()) {
            Ok(()) => return Ok(()),
            Err(SocketConnectError::Started(_)) if nonblocking => {
                return Err(SysError::InProgress);
            },
            Err(SocketConnectError::Started(wait)) => {
                waited_for_handshake = true;
                wait_for_socket_operation("sys_connect", task, &wait, PollEvent::WRITABLE)?;
            },
            Err(SocketConnectError::InProgress(_)) if nonblocking => {
                return Err(SysError::AlreadyInProgress);
            },
            Err(SocketConnectError::InProgress(wait)) => {
                waited_for_handshake = true;
                wait_for_socket_operation("sys_connect", task, &wait, PollEvent::WRITABLE)?;
            },
            Err(SocketConnectError::WouldBlock(_)) if nonblocking => {
                return Err(SysError::Again);
            },
            Err(SocketConnectError::WouldBlock(wait)) => {
                // The capability can only subscribe/recheck listener capacity
                // and client lifetime. The next attempt repeats pathname lookup
                // and DAC instead of reusing admission authority across sleep.
                wait_for_socket_operation("sys_connect", task, &wait, PollEvent::WRITABLE)?;
            },
            Err(SocketConnectError::Unsupported) => return Err(SysError::NotSupported),
            Err(SocketConnectError::Retired) => return Err(SysError::BadFileDescriptor),
            Err(SocketConnectError::InvalidState) => return Err(SysError::InvalidArgument),
            Err(SocketConnectError::AlreadyConnected) => {
                return if waited_for_handshake {
                    Ok(())
                } else {
                    Err(SysError::AlreadyConnected)
                };
            },
            Err(SocketConnectError::ConnectionRefused) => {
                return Err(SysError::ConnectionRefused);
            },
            Err(SocketConnectError::ConnectionReset) => return Err(SysError::ConnectionReset),
            Err(SocketConnectError::ConnectionAborted) => return Err(SysError::ConnectionAborted),
            Err(SocketConnectError::ConnectionTimedOut) => return Err(SysError::Timeout),
            Err(SocketConnectError::ProtocolTypeMismatch) => {
                return Err(SysError::ProtocolTypeMismatch);
            },
            Err(SocketConnectError::Operation(error)) => return Err(error),
        }
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    use core::sync::atomic::{AtomicU16, Ordering};

    use anemone_net_api::Ipv4Address;

    use crate::{
        fs::socket::{
            SocketAddress, TCP_SOCKET_OPS, prepare_socket, socket_file_desc_ops, socket_from_file,
        },
        task::files::{OpenAccessMode, OpenedFileFinalReleaseCtx},
    };

    static NEXT_PORT: AtomicU16 = AtomicU16::new(49_000);

    fn release(file: &File) {
        (socket_file_desc_ops().final_release.unwrap())(OpenedFileFinalReleaseCtx {
            file,
            access: OpenAccessMode::ReadWrite,
            notification_suppressed: true,
        });
    }

    #[kunit]
    fn blocking_retry_waits_for_an_existing_handshake() {
        let address = SocketAddress::Ipv4 {
            address: Ipv4Address::LOOPBACK,
            port: NEXT_PORT.fetch_add(1, Ordering::Relaxed),
        };
        let (listener_file, listener_creation) = prepare_socket(&TCP_SOCKET_OPS).unwrap();
        listener_creation.commit();
        let listener = socket_from_file(&listener_file).unwrap();
        listener.bind(address.clone()).unwrap();
        listener.listen(1).unwrap();

        let (client_file, client_creation) = prepare_socket(&TCP_SOCKET_OPS).unwrap();
        client_creation.commit();
        let client = socket_from_file(&client_file).unwrap();
        assert!(matches!(
            client.connect(address.clone()),
            Err(SocketConnectError::Started(_))
        ));

        // Enter through the real blocking syscall driver after another call
        // has already started the handshake. Success must belong to this wait
        // round rather than being mistaken for a fresh EISCONN call.
        assert_eq!(
            connect_socket(&get_current_task(), client, address.clone(), false),
            Ok(())
        );
        assert_eq!(
            connect_socket(&get_current_task(), client, address, false),
            Err(SysError::AlreadyConnected)
        );

        release(&client_file);
        release(&listener_file);
    }
}
