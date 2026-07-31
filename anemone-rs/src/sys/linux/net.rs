use super::*;

pub fn socket(family: u64, socket_type: u64, protocol: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_SOCKET, family, socket_type, protocol, 0, 0, 0) }
}

pub fn bind(fd: u64, addr: u64, addrlen: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_BIND, fd, addr, addrlen, 0, 0, 0) }
}

pub fn getsockname(fd: u64, addr: u64, addrlen: u64) -> Result<u64, Errno> {
    unsafe { syscall(SYS_GETSOCKNAME, fd, addr, addrlen, 0, 0, 0) }
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
