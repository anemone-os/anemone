//! Linux-compatible socket-related system calls.

use anemone_abi::{
    net::sock,
    syscall::{
        SYS_ACCEPT, SYS_ACCEPT4, SYS_BIND, SYS_CONNECT, SYS_GETSOCKOPT, SYS_LISTEN,
        SYS_RECVFROM, SYS_SENDTO, SYS_SETSOCKOPT, SYS_SHUTDOWN, SYS_SOCKET, SYS_SOCKETPAIR,
    },
};

use crate::{
    net::user_socket::{
        sys_bind_impl, sys_connect_impl, sys_recvfrom_impl, sys_sendto_impl, sys_socket_impl,
        UserSocket,
    },
    prelude::{dt::UserReadPtr, dt::UserWritePtr, *},
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
    let sock = UserSocket { inner };
    let fd = with_current_task(|task| task.open_socket_fd(sock, ff, df));
    Ok(fd as u64)
}

#[syscall(SYS_BIND)]
fn sys_bind(sockfd: usize, addr: UserReadPtr<u8>, addrlen: u32) -> Result<u64, SysError> {
    let fd = with_current_task(|task| task.get_fd(sockfd).ok_or(KernelError::BadFileDescriptor))?;
    let inner = fd
        .user_socket()
        .ok_or(KernelError::BadFileDescriptor)?
        .inner
        .clone();
    sys_bind_impl(&inner, addr, addrlen)
}

#[syscall(SYS_CONNECT)]
fn sys_connect(sockfd: usize, addr: UserReadPtr<u8>, addrlen: u32) -> Result<u64, SysError> {
    let fd = with_current_task(|task| task.get_fd(sockfd).ok_or(KernelError::BadFileDescriptor))?;
    let inner = fd
        .user_socket()
        .ok_or(KernelError::BadFileDescriptor)?
        .inner
        .clone();
    sys_connect_impl(&inner, addr, addrlen)
}

#[syscall(SYS_SENDTO)]
fn sys_sendto(
    sockfd: usize,
    buf: UserReadPtr<u8>,
    len: usize,
    flags: u32,
    dest_addr: UserReadPtr<u8>,
    addrlen: u32,
) -> Result<u64, SysError> {
    let fd = with_current_task(|task| task.get_fd(sockfd).ok_or(KernelError::BadFileDescriptor))?;
    let inner = fd
        .user_socket()
        .ok_or(KernelError::BadFileDescriptor)?
        .inner
        .clone();
    sys_sendto_impl(
        &inner,
        buf,
        len,
        flags,
        dest_addr,
        addrlen,
        fd.file_flags(),
    )
}

#[syscall(SYS_RECVFROM)]
fn sys_recvfrom(
    sockfd: usize,
    buf: UserWritePtr<u8>,
    len: usize,
    flags: u32,
    src_addr: UserWritePtr<u8>,
    addrlen: UserWritePtr<u32>,
) -> Result<u64, SysError> {
    let fd = with_current_task(|task| task.get_fd(sockfd).ok_or(KernelError::BadFileDescriptor))?;
    let inner = fd
        .user_socket()
        .ok_or(KernelError::BadFileDescriptor)?
        .inner
        .clone();
    let max_addr = if addrlen.addr() == 0 {
        0u32
    } else {
        UserReadPtr::<u32>::from_raw(addrlen.addr())?.safe_read()?
    };
    sys_recvfrom_impl(
        &inner,
        buf,
        len,
        flags,
        src_addr,
        addrlen,
        max_addr,
        fd.file_flags(),
    )
}

#[syscall(SYS_LISTEN)]
fn sys_listen(_sockfd: usize, _backlog: i32) -> Result<u64, SysError> {
    Err(KernelError::Errno(anemone_abi::errno::EOPNOTSUPP).into())
}

#[syscall(SYS_ACCEPT)]
fn sys_accept(_sockfd: usize, _addr: UserWritePtr<u8>, _addrlen: UserWritePtr<u32>) -> Result<u64, SysError> {
    Err(KernelError::Errno(anemone_abi::errno::EOPNOTSUPP).into())
}

#[syscall(SYS_ACCEPT4)]
fn sys_accept4(
    _sockfd: usize,
    _addr: UserWritePtr<u8>,
    _addrlen: UserWritePtr<u32>,
    _flags: i32,
) -> Result<u64, SysError> {
    Err(KernelError::Errno(anemone_abi::errno::EOPNOTSUPP).into())
}

#[syscall(SYS_SOCKETPAIR)]
fn sys_socketpair(
    _domain: i32,
    _ty: i32,
    _protocol: i32,
    _sv: UserWritePtr<i32>,
) -> Result<u64, SysError> {
    Err(KernelError::Errno(anemone_abi::errno::EOPNOTSUPP).into())
}

#[syscall(SYS_SHUTDOWN)]
fn sys_shutdown(_sockfd: usize, _how: i32) -> Result<u64, SysError> {
    Err(KernelError::Errno(anemone_abi::errno::ENOTCONN).into())
}

#[syscall(SYS_SETSOCKOPT)]
fn sys_setsockopt(
    _sockfd: usize,
    _level: i32,
    _optname: i32,
    _optval: UserReadPtr<u8>,
    _optlen: u32,
) -> Result<u64, SysError> {
    Err(KernelError::Errno(anemone_abi::errno::EOPNOTSUPP).into())
}

#[syscall(SYS_GETSOCKOPT)]
fn sys_getsockopt(
    _sockfd: usize,
    _level: i32,
    _optname: i32,
    _optval: UserWritePtr<u8>,
    _optlen: UserWritePtr<u32>,
) -> Result<u64, SysError> {
    Err(KernelError::Errno(anemone_abi::errno::EINVAL).into())
}
