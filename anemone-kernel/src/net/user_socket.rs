//! User-visible sockets (`socket`, UDP/TCP) backed by smoltcp on a [`NetStack`](super::stack::NetStack).

use alloc::{boxed::Box, string::String, sync::Arc, vec, vec::Vec};
use core::sync::atomic::{AtomicU16, Ordering};

use smoltcp::{
    iface::SocketHandle,
    socket::{tcp, udp},
    wire::{IpAddress, IpEndpoint, Ipv4Address},
};

use anemone_abi::net::linux::{af, msg};

use crate::{
    device::net::{get_netdev, NetDevClass},
    net::error::NetError,
    prelude::*,
    sched::{add_to_ready, clone_current_task, sleep_as_waiting},
    syscall::dt::{UserReadPtr, UserWritePtr},
    task::{files::FileFlags, Task, TaskStatus},
};

use super::stack::NetStack;

static NEXT_EPHEMERAL: AtomicU16 = AtomicU16::new(49_152);

pub(crate) fn alloc_ephemeral_port() -> u16 {
    loop {
        let p = NEXT_EPHEMERAL.fetch_add(1, Ordering::Relaxed);
        if p >= 65_535 {
            NEXT_EPHEMERAL.store(49_152, Ordering::Relaxed);
        }
        if p != 0 {
            return p;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UserSocketKind {
    Udp,
    Tcp,
}

pub(crate) struct UserSocketEntry {
    pub(crate) handle: SocketHandle,
    pub(crate) kind: UserSocketKind,
    pub(crate) shared: alloc::sync::Weak<UserSocketShared>,
}

/// Owned backing buffers for a smoltcp socket.
///
/// smoltcp requires `'static` references for its socket buffers. We fulfil
/// this by using `Box::leak`, which gives a `&'static mut [T]`. The raw
/// pointer is stored here so that when the socket is closed the memory can be
/// reclaimed via `Box::from_raw`.
///
/// # Drop ordering
///
/// `SocketBackingBufs` is a field of [`UserSocketShared`]. The manual
/// [`Drop`] impl on `UserSocketShared` first calls
/// `remove_user_socket_handle`, which removes the smoltcp socket from the
/// `SocketSet` (dropping it and releasing its hold on the `'static`
/// references). Only after that function returns do the struct fields get
/// dropped, which means `SocketBackingBufs::drop` runs when the smoltcp
/// socket no longer holds any references to the buffers — making the
/// `Box::from_raw` calls below safe.
pub(crate) enum SocketBackingBufs {
    Udp {
        rx_meta: *mut [udp::PacketMetadata],
        rx_data: *mut [u8],
        tx_meta: *mut [udp::PacketMetadata],
        tx_data: *mut [u8],
    },
    Tcp {
        rx: *mut [u8],
        tx: *mut [u8],
    },
}

unsafe impl Send for SocketBackingBufs {}
unsafe impl Sync for SocketBackingBufs {}

impl Drop for SocketBackingBufs {
    fn drop(&mut self) {
        // Safety: All pointers come from `Box::leak`. The smoltcp socket that
        // held `'static` references to these slices has already been removed
        // from the `SocketSet` by `UserSocketShared::drop` before this runs.
        unsafe {
            match self {
                Self::Udp { rx_meta, rx_data, tx_meta, tx_data } => {
                    drop(Box::from_raw(*rx_meta));
                    drop(Box::from_raw(*rx_data));
                    drop(Box::from_raw(*tx_meta));
                    drop(Box::from_raw(*tx_data));
                }
                Self::Tcp { rx, tx } => {
                    drop(Box::from_raw(*rx));
                    drop(Box::from_raw(*tx));
                }
            }
        }
    }
}

pub struct UserSocketShared {
    pub(crate) stack_name: String,
    pub(crate) handle: SocketHandle,
    pub(crate) kind: UserSocketKind,
    /// Backing heap buffers lent to smoltcp. Dropped after the smoltcp socket
    /// is removed from the `SocketSet` (see [`Drop`] impl below).
    _backing_bufs: SocketBackingBufs,
    wait_recv: SpinLock<Vec<Arc<Task>>>,
    wait_connect: SpinLock<Vec<Arc<Task>>>,
    wait_send: SpinLock<Vec<Arc<Task>>>,
}

impl UserSocketShared {
    pub(crate) fn new(
        stack_name: String,
        handle: SocketHandle,
        kind: UserSocketKind,
        backing_bufs: SocketBackingBufs,
    ) -> Self {
        Self {
            stack_name,
            handle,
            kind,
            _backing_bufs: backing_bufs,
            wait_recv: SpinLock::new(Vec::new()),
            wait_connect: SpinLock::new(Vec::new()),
            wait_send: SpinLock::new(Vec::new()),
        }
    }

    fn push_wait_unique(lock: &SpinLock<Vec<Arc<Task>>>, task: Arc<Task>) {
        let mut g = lock.lock_irqsave();
        if !g.iter().any(|t| t.tid() == task.tid()) {
            g.push(task);
        }
    }

    pub(crate) fn push_recv_waiter(&self, task: Arc<Task>) {
        Self::push_wait_unique(&self.wait_recv, task);
    }

    pub(crate) fn push_connect_waiter(&self, task: Arc<Task>) {
        Self::push_wait_unique(&self.wait_connect, task);
    }

    pub(crate) fn push_send_waiter(&self, task: Arc<Task>) {
        Self::push_wait_unique(&self.wait_send, task);
    }

    pub(crate) fn remove_waiter_if_present(&self, task: &Task) {
        let tid = task.tid();
        {
            let mut g = self.wait_recv.lock_irqsave();
            g.retain(|t| t.tid() != tid);
        }
        {
            let mut g = self.wait_connect.lock_irqsave();
            g.retain(|t| t.tid() != tid);
        }
        {
            let mut g = self.wait_send.lock_irqsave();
            g.retain(|t| t.tid() != tid);
        }
    }

    fn wake_wait_list(lock: &SpinLock<Vec<Arc<Task>>>) {
        let tasks: Vec<_> = {
            let mut g = lock.lock_irqsave();
            core::mem::take(&mut *g)
        };
        for t in tasks {
            if matches!(t.status(), TaskStatus::Waiting { .. }) {
                add_to_ready(t);
            }
        }
    }

    pub(crate) fn wake_recv_waiters(&self) {
        Self::wake_wait_list(&self.wait_recv);
    }

    pub(crate) fn wake_connect_waiters(&self) {
        Self::wake_wait_list(&self.wait_connect);
    }

    pub(crate) fn wake_send_waiters(&self) {
        Self::wake_wait_list(&self.wait_send);
    }
}

impl Drop for UserSocketShared {
    fn drop(&mut self) {
        // Remove the smoltcp socket from the SocketSet first. This drops the
        // smoltcp socket struct, which gives up its `'static` references to
        // the backing buffers. The `_backing_bufs` field is dropped afterwards
        // (automatically, as a struct field), which is safe because the
        // smoltcp socket no longer holds any references to those slices.
        super::remove_user_socket_handle(&self.stack_name, self.handle);
    }
}

pub(crate) fn register_on_stack(stack: &mut NetStack, inner: &Arc<UserSocketShared>) {
    stack.user_socket_entries.push(UserSocketEntry {
        handle: inner.handle,
        kind: inner.kind,
        shared: Arc::downgrade(inner),
    });
}

pub(crate) fn parse_sockaddr_in(buf: &[u8]) -> Result<(Ipv4Address, u16), SysError> {
    if buf.len() < 8 {
        return Err(NetError::InvalidArgument.into());
    }
    let family = u16::from_ne_bytes([buf[0], buf[1]]);
    if family != af::AF_INET as u16 {
        return Err(NetError::AddressFamilyNotSupported.into());
    }
    let port = u16::from_be_bytes([buf[2], buf[3]]);
    let addr = Ipv4Address::new(buf[4], buf[5], buf[6], buf[7]);
    Ok((addr, port))
}

pub(crate) fn ip_endpoint_v4(addr: Ipv4Address, port: u16) -> IpEndpoint {
    IpEndpoint::new(IpAddress::Ipv4(addr), port)
}

pub(crate) fn emit_sockaddr_in(out: &mut [u8], addr: Ipv4Address, port: u16) -> Result<usize, SysError> {
    if out.len() < 16 {
        return Err(NetError::InvalidArgument.into());
    }
    out[0..2].copy_from_slice(&(af::AF_INET as u16).to_ne_bytes());
    out[2..4].copy_from_slice(&port.to_be_bytes());
    out[4..8].copy_from_slice(&addr.octets());
    out[8..16].fill(0);
    Ok(16)
}

pub(crate) fn post_poll_wake(stack: &mut NetStack) {
    stack.user_socket_entries.retain_mut(|e| {
        let Some(shared) = e.shared.upgrade() else {
            // Stale row: no live `Arc<UserSocketShared>` (e.g. ordering glitch or missed Drop).
            // Drop the smoltcp socket so handles stay aligned with `SocketSet`.
            let _ = stack.sockets.remove(e.handle);
            return false;
        };
        match e.kind {
            UserSocketKind::Udp => {
                let s = stack.sockets.get_mut::<udp::Socket>(e.handle);
                if s.can_recv() {
                    shared.wake_recv_waiters();
                }
                if s.can_send() {
                    shared.wake_send_waiters();
                }
            }
            UserSocketKind::Tcp => {
                let s = stack.sockets.get_mut::<tcp::Socket>(e.handle);
                use smoltcp::socket::tcp::State;
                let st = s.state();
                if s.can_recv() {
                    shared.wake_recv_waiters();
                }
                if s.can_send() && s.may_send() {
                    shared.wake_send_waiters();
                }
                if matches!(
                    st,
                    State::Established
                        | State::CloseWait
                        | State::Closed
                        | State::FinWait1
                        | State::FinWait2
                ) {
                    shared.wake_connect_waiters();
                }
            }
        }
        true
    });
}

pub(crate) fn default_ethernet_stack_name() -> Result<String, SysError> {
    let table = super::NET_STACK_TABLE.read_irqsave();
    for name in &table.ordered {
        if get_netdev(name.as_str()).map(|d| d.class()) == Some(NetDevClass::Ethernet) {
            return Ok(name.clone());
        }
    }
    Err(NetError::NetworkDown.into())
}

pub(crate) fn with_default_eth_stack_mut<R>(
    f: impl FnOnce(&mut NetStack) -> Result<R, SysError>,
) -> Result<R, SysError> {
    let name = default_ethernet_stack_name()?;
    let arc = {
        let table = super::NET_STACK_TABLE.read_irqsave();
        table
            .stacks
            .get(&name)
            .cloned()
            .ok_or_else(|| SysError::from(NetError::NetworkDown))?
    };
    let mut stack = arc.lock_irqsave();
    f(&mut stack)
}

/// Allocate a UDP packet buffer backed by heap memory, returning both the
/// smoltcp `PacketBuffer` (with `'static` references) and the raw pointers
/// needed to reclaim the memory later.
pub(crate) fn make_udp_packet_buffers()
    -> (udp::PacketBuffer<'static>, *mut [udp::PacketMetadata], *mut [u8])
{
    const SLOTS: usize = 8;
    const PAYLOAD: usize = 2048;
    let metadata: &'static mut [udp::PacketMetadata] =
        Box::leak(vec![udp::PacketMetadata::EMPTY; SLOTS].into_boxed_slice());
    let payload: &'static mut [u8] =
        Box::leak(vec![0u8; SLOTS * PAYLOAD].into_boxed_slice());
    let meta_ptr: *mut [udp::PacketMetadata] = metadata;
    let data_ptr: *mut [u8] = payload;
    (udp::PacketBuffer::new(metadata, payload), meta_ptr, data_ptr)
}

/// Allocate a TCP socket with heap-backed rx/tx ring buffers, returning both
/// the smoltcp socket (with `'static` references) and the raw pointers needed
/// to reclaim the memory later.
pub(crate) fn make_tcp_socket() -> (tcp::Socket<'static>, *mut [u8], *mut [u8]) {
    const SZ: usize = 4096;
    let rx: &'static mut [u8] = Box::leak(vec![0u8; SZ].into_boxed_slice());
    let tx: &'static mut [u8] = Box::leak(vec![0u8; SZ].into_boxed_slice());
    let rx_ptr: *mut [u8] = rx;
    let tx_ptr: *mut [u8] = tx;
    (tcp::Socket::new(tcp::SocketBuffer::new(rx), tcp::SocketBuffer::new(tx)), rx_ptr, tx_ptr)
}

pub(crate) fn user_socket_read(
    shared: &Arc<UserSocketShared>,
    buf: &mut [u8],
    file_flags: FileFlags,
) -> Result<usize, SysError> {
    if buf.is_empty() {
        return Ok(0);
    }
    let stack_name = shared.stack_name.clone();
    let h = shared.handle;
    loop {
        let (n, eof) = with_default_eth_stack_mut(|stack| {
            if stack.name != stack_name {
                return Err(NetError::NetworkDown.into());
            }
            super::poll::poll_one_stack(stack);
            match shared.kind {
                UserSocketKind::Udp => {
                    let s = stack.sockets.get_mut::<udp::Socket>(h);
                    if s.can_recv() {
                        match s.recv_slice(buf) {
                            Ok((n, _)) => Ok((n, false)),
                            Err(_) => Ok((0, false)),
                        }
                    } else {
                        Ok((0, false))
                    }
                }
                UserSocketKind::Tcp => {
                    let s = stack.sockets.get_mut::<tcp::Socket>(h);
                    if s.can_recv() {
                        match s.recv_slice(buf) {
                            Ok(n) => Ok((n, false)),
                            Err(smoltcp::socket::tcp::RecvError::Finished) => Ok((0, true)),
                            Err(_) => Ok((0, false)),
                        }
                    } else {
                        use smoltcp::socket::tcp::State;
                        let eof = matches!(s.state(), State::Closed | State::TimeWait);
                        Ok((0, eof))
                    }
                }
            }
        })?;
        if n > 0 || eof {
            return Ok(n);
        }
        if file_flags.contains(FileFlags::NONBLOCK) {
            return Err(NetError::WouldBlock.into());
        }
        let task = clone_current_task();
        shared.push_recv_waiter(task.clone());
        task.set_status(TaskStatus::Waiting { interruptible: true });
        let quick = with_default_eth_stack_mut(|stack| {
            super::poll::poll_one_stack(stack);
            match shared.kind {
                UserSocketKind::Udp => Ok(stack.sockets.get_mut::<udp::Socket>(h).can_recv()),
                UserSocketKind::Tcp => Ok(stack.sockets.get_mut::<tcp::Socket>(h).can_recv()),
            }
        })?;
        shared.remove_waiter_if_present(task.as_ref());
        if quick {
            continue;
        }
        sleep_as_waiting(true);
    }
}

pub(crate) fn user_socket_write(
    shared: &Arc<UserSocketShared>,
    buf: &[u8],
    file_flags: FileFlags,
) -> Result<usize, SysError> {
    if buf.is_empty() {
        return Ok(0);
    }
    if shared.kind != UserSocketKind::Tcp {
        return Err(NetError::OperationNotSupported.into());
    }
    let stack_name = shared.stack_name.clone();
    let h = shared.handle;
    loop {
        let r: Result<usize, SysError> = with_default_eth_stack_mut(|stack| {
            if stack.name != stack_name {
                return Err(NetError::NetworkDown.into());
            }
            super::poll::poll_one_stack(stack);
            let s = stack.sockets.get_mut::<tcp::Socket>(h);
            if !s.may_send() {
                return Err(NetError::NotConnected.into());
            }
            if !file_flags.contains(FileFlags::NONBLOCK) && !s.can_send() {
                return Ok(0);
            }
            if file_flags.contains(FileFlags::NONBLOCK) && !s.can_send() {
                return Err(NetError::WouldBlock.into());
            }
            s.send_slice(buf)
                .map_err(|_| NetError::BrokenPipe.into())
        });
        match r {
            Ok(0) if !file_flags.contains(FileFlags::NONBLOCK) => {
                let task = clone_current_task();
                shared.push_send_waiter(task.clone());
                task.set_status(TaskStatus::Waiting { interruptible: true });
                let quick = with_default_eth_stack_mut(|stack| {
                    super::poll::poll_one_stack(stack);
                    let s = stack.sockets.get_mut::<tcp::Socket>(h);
                    Ok(s.can_send() && s.may_send())
                })?;
                shared.remove_waiter_if_present(task.as_ref());
                if quick {
                    continue;
                }
                sleep_as_waiting(true);
            }
            Ok(n) => return Ok(n),
            Err(e) => return Err(e),
        }
    }
}

/// Implementation shared by `sys_sendto` and the VFS write path.
pub(crate) fn do_sendto(
    shared: &Arc<UserSocketShared>,
    buf: UserReadPtr<u8>,
    len: usize,
    flags: u32,
    dest_addr: UserReadPtr<u8>,
    addrlen: u32,
    file_flags: FileFlags,
) -> Result<u64, SysError> {
    let nb = file_flags.contains(FileFlags::NONBLOCK) || (flags & msg::MSG_DONTWAIT) != 0;
    if len == 0 {
        return Ok(0);
    }
    let mut kbuf = vec![0u8; len];
    buf.slice(len).safe_read(&mut kbuf)?;
    let stack_name = shared.stack_name.clone();
    let h = shared.handle;
    loop {
        let r: Result<usize, SysError> = with_default_eth_stack_mut(|stack| {
            if stack.name != stack_name {
                return Err(NetError::NetworkDown.into());
            }
            super::poll::poll_one_stack(stack);
            match shared.kind {
                UserSocketKind::Udp => {
                    if addrlen == 0 {
                        return Err(NetError::DestinationAddressRequired.into());
                    }
                    let mut abuf = vec![0u8; addrlen as usize];
                    dest_addr
                        .slice(addrlen as usize)
                        .safe_read(&mut abuf)?;
                    let (a, p) = parse_sockaddr_in(&abuf)?;
                    let dest = ip_endpoint_v4(a, p);
                    let s = stack.sockets.get_mut::<udp::Socket>(h);
                    if !nb && !s.can_send() {
                        return Ok(0);
                    }
                    if nb && !s.can_send() {
                        return Err(NetError::WouldBlock.into());
                    }
                    s.send_slice(&kbuf, dest)
                        .map(|_| kbuf.len())
                        .map_err(|_| SysError::from(NetError::MessageTooLong))
                }
                UserSocketKind::Tcp => {
                    let s = stack.sockets.get_mut::<tcp::Socket>(h);
                    if !s.may_send() {
                        return Err(NetError::NotConnected.into());
                    }
                    if !nb && !s.can_send() {
                        return Ok(0);
                    }
                    if nb && !s.can_send() {
                        return Err(NetError::WouldBlock.into());
                    }
                    s.send_slice(&kbuf)
                        .map_err(|_| NetError::BrokenPipe.into())
                }
            }
        });
        match r {
            Ok(0) if !nb => {
                let task = clone_current_task();
                shared.push_send_waiter(task.clone());
                task.set_status(TaskStatus::Waiting { interruptible: true });
                let quick = with_default_eth_stack_mut(|stack| {
                    super::poll::poll_one_stack(stack);
                    match shared.kind {
                        UserSocketKind::Udp => {
                            Ok(stack.sockets.get_mut::<udp::Socket>(h).can_send())
                        }
                        UserSocketKind::Tcp => {
                            let s = stack.sockets.get_mut::<tcp::Socket>(h);
                            Ok(s.can_send() && s.may_send())
                        }
                    }
                })?;
                shared.remove_waiter_if_present(task.as_ref());
                if quick {
                    continue;
                }
                sleep_as_waiting(true);
            }
            Ok(n) => return Ok(n as u64),
            Err(e) => return Err(e),
        }
    }
}

/// Implementation shared by `sys_recvfrom` and the VFS read path.
pub(crate) fn do_recvfrom(
    shared: &Arc<UserSocketShared>,
    buf: UserWritePtr<u8>,
    len: usize,
    flags: u32,
    src_addr: UserWritePtr<u8>,
    addrlen_ptr: UserWritePtr<u32>,
    max_addr_len: u32,
    file_flags: FileFlags,
) -> Result<u64, SysError> {
    let nb = file_flags.contains(FileFlags::NONBLOCK) || (flags & msg::MSG_DONTWAIT) != 0;
    if len == 0 {
        return Ok(0);
    }
    let stack_name = shared.stack_name.clone();
    let h = shared.handle;
    loop {
        let mut scratch = vec![0u8; len];
        let (n, meta_opt) = with_default_eth_stack_mut(|stack| {
            if stack.name != stack_name {
                return Err(NetError::NetworkDown.into());
            }
            super::poll::poll_one_stack(stack);
            match shared.kind {
                UserSocketKind::Udp => {
                    let s = stack.sockets.get_mut::<udp::Socket>(h);
                    if s.can_recv() {
                        match s.recv_slice(&mut scratch) {
                            Ok((n, meta)) => Ok((n, Some(meta.endpoint))),
                            Err(_) => Ok((0, None)),
                        }
                    } else {
                        Ok((0, None))
                    }
                }
                UserSocketKind::Tcp => {
                    let s = stack.sockets.get_mut::<tcp::Socket>(h);
                    if s.can_recv() {
                        match s.recv_slice(&mut scratch) {
                            Ok(n) => Ok((n, s.remote_endpoint())),
                            Err(smoltcp::socket::tcp::RecvError::Finished) => Ok((0, None)),
                            Err(_) => Ok((0, None)),
                        }
                    } else {
                        Ok((0, None))
                    }
                }
            }
        })?;
        if n > 0 {
            let out = n.min(len);
            buf.slice(out).safe_write(&scratch[..out])?;
            let ret = out as u64;
            if src_addr.addr() != 0 {
                if let Some(ep) = meta_opt {
                    let IpAddress::Ipv4(a) = ep.addr;
                    let mut out = [0u8; 16];
                    let _ = emit_sockaddr_in(&mut out, a, ep.port)?;
                    let out_len = (max_addr_len as usize).min(16);
                    src_addr.slice(out_len).safe_write(&out[..out_len])?;
                    addrlen_ptr.safe_write(out_len as u32)?;
                }
            }
            return Ok(ret);
        }
        if shared.kind == UserSocketKind::Tcp {
            let closed = with_default_eth_stack_mut(|stack| {
                let s = stack.sockets.get_mut::<tcp::Socket>(h);
                use smoltcp::socket::tcp::State;
                Ok(matches!(s.state(), State::Closed | State::TimeWait) && !s.can_recv())
            })?;
            if closed {
                return Ok(0);
            }
        }
        if nb {
            return Err(NetError::WouldBlock.into());
        }
        let task = clone_current_task();
        shared.push_recv_waiter(task.clone());
        task.set_status(TaskStatus::Waiting { interruptible: true });
        let quick = with_default_eth_stack_mut(|stack| {
            super::poll::poll_one_stack(stack);
            match shared.kind {
                UserSocketKind::Udp => Ok(stack.sockets.get_mut::<udp::Socket>(h).can_recv()),
                UserSocketKind::Tcp => Ok(stack.sockets.get_mut::<tcp::Socket>(h).can_recv()),
            }
        })?;
        shared.remove_waiter_if_present(task.as_ref());
        if quick {
            continue;
        }
        sleep_as_waiting(true);
    }
}
