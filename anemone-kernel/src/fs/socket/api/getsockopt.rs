use core::mem::size_of;

use anemone_abi::{
    net::linux::{
        ICMP_FILTER, IP_TOS, IP_TTL, IPPROTO_IP, SO_ACCEPTCONN, SO_DOMAIN, SO_ERROR, SO_PROTOCOL,
        SO_TYPE, SOL_RAW, SOL_SOCKET,
    },
    syscall::SYS_GETSOCKOPT,
};

use super::profile::socket_abi_profile;

use crate::{
    fs::socket::{
        SocketOptionError, SocketOptionQuery, SocketOptionValue, SocketPendingError,
        SocketQueryError, front::Socket, socket_from_file,
    },
    prelude::*,
    syscall::user_access::{UserReadSlice, UserWriteSlice, user_addr},
    task::files::Fd,
};

// These constants stay private until the TCP creation tuple is published.
const SO_REUSEADDR: i32 = 2;
const IPPROTO_TCP: i32 = 6;
const TCP_NODELAY: i32 = 1;

fn query_value(socket: &Socket, option: i32) -> Result<i32, SysError> {
    let profile = socket_abi_profile(socket.socket_type());
    match option {
        SO_TYPE => Ok(profile.socket_kind()),
        SO_DOMAIN => Ok(profile.domain()),
        SO_PROTOCOL => Ok(profile.protocol()),
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

enum GetOption {
    Descriptor(i32),
    Ipv4Scalar(i32),
    Bytes([u8; size_of::<u32>()]),
}

impl GetOption {
    fn bytes(&self) -> [u8; size_of::<u32>()] {
        match self {
            Self::Descriptor(value) | Self::Ipv4Scalar(value) => value.to_ne_bytes(),
            Self::Bytes(bytes) => *bytes,
        }
    }

    fn copied_len(&self, requested: usize) -> usize {
        match self {
            Self::Ipv4Scalar(value)
                if (1..size_of::<i32>()).contains(&requested)
                    && (0..=u8::MAX as i32).contains(value) =>
            {
                1
            },
            _ => requested.min(size_of::<u32>()),
        }
    }

    fn writes_len_first(&self) -> bool {
        !matches!(self, Self::Descriptor(_))
    }
}

fn map_option_error(error: SocketOptionError) -> SysError {
    match error {
        SocketOptionError::Unsupported => SysError::ProtocolOptionNotSupported,
        SocketOptionError::Retired => SysError::BadFileDescriptor,
        SocketOptionError::InvalidValue => SysError::InvalidArgument,
    }
}

fn query_option(socket: &Socket, level: i32, option: i32) -> Result<GetOption, SysError> {
    match (level, option) {
        (SOL_SOCKET, SO_REUSEADDR) => socket
            .query_option(SocketOptionQuery::ReuseAddress)
            .map_err(map_option_error)
            .and_then(|value| match value {
                SocketOptionValue::Boolean(value) => Ok(GetOption::Descriptor(i32::from(value))),
                _ => Err(SysError::ProtocolOptionNotSupported),
            }),
        (SOL_SOCKET, SO_ERROR) => socket
            .query_option(SocketOptionQuery::PendingError)
            .map_err(map_option_error)
            .and_then(|value| match value {
                SocketOptionValue::PendingError(error) => {
                    Ok(GetOption::Descriptor(error.map_or(0, pending_error_errno)))
                },
                _ => Err(SysError::ProtocolOptionNotSupported),
            }),
        (SOL_SOCKET, option) => query_value(socket, option).map(GetOption::Descriptor),
        (IPPROTO_TCP, TCP_NODELAY) => socket
            .query_option(SocketOptionQuery::TcpNoDelay)
            .map_err(map_option_error)
            .and_then(|value| match value {
                SocketOptionValue::Boolean(value) => Ok(GetOption::Descriptor(i32::from(value))),
                _ => Err(SysError::ProtocolOptionNotSupported),
            }),
        (IPPROTO_IP, IP_TTL) => socket
            .query_option(SocketOptionQuery::Ipv4TimeToLive)
            .map_err(map_option_error)
            .and_then(|value| match value {
                SocketOptionValue::Ipv4TimeToLive(value) => Ok(GetOption::Ipv4Scalar(value as i32)),
                _ => Err(SysError::ProtocolOptionNotSupported),
            }),
        (IPPROTO_IP, IP_TOS) => socket
            .query_option(SocketOptionQuery::Ipv4TypeOfService)
            .map_err(map_option_error)
            .and_then(|value| match value {
                SocketOptionValue::Ipv4TypeOfService(value) => {
                    Ok(GetOption::Ipv4Scalar(value as i32))
                },
                _ => Err(SysError::ProtocolOptionNotSupported),
            }),
        (SOL_RAW, ICMP_FILTER) => socket
            .query_option(SocketOptionQuery::IcmpTypeFilter)
            .map_err(map_option_error)
            .and_then(|value| match value {
                SocketOptionValue::IcmpTypeFilter(value) => {
                    Ok(GetOption::Bytes(value.to_ne_bytes()))
                },
                _ => Err(SysError::ProtocolOptionNotSupported),
            }),
        _ => Err(SysError::ProtocolOptionNotSupported),
    }
}

const fn pending_error_errno(error: SocketPendingError) -> i32 {
    match error {
        SocketPendingError::ConnectionRefused => SysError::ConnectionRefused.as_errno(),
        SocketPendingError::ConnectionReset => SysError::ConnectionReset.as_errno(),
        SocketPendingError::TimedOut => SysError::Timeout.as_errno(),
    }
}

trait GetOptionOutput {
    fn write_length(&mut self, bytes: &[u8]) -> Result<(), SysError>;
    fn write_value(&mut self, bytes: &[u8]) -> Result<(), SysError>;
}

struct UserGetOptionOutput {
    value: u64,
    len_address: VirtAddr,
    uspace: Arc<UserSpaceHandle>,
}

impl GetOptionOutput for UserGetOptionOutput {
    fn write_length(&mut self, bytes: &[u8]) -> Result<(), SysError> {
        UserWriteSlice::<u8>::try_new(self.len_address, bytes.len(), &mut self.uspace.lock())?
            .copy_from_slice(bytes)
    }

    fn write_value(&mut self, bytes: &[u8]) -> Result<(), SysError> {
        let value = user_addr(self.value)?;
        UserWriteSlice::<u8>::try_new(value, bytes.len(), &mut self.uspace.lock())?
            .copy_from_slice(bytes)
    }
}

fn query_option_into(
    socket: &Socket,
    level: i32,
    option: i32,
    requested: usize,
    output: &mut dyn GetOptionOutput,
) -> Result<(), SysError> {
    // SO_ERROR consumption happens here, before either user copy. A later
    // fault must not recreate the owner error in a Socket-side cache.
    let result = query_option(socket, level, option)?;
    let bytes = result.bytes();
    let copied = result.copied_len(requested);
    let actual = (copied as i32).to_ne_bytes();
    if result.writes_len_first() {
        output.write_length(&actual)?;
    }
    if copied != 0 {
        output.write_value(&bytes[..copied])?;
    }
    if !result.writes_len_first() {
        output.write_length(&actual)?;
    }
    Ok(())
}

fn get_socket_option(
    fd: Fd,
    level: i32,
    option: i32,
    value: u64,
    len: u64,
) -> Result<u64, SysError> {
    let desc = get_current_task().get_fd(fd)?;
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    let len_address = user_addr(len)?;
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut len_bytes = [0u8; size_of::<i32>()];
    {
        UserReadSlice::<u8>::try_new(len_address, len_bytes.len(), &mut uspace.lock())?
            .copy_to_slice(&mut len_bytes)?;
    }
    let requested = i32::from_ne_bytes(len_bytes);
    if requested < 0 {
        return Err(SysError::InvalidArgument);
    }
    query_option_into(
        socket,
        level,
        option,
        requested as usize,
        &mut UserGetOptionOutput {
            value,
            len_address,
            uspace,
        },
    )?;
    Ok(0)
}

#[syscall(SYS_GETSOCKOPT)]
fn sys_getsockopt(fd: Fd, level: i32, option: i32, value: u64, len: u64) -> Result<u64, SysError> {
    get_socket_option(fd, level, option, value, len)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;
    use crate::{
        fs::socket::{
            ICMP_RAW_SOCKET_OPS, SocketAddress, SocketConnectError, TCP_SOCKET_OPS, UDP_SOCKET_OPS,
            UNIX_STREAM_SOCKET_OPS, prepare_socket, prepare_socket_pair, socket_file_desc_ops,
            socket_from_file,
        },
        task::files::{OpenAccessMode, OpenedFileFinalReleaseCtx},
    };
    use anemone_abi::net::linux::{
        AF_INET, AF_UNIX, IPPROTO_ICMP, IPPROTO_UDP, SOCK_DGRAM, SOCK_RAW, SOCK_STREAM,
    };
    use anemone_net_api::Ipv4Address;

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

        let (raw_file, raw_creation) = prepare_socket(&ICMP_RAW_SOCKET_OPS).unwrap();
        let raw = socket_from_file(&raw_file).unwrap();
        assert_eq!(query_value(raw, SO_TYPE), Ok(SOCK_RAW));
        assert_eq!(query_value(raw, SO_DOMAIN), Ok(AF_INET));
        assert_eq!(query_value(raw, SO_PROTOCOL), Ok(IPPROTO_ICMP));
        assert_eq!(query_value(raw, SO_ACCEPTCONN), Ok(0));
        drop(raw_creation);
    }

    #[kunit]
    fn tcp_so_error_copy_fault_consumes_the_owner_error_once() {
        let (file, creation) = prepare_socket(&TCP_SOCKET_OPS).unwrap();
        creation.commit();
        assert!(matches!(
            socket_from_file(&file)
                .unwrap()
                .connect(SocketAddress::Ipv4 {
                    address: Ipv4Address::LOOPBACK,
                    port: 1,
                }),
            Err(SocketConnectError::WouldBlock(_))
        ));

        for _ in 0..20_000 {
            yield_now();
        }

        struct FaultValue {
            length_writes: usize,
            value_writes: usize,
        }

        impl GetOptionOutput for FaultValue {
            fn write_length(&mut self, _bytes: &[u8]) -> Result<(), SysError> {
                self.length_writes += 1;
                Ok(())
            }

            fn write_value(&mut self, _bytes: &[u8]) -> Result<(), SysError> {
                self.value_writes += 1;
                Err(SysError::BadAddress)
            }
        }

        let socket = socket_from_file(&file).unwrap();
        let mut output = FaultValue {
            length_writes: 0,
            value_writes: 0,
        };
        assert_eq!(
            query_option_into(socket, SOL_SOCKET, SO_ERROR, size_of::<i32>(), &mut output),
            Err(SysError::BadAddress)
        );
        assert_eq!(output.value_writes, 1);
        assert_eq!(output.length_writes, 0);
        assert_eq!(
            socket.query_option(SocketOptionQuery::PendingError),
            Ok(SocketOptionValue::PendingError(None))
        );

        (socket_file_desc_ops().final_release.unwrap())(OpenedFileFinalReleaseCtx {
            file: &file,
            access: OpenAccessMode::ReadWrite,
            notification_suppressed: true,
        });
    }
}
