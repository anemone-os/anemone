//! socket system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/socket.2.html

use anemone_abi::{net::sock, syscall::SYS_SOCKET};

use crate::{
    net::{create_socket_file, user_socket::sys_socket_impl},
    prelude::*,
    task::files::{FdFlags, FileFlags},
};

#[syscall(SYS_SOCKET)]
fn sys_socket(domain: i32, kind_raw: i32, protocol: i32) -> Result<u64, SysError> {
    let mut ty = kind_raw as u32;
    let nonblock = (ty & sock::SOCK_NONBLOCK) != 0;
    let cloexec = (ty & sock::SOCK_CLOEXEC) != 0;
    ty &= !(sock::SOCK_NONBLOCK | sock::SOCK_CLOEXEC);
    let inner = sys_socket_impl(domain, ty as i32, protocol)?;
    let mut ff = FileFlags::READ | FileFlags::WRITE;
    if nonblock {
        ff |= FileFlags::NONBLOCK;
    }
    let mut df = FdFlags::empty();
    if cloexec {
        df |= FdFlags::CLOSE_ON_EXEC;
    }
    let file = create_socket_file(inner, ff)?;
    let fd = with_current_task(|task| task.open_fd(file, ff, df));
    Ok(fd as u64)
}
