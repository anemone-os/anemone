//! connect system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/connect.2.html

use anemone_abi::syscall::SYS_CONNECT;

use crate::{
    net::{
        error::NetError,
        get_socket_shared,
        user_socket::{
            alloc_ephemeral_port, ip_endpoint_v4, parse_sockaddr_in, UserSocketKind,
        },
    },
    prelude::{dt::UserReadPtr, *},
    sched::{clone_current_task, sleep_as_waiting},
    task::{files::Fd, TaskStatus},
};

use smoltcp::socket::tcp;

#[syscall(SYS_CONNECT)]
fn sys_connect(sockfd: Fd, addr: UserReadPtr<u8>, addrlen: u32) -> Result<u64, SysError> {
    let fd = with_current_task(|task| task.get_fd(sockfd).ok_or(SysError::BadFileDescriptor))?;
    let inner = get_socket_shared(&fd).ok_or(SysError::BadFileDescriptor)?;

    if inner.kind != UserSocketKind::Tcp {
        return Err(NetError::OperationNotSupported.into());
    }
    let mut buf = alloc::vec![0u8; addrlen as usize];
    addr.slice(addrlen as usize).safe_read(&mut buf)?;
    let (ipv4, port) = parse_sockaddr_in(&buf)?;
    let remote = ip_endpoint_v4(ipv4, port);
    let local_port = alloc_ephemeral_port();
    let stack_name = inner.stack_name.clone();
    let h = inner.handle;

    {
        let table = crate::net::NET_STACK_TABLE.read_irqsave();
        let arc = table
            .stacks
            .get(&stack_name)
            .cloned()
            .ok_or(NetError::NetworkDown)?;
        let mut stack = arc.lock_irqsave();
        {
            let net = &mut *stack;
            let iface = &mut net.iface;
            let sockets = &mut net.sockets;
            let cx = iface.context();
            let s = sockets.get_mut::<tcp::Socket>(h);
            s.connect(cx, remote, local_port)
                .map_err(|_| SysError::from(NetError::InvalidArgument))?;
        }
        crate::net::poll::poll_one_stack(&mut stack);
    }

    loop {
        let progress = {
            let table = crate::net::NET_STACK_TABLE.read_irqsave();
            let Some(arc) = table.stacks.get(&stack_name).cloned() else {
                return Err(NetError::NetworkDown.into());
            };
            let mut stack = arc.lock_irqsave();
            crate::net::poll::poll_one_stack(&mut stack);
            let s = stack.sockets.get_mut::<tcp::Socket>(h);
            use smoltcp::socket::tcp::State;
            match s.state() {
                State::Established => 1i32,
                State::Closed => -1,
                _ => 0,
            }
        };
        if progress == 1 {
            break;
        }
        if progress == -1 {
            return Err(NetError::ConnectionRefused.into());
        }
        let task = clone_current_task();
        inner.push_connect_waiter(task.clone());
        task.set_status(TaskStatus::Waiting { interruptible: true });
        let quick = {
            let table = crate::net::NET_STACK_TABLE.read_irqsave();
            let Some(arc) = table.stacks.get(&stack_name).cloned() else {
                return Err(NetError::NetworkDown.into());
            };
            let mut stack = arc.lock_irqsave();
            crate::net::poll::poll_one_stack(&mut stack);
            let s = stack.sockets.get_mut::<tcp::Socket>(h);
            use smoltcp::socket::tcp::State;
            match s.state() {
                State::Established => 1i32,
                State::Closed => -1,
                _ => 0,
            }
        };
        inner.remove_waiter_if_present(task.as_ref());
        if quick == 1 {
            break;
        }
        if quick == -1 {
            return Err(NetError::ConnectionRefused.into());
        }
        sleep_as_waiting(true);
    }

    let table = crate::net::NET_STACK_TABLE.read_irqsave();
    let arc = table
        .stacks
        .get(&stack_name)
        .cloned()
        .ok_or(NetError::NetworkDown)?;
    let mut stack = arc.lock_irqsave();
    let s = stack.sockets.get_mut::<tcp::Socket>(h);
    use smoltcp::socket::tcp::State;
    match s.state() {
        State::Established => Ok(0),
        _ => Err(NetError::ConnectionRefused.into()),
    }
}
