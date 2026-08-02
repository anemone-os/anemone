use super::*;

pub fn socket(family: u64, socket_type: u64, protocol: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_SOCKET, family, socket_type, protocol, 0, 0, 0) }
}

pub fn socketpair(
    family: u64,
    socket_type: u64,
    protocol: u64,
    pair_ptr: u64,
) -> Result<u64, Errno> {
    unsafe {
        syscall(
            SYS_SOCKETPAIR,
            family,
            socket_type,
            protocol,
            pair_ptr,
            0,
            0,
        )
    }
}

pub fn bind(fd: u64, addr: u64, addrlen: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_BIND, fd, addr, addrlen, 0, 0, 0) }
}

pub fn listen(fd: u64, backlog: i32) -> Result<u64, Errno> {
    unsafe { syscall(SYS_LISTEN, fd, backlog as i64 as u64, 0, 0, 0, 0) }
}

pub fn connect(fd: u64, addr: u64, addrlen: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_CONNECT, fd, addr, addrlen, 0, 0, 0) }
}

pub fn accept(fd: u64, addr: u64, addrlen: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_ACCEPT, fd, addr, addrlen, 0, 0, 0) }
}

pub fn accept4(fd: u64, addr: u64, addrlen: u64, flags: i32) -> Result<u64, Errno> {
    unsafe { syscall(SYS_ACCEPT4, fd, addr, addrlen, flags as i64 as u64, 0, 0) }
}

pub fn getsockname(fd: u64, addr: u64, addrlen: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_GETSOCKNAME, fd, addr, addrlen, 0, 0, 0) }
}

pub fn getpeername(fd: u64, addr: u64, addrlen: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_GETPEERNAME, fd, addr, addrlen, 0, 0, 0) }
}

pub fn sendto(
    fd: u64,
    buf: u64,
    len: u64,
    flags: u64,
    addr: u64,
    addrlen: u64,
) -> Result<u64, Errno> {
    unsafe { syscall(SYS_SENDTO, fd, buf, len, flags, addr, addrlen) }
}

pub fn recvfrom(
    fd: u64,
    buf: u64,
    len: u64,
    flags: u64,
    addr: u64,
    addrlen: u64,
) -> Result<u64, Errno> {
    unsafe { syscall(SYS_RECVFROM, fd, buf, len, flags, addr, addrlen) }
}

pub fn setsockopt(
    fd: u64,
    level: u64,
    option: u64,
    value: u64,
    len: u64,
) -> Result<u64, Errno> {
    unsafe { syscall(SYS_SETSOCKOPT, fd, level, option, value, len, 0) }
}

pub fn getsockopt(
    fd: u64,
    level: u64,
    option: u64,
    value: u64,
    len: u64,
) -> Result<u64, Errno> {
    unsafe { syscall(SYS_GETSOCKOPT, fd, level, option, value, len, 0) }
}

pub fn shutdown(fd: u64, how: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_SHUTDOWN, fd, how, 0, 0, 0, 0) }
}
