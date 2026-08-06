use core::mem::size_of;

use anemone_abi::{
    net::linux::{
        ICMP_FILTER, IP_TOS, IP_TTL, IPPROTO_IP, IPPROTO_TCP, SO_REUSEADDR, SOL_RAW, SOL_SOCKET,
        TCP_NODELAY,
    },
    syscall::SYS_SETSOCKOPT,
};

use crate::{
    fs::socket::{
        SocketOptionError, SocketOptionMutation, SocketOptionQuery, SocketOptionValue, SocketType,
        front::Socket, socket_from_file,
    },
    kconfig_defs::NET_ICMP_RAW_DEFAULT_TTL,
    prelude::*,
    syscall::user_access::{UserReadSlice, user_addr},
    task::files::Fd,
};

fn map_option_error(error: SocketOptionError) -> SysError {
    match error {
        SocketOptionError::Unsupported => SysError::ProtocolOptionNotSupported,
        SocketOptionError::Retired => SysError::BadFileDescriptor,
        SocketOptionError::InvalidValue => SysError::InvalidArgument,
    }
}

trait OptionInput {
    fn read(&mut self, bytes: &mut [u8]) -> Result<(), SysError>;
}

struct UserOptionInput {
    value: u64,
}

impl OptionInput for UserOptionInput {
    fn read(&mut self, bytes: &mut [u8]) -> Result<(), SysError> {
        if bytes.is_empty() {
            return Ok(());
        }
        let address = user_addr(self.value)?;
        let task = get_current_task();
        let uspace = task.clone_uspace_handle();
        UserReadSlice::<u8>::try_new(address, bytes.len(), &mut uspace.lock())?.copy_to_slice(bytes)
    }
}

fn read_scalar(input: &mut dyn OptionInput, len: usize) -> Result<i32, SysError> {
    let copied = len.min(size_of::<i32>());
    let mut bytes = [0u8; size_of::<i32>()];
    input.read(&mut bytes[..copied])?;
    Ok(if copied == size_of::<i32>() {
        i32::from_ne_bytes(bytes)
    } else {
        bytes[0] as i32
    })
}

fn normalize_option_len(len: i32) -> Result<usize, SysError> {
    usize::try_from(len).map_err(|_| SysError::InvalidArgument)
}

fn mutate_socket_option(
    socket: &Socket,
    level: i32,
    option: i32,
    len: usize,
    input: &mut dyn OptionInput,
) -> Result<(), SysError> {
    let mutation = match (socket.socket_type(), level, option) {
        (SocketType::Ipv4Tcp, SOL_SOCKET, SO_REUSEADDR) => {
            if len < size_of::<i32>() {
                return Err(SysError::InvalidArgument);
            }
            SocketOptionMutation::ReuseAddress(read_scalar(input, size_of::<i32>())? != 0)
        },
        (SocketType::Ipv4Tcp, IPPROTO_TCP, TCP_NODELAY) => {
            if len < size_of::<i32>() {
                return Err(SysError::InvalidArgument);
            }
            SocketOptionMutation::TcpNoDelay(read_scalar(input, size_of::<i32>())? != 0)
        },
        (SocketType::Ipv4IcmpRaw, IPPROTO_IP, IP_TTL) => match read_scalar(input, len)? {
            -1 => SocketOptionMutation::Ipv4TimeToLive(NET_ICMP_RAW_DEFAULT_TTL),
            ttl @ 1..=255 => SocketOptionMutation::Ipv4TimeToLive(ttl as u8),
            _ => return Err(SysError::InvalidArgument),
        },
        (SocketType::Ipv4IcmpRaw, IPPROTO_IP, IP_TOS) => {
            SocketOptionMutation::Ipv4TypeOfService(read_scalar(input, len)? as u8)
        },
        (SocketType::Ipv4IcmpRaw, SOL_RAW, ICMP_FILTER) => {
            let SocketOptionValue::IcmpTypeFilter(current) = socket
                .query_option(SocketOptionQuery::IcmpTypeFilter)
                .map_err(map_option_error)?
            else {
                return Err(SysError::ProtocolOptionNotSupported);
            };
            let copied = len.min(size_of::<u32>());
            let mut bytes = current.to_ne_bytes();
            input.read(&mut bytes[..copied])?;
            SocketOptionMutation::IcmpTypeFilter(u32::from_ne_bytes(bytes))
        },
        _ => return Err(SysError::ProtocolOptionNotSupported),
    };
    socket.mutate_option(mutation).map_err(map_option_error)
}

fn set_socket_option(
    fd: i32,
    level: i32,
    option: i32,
    value: u64,
    len: i32,
) -> Result<u64, SysError> {
    let len = normalize_option_len(len)?;
    // Linux rejects a signed-negative optlen before looking up the fd. Keep
    // fd conversion here, after that ABI check, instead of in syscall parsing.
    let fd = Fd::new(fd as u32).ok_or(SysError::BadFileDescriptor)?;
    let desc = get_current_task().get_fd(fd)?;
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    mutate_socket_option(socket, level, option, len, &mut UserOptionInput { value })?;
    Ok(0)
}

#[syscall(SYS_SETSOCKOPT)]
fn sys_setsockopt(fd: i32, level: i32, option: i32, value: u64, len: i32) -> Result<u64, SysError> {
    set_socket_option(fd, level, option, value, len)
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    use crate::{
        fs::socket::{TCP_SOCKET_OPS, prepare_socket, socket_file_desc_ops},
        task::files::{OpenAccessMode, OpenedFileFinalReleaseCtx},
    };

    struct ScalarInput {
        bytes: [u8; size_of::<i32>()],
        reads: usize,
        fault: bool,
    }

    impl ScalarInput {
        fn value(value: i32) -> Self {
            Self {
                bytes: value.to_ne_bytes(),
                reads: 0,
                fault: false,
            }
        }

        fn fault() -> Self {
            Self {
                bytes: [0; size_of::<i32>()],
                reads: 0,
                fault: true,
            }
        }
    }

    impl OptionInput for ScalarInput {
        fn read(&mut self, bytes: &mut [u8]) -> Result<(), SysError> {
            self.reads += 1;
            if self.fault {
                return Err(SysError::BadAddress);
            }
            bytes.copy_from_slice(&self.bytes[..bytes.len()]);
            Ok(())
        }
    }

    #[kunit]
    fn tcp_option_length_and_value_precedence_use_the_production_dispatch() {
        assert_eq!(normalize_option_len(-1), Err(SysError::InvalidArgument));

        let (file, creation) = prepare_socket(&TCP_SOCKET_OPS).unwrap();
        creation.commit();
        let socket = socket_from_file(&file).unwrap();

        let mut unused = ScalarInput::value(1);
        assert_eq!(
            mutate_socket_option(socket, IPPROTO_TCP, TCP_NODELAY, 3, &mut unused),
            Err(SysError::InvalidArgument)
        );
        assert_eq!(unused.reads, 0);

        let mut fault = ScalarInput::fault();
        assert_eq!(
            mutate_socket_option(socket, IPPROTO_TCP, TCP_NODELAY, 4, &mut fault),
            Err(SysError::BadAddress)
        );
        assert_eq!(fault.reads, 1);

        let mut unknown = ScalarInput::value(1);
        assert_eq!(
            mutate_socket_option(socket, IPPROTO_TCP, TCP_NODELAY + 1, 4, &mut unknown),
            Err(SysError::ProtocolOptionNotSupported)
        );
        assert_eq!(unknown.reads, 0);

        let mut enabled = ScalarInput::value(1);
        assert_eq!(
            mutate_socket_option(socket, IPPROTO_TCP, TCP_NODELAY, 4, &mut enabled),
            Ok(())
        );
        assert_eq!(
            socket.query_option(SocketOptionQuery::TcpNoDelay),
            Ok(SocketOptionValue::Boolean(true))
        );

        (socket_file_desc_ops().final_release.unwrap())(OpenedFileFinalReleaseCtx {
            file: &file,
            access: OpenAccessMode::ReadWrite,
            notification_suppressed: true,
        });
    }
}
