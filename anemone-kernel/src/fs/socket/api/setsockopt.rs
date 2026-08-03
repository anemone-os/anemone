use core::mem::size_of;

use anemone_abi::{
    net::linux::{ICMP_FILTER, IP_TOS, IP_TTL, IPPROTO_IP, SOL_RAW},
    syscall::SYS_SETSOCKOPT,
};

use crate::{
    fs::socket::{
        SocketOptionError, SocketOptionMutation, SocketOptionQuery, SocketOptionValue, SocketType,
        socket_from_file,
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

fn read_scalar(value: u64, len: usize) -> Result<i32, SysError> {
    let copied = len.min(size_of::<i32>());
    if copied == 0 {
        return Ok(0);
    }
    let address = user_addr(value)?;
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mut bytes = [0u8; size_of::<i32>()];
    UserReadSlice::<u8>::try_new(address, copied, &mut uspace.lock())?
        .copy_to_slice(&mut bytes[..copied])?;
    Ok(if copied == size_of::<i32>() {
        i32::from_ne_bytes(bytes)
    } else {
        bytes[0] as i32
    })
}

#[syscall(SYS_SETSOCKOPT)]
fn sys_setsockopt(fd: i32, level: i32, option: i32, value: u64, len: i32) -> Result<u64, SysError> {
    if len < 0 {
        return Err(SysError::InvalidArgument);
    }
    // Linux rejects a signed-negative optlen before looking up the fd. Keep
    // fd conversion here, after that ABI check, instead of in syscall parsing.
    let fd = Fd::new(fd as u32).ok_or(SysError::BadFileDescriptor)?;
    let desc = get_current_task().get_fd(fd)?;
    let socket = socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    let len = len as usize;
    let mutation = match (socket.socket_type(), level, option) {
        (SocketType::Ipv4IcmpRaw, IPPROTO_IP, IP_TTL) => match read_scalar(value, len)? {
            -1 => SocketOptionMutation::Ipv4TimeToLive(NET_ICMP_RAW_DEFAULT_TTL),
            ttl @ 1..=255 => SocketOptionMutation::Ipv4TimeToLive(ttl as u8),
            _ => return Err(SysError::InvalidArgument),
        },
        (SocketType::Ipv4IcmpRaw, IPPROTO_IP, IP_TOS) => {
            SocketOptionMutation::Ipv4TypeOfService(read_scalar(value, len)? as u8)
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
            if copied != 0 {
                let address = user_addr(value)?;
                let task = get_current_task();
                let uspace = task.clone_uspace_handle();
                UserReadSlice::<u8>::try_new(address, copied, &mut uspace.lock())?
                    .copy_to_slice(&mut bytes[..copied])?;
            }
            SocketOptionMutation::IcmpTypeFilter(u32::from_ne_bytes(bytes))
        },
        _ => return Err(SysError::ProtocolOptionNotSupported),
    };
    socket.mutate_option(mutation).map_err(map_option_error)?;
    Ok(0)
}
