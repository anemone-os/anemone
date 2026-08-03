use core::mem::size_of;

use anemone_abi::{
    net::linux::{
        AF_INET, AF_UNIX, IPPROTO_UDP, SO_ACCEPTCONN, SO_DOMAIN, SO_PROTOCOL, SO_TYPE, SOCK_DGRAM,
        SOCK_STREAM, SOL_SOCKET,
    },
    syscall::SYS_GETSOCKOPT,
};

use crate::{
    fs::socket::{SocketQueryError, SocketType, front::Socket, socket_from_file},
    prelude::*,
    syscall::user_access::{UserReadSlice, UserWriteSlice, user_addr},
    task::files::Fd,
};

fn query_value(socket: &Socket, option: i32) -> Result<i32, SysError> {
    match option {
        SO_TYPE => Ok(match socket.socket_type() {
            SocketType::Ipv4Udp => SOCK_DGRAM,
            SocketType::UnixStream => SOCK_STREAM,
            SocketType::Ipv4IcmpRaw => {
                unreachable!("ICMP raw Socket descriptor is not fd-reachable before Checkpoint 2B")
            },
        }),
        SO_DOMAIN => Ok(match socket.socket_type() {
            SocketType::Ipv4Udp => AF_INET,
            SocketType::UnixStream => AF_UNIX,
            SocketType::Ipv4IcmpRaw => {
                unreachable!("ICMP raw Socket descriptor is not fd-reachable before Checkpoint 2B")
            },
        }),
        SO_PROTOCOL => Ok(match socket.socket_type() {
            SocketType::Ipv4Udp => IPPROTO_UDP,
            SocketType::UnixStream => 0,
            SocketType::Ipv4IcmpRaw => {
                unreachable!("ICMP raw Socket descriptor is not fd-reachable before Checkpoint 2B")
            },
        }),
        SO_ACCEPTCONN => socket
            .is_accepting()
            .map(i32::from)
            .map_err(|error| match error {
                SocketQueryError::Retired => SysError::BadFileDescriptor,
                SocketQueryError::Unsupported
                | SocketQueryError::NotConnected
                | SocketQueryError::Copy(_) => {
                    unreachable!("Socket role query returned an invalid family outcome")
                },
            }),
        _ => Err(SysError::ProtocolOptionNotSupported),
    }
}

#[syscall(SYS_GETSOCKOPT)]
fn sys_getsockopt(fd: Fd, level: i32, option: i32, value: u64, len: u64) -> Result<u64, SysError> {
    let desc = get_current_task().get_fd(fd)?;
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    let len_address = user_addr(len)?;
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut len_bytes = [0u8; size_of::<i32>()];
    UserReadSlice::<u8>::try_new(len_address, len_bytes.len(), &mut uspace.lock())?
        .copy_to_slice(&mut len_bytes)?;
    let requested = i32::from_ne_bytes(len_bytes);
    if requested < 0 {
        return Err(SysError::InvalidArgument);
    }
    if level != SOL_SOCKET {
        return Err(SysError::ProtocolOptionNotSupported);
    }
    let result = query_value(socket, option)?.to_ne_bytes();
    let copied = (requested as usize).min(result.len());
    if copied != 0 {
        let value = user_addr(value)?;
        UserWriteSlice::<u8>::try_new(value, copied, &mut uspace.lock())?
            .copy_from_slice(&result[..copied])?;
    }
    let actual = (copied as i32).to_ne_bytes();
    UserWriteSlice::<u8>::try_new(len_address, actual.len(), &mut uspace.lock())?
        .copy_from_slice(&actual)?;
    Ok(0)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::fs::socket::{
        UDP_SOCKET_OPS, UNIX_STREAM_SOCKET_OPS, prepare_socket, prepare_socket_pair,
        socket_from_file,
    };

    #[kunit]
    fn descriptor_and_role_queries_are_family_neutral() {
        let (udp_file, creation) = prepare_socket(&UDP_SOCKET_OPS).unwrap();
        let udp = socket_from_file(&udp_file).unwrap();
        assert_eq!(query_value(udp, SO_TYPE), Ok(SOCK_DGRAM));
        assert_eq!(query_value(udp, SO_DOMAIN), Ok(AF_INET));
        assert_eq!(query_value(udp, SO_PROTOCOL), Ok(IPPROTO_UDP));
        assert_eq!(query_value(udp, SO_ACCEPTCONN), Ok(0));
        assert_eq!(
            query_value(udp, anemone_abi::net::linux::SO_ERROR),
            Err(SysError::ProtocolOptionNotSupported)
        );
        drop(creation);

        let (unix_file, _) = prepare_socket_pair(&UNIX_STREAM_SOCKET_OPS).unwrap();
        let unix = socket_from_file(&unix_file).unwrap();
        assert_eq!(query_value(unix, SO_TYPE), Ok(SOCK_STREAM));
        assert_eq!(query_value(unix, SO_DOMAIN), Ok(AF_UNIX));
        assert_eq!(query_value(unix, SO_PROTOCOL), Ok(0));
        assert_eq!(query_value(unix, SO_ACCEPTCONN), Ok(0));
    }
}
