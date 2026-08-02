use anemone_abi::syscall::SYS_SETSOCKOPT;

use crate::{fs::socket::socket_from_file, prelude::*, task::files::Fd};

#[syscall(SYS_SETSOCKOPT)]
fn sys_setsockopt(
    fd: i32,
    _level: i32,
    _option: i32,
    _value: u64,
    len: i32,
) -> Result<u64, SysError> {
    if len < 0 {
        return Err(SysError::InvalidArgument);
    }
    // Linux rejects a signed-negative optlen before looking up the fd. Keep
    // fd conversion here, after that ABI check, instead of in syscall parsing.
    let fd = Fd::new(fd as u32).ok_or(SysError::BadFileDescriptor)?;
    let desc = get_current_task().get_fd(fd)?;
    socket_from_file(desc.vfs_file()).ok_or(SysError::NotSocket)?;
    // R1 deliberately has no successful mutable option. Do not validate or
    // retain the caller's option buffer for an unsupported operation.
    Err(SysError::ProtocolOptionNotSupported)
}
