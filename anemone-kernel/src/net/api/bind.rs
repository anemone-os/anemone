//! bind system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/bind.2.html

use anemone_abi::syscall::SYS_BIND;

use smoltcp::wire::{IpAddress, IpEndpoint, IpListenEndpoint};

use crate::{
    net::{
        error::NetError,
        get_socket_shared,
        user_socket::{parse_sockaddr_in, with_default_eth_stack_mut, UserSocketKind},
    },
    prelude::{dt::UserReadPtr, *},
    task::files::Fd,
};

use smoltcp::socket::udp;

#[syscall(SYS_BIND)]
fn sys_bind(sockfd: Fd, addr: UserReadPtr<u8>, addrlen: u32) -> Result<u64, SysError> {
    let fd = with_current_task(|task| task.get_fd(sockfd).ok_or(SysError::BadFileDescriptor))?;
    let inner = get_socket_shared(&fd).ok_or(SysError::BadFileDescriptor)?;

    if inner.kind == UserSocketKind::Tcp {
        return Err(NetError::OperationNotSupported.into());
    }
    let mut buf = alloc::vec![0u8; addrlen as usize];
    addr.slice(addrlen as usize).safe_read(&mut buf)?;
    let (ipv4, port) = parse_sockaddr_in(&buf)?;
    if port == 0 {
        return Err(NetError::InvalidArgument.into());
    }
    with_default_eth_stack_mut(|stack| {
        if stack.name != inner.stack_name {
            return Err(NetError::NetworkDown.into());
        }
        let s = stack.sockets.get_mut::<udp::Socket>(inner.handle);
        s.bind(IpListenEndpoint::from(IpEndpoint::new(IpAddress::Ipv4(ipv4), port)))
            .map_err(|_| SysError::from(NetError::AddressInUse))?;
        crate::net::poll::poll_one_stack(stack);
        Ok(0u64)
    })
}
