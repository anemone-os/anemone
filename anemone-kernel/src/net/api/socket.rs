//! socket system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/socket.2.html

use anemone_abi::{
    net::linux::{af, ipproto, sock},
    syscall::SYS_SOCKET,
};

use crate::{
    net::{
        create_socket_file,
        user_socket::{
            alloc_ephemeral_port, default_ethernet_stack_name, make_tcp_socket,
            make_udp_packet_buffers, register_on_stack, SocketBackingBufs, UserSocketKind,
            UserSocketShared,
        },
    },
    prelude::*,
    task::files::{FdFlags, FileFlags},
};

use smoltcp::socket::udp;

#[syscall(SYS_SOCKET)]
fn sys_socket(domain: i32, kind_raw: i32, protocol: i32) -> Result<u64, SysError> {
    let mut ty = kind_raw as u32;
    let nonblock = (ty & sock::SOCK_NONBLOCK) != 0;
    let cloexec = (ty & sock::SOCK_CLOEXEC) != 0;
    ty &= !(sock::SOCK_NONBLOCK | sock::SOCK_CLOEXEC);

    if domain != af::AF_INET as i32 {
        return Err(crate::net::error::NetError::AddressFamilyNotSupported.into());
    }

    let stack_name = default_ethernet_stack_name()?;
    let stack_arc = {
        let table = crate::net::NET_STACK_TABLE.read_irqsave();
        table
            .stacks
            .get(&stack_name)
            .cloned()
            .ok_or(SysError::IO)?
    };
    let mut stack = stack_arc.lock_irqsave();

    let inner: Arc<UserSocketShared> = match ty {
        sock::SOCK_DGRAM => {
            if protocol != 0 && protocol != ipproto::UDP {
                return Err(crate::net::error::NetError::ProtocolNotSupported.into());
            }
            let (rx_buf, rx_meta, rx_data) = make_udp_packet_buffers();
            let (tx_buf, tx_meta, tx_data) = make_udp_packet_buffers();
            let h = stack.sockets.add(udp::Socket::new(rx_buf, tx_buf));
            {
                let s = stack.sockets.get_mut::<udp::Socket>(h);
                let _ = s.bind(alloc_ephemeral_port());
            }
            let backing = SocketBackingBufs::Udp { rx_meta, rx_data, tx_meta, tx_data };
            Arc::new(UserSocketShared::new(stack_name, h, UserSocketKind::Udp, backing))
        }
        sock::SOCK_STREAM => {
            if protocol != 0 && protocol != ipproto::TCP {
                return Err(crate::net::error::NetError::ProtocolNotSupported.into());
            }
            let (sock, rx_ptr, tx_ptr) = make_tcp_socket();
            let h = stack.sockets.add(sock);
            let backing = SocketBackingBufs::Tcp { rx: rx_ptr, tx: tx_ptr };
            Arc::new(UserSocketShared::new(stack_name, h, UserSocketKind::Tcp, backing))
        }
        _ => return Err(crate::net::error::NetError::SocketTypeNotSupported.into()),
    };

    register_on_stack(&mut stack, &inner);
    drop(stack);

    let mut ff = FileFlags::READ | FileFlags::WRITE;
    if nonblock {
        ff |= FileFlags::NONBLOCK;
    }
    let mut df = FdFlags::empty();
    if cloexec {
        df |= FdFlags::CLOSE_ON_EXEC;
    }
    let file = create_socket_file(inner, ff)?;
    let fd = with_current_task(|task| task.open_fd(file, ff, df))
        .ok_or(SysError::NoMoreFd)?;
    Ok(fd.raw() as u64)
}
