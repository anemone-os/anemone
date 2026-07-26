use anemone_abi::{
    fs::linux::epoll::{
        EPOLL_CTL_ADD, EPOLL_CTL_DEL, EPOLL_CTL_MOD, EPOLLET, EPOLLEXCLUSIVE, EPOLLHUP, EPOLLIN,
        EPOLLMSG, EPOLLONESHOT, EPOLLOUT, EPOLLPRI, EPOLLRDBAND, EPOLLRDHUP, EPOLLRDNORM,
        EPOLLWAKEUP, EPOLLWRBAND, EPOLLWRNORM, EpollEvent,
    },
    syscall::SYS_EPOLL_CTL,
};

use crate::{
    fs::epoll::{WatchPolicy, epoll_from_file},
    prelude::*,
    syscall::{
        handler::TryFromSyscallArg,
        user_access::{UserReadSlice, user_addr},
    },
    task::files::Fd,
};

use super::resolve_epoll_fd;

const COMPAT_READINESS_BITS: u32 =
    EPOLLPRI | EPOLLRDNORM | EPOLLRDBAND | EPOLLWRNORM | EPOLLWRBAND | EPOLLMSG | EPOLLRDHUP;
const ACCEPTED_EVENT_BITS: u32 = EPOLLIN
    | EPOLLOUT
    | anemone_abi::fs::linux::epoll::EPOLLERR
    | EPOLLHUP
    | COMPAT_READINESS_BITS
    | EPOLLWAKEUP
    | EPOLLONESHOT
    | EPOLLET;

#[derive(Debug, Clone, Copy)]
enum EpollCtlOp {
    Add,
    Delete,
    Modify,
}

impl TryFromSyscallArg for EpollCtlOp {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        match i32::try_from_syscall_arg(raw)? {
            EPOLL_CTL_ADD => Ok(Self::Add),
            EPOLL_CTL_DEL => Ok(Self::Delete),
            EPOLL_CTL_MOD => Ok(Self::Modify),
            op => {
                knoticeln!("sys_epoll_ctl: unsupported operation {}", op);
                Err(SysError::InvalidArgument)
            },
        }
    }
}

fn read_event(addr: Option<VirtAddr>) -> Result<EpollEvent, SysError> {
    let addr = addr.ok_or(SysError::BadAddress)?;
    let task = get_current_task();
    let usp_handle = task.clone_uspace_handle();
    let mut usp = usp_handle.lock();
    let mut bytes = [0u8; size_of::<EpollEvent>()];
    UserReadSlice::<u8>::try_new(addr, bytes.len(), &mut usp)?.copy_to_slice(&mut bytes);
    Ok(EpollEvent::new(
        u32::from_ne_bytes(bytes[0..4].try_into().unwrap()),
        u64::from_ne_bytes(bytes[8..16].try_into().unwrap()),
    ))
}

fn watch_policy(event: EpollEvent) -> Result<WatchPolicy, SysError> {
    if event.events & EPOLLEXCLUSIVE != 0 {
        knoticeln!("sys_epoll_ctl: EPOLLEXCLUSIVE is not supported");
        return Err(SysError::InvalidArgument);
    }
    let unknown = event.events & !(ACCEPTED_EVENT_BITS | EPOLLEXCLUSIVE);
    if unknown != 0 {
        knoticeln!("sys_epoll_ctl: unknown event bits {:#x}", unknown);
        return Err(SysError::InvalidArgument);
    }
    let compat = event.events & COMPAT_READINESS_BITS;
    if compat != 0 {
        // These Linux readiness classes currently have no internal source
        // truth. Accepting them is observable compatibility only; remove this
        // notice when PollEvent and source registration can produce the bits.
        knoticeln!(
            "sys_epoll_ctl: accepted readiness bits without source truth {:#x}",
            compat,
        );
    }
    if event.events & EPOLLWAKEUP != 0 {
        // Anemone has no suspend/wakeup-source owner yet. EPOLLWAKEUP is a
        // silent behavioral no-op until that owner and its lifetime contract
        // exist, but retain a notice so the compatibility choice is visible.
        knoticeln!("sys_epoll_ctl: EPOLLWAKEUP accepted as a compatibility no-op");
    }

    let mut interests = PollEvent::empty();
    if event.events & EPOLLIN != 0 {
        interests |= PollEvent::READABLE;
    }
    if event.events & EPOLLOUT != 0 {
        interests |= PollEvent::WRITABLE;
    }
    Ok(WatchPolicy::new(
        interests,
        event.events & EPOLLET != 0,
        event.events & EPOLLONESHOT != 0,
        event.data,
    ))
}

#[syscall(SYS_EPOLL_CTL)]
fn sys_epoll_ctl(
    epfd: Fd,
    op: EpollCtlOp,
    target_fd: Fd,
    event_addr: u64,
) -> Result<u64, SysError> {
    // Linux ignores the event pointer for DEL. ADD/MOD copy the complete
    // byte-layout record before entering fd/epoll state.
    let event = match op {
        EpollCtlOp::Delete => None,
        EpollCtlOp::Add | EpollCtlOp::Modify => {
            let event_addr = (event_addr != 0)
                .then(|| user_addr(event_addr))
                .transpose()?;
            Some(read_event(event_addr)?)
        },
    };

    let task = get_current_task();
    let (_, epoll) = resolve_epoll_fd(&task, epfd)?;
    let target_desc = task.get_fd(target_fd)?;
    if epoll_from_file(target_desc.vfs_file()).is_some() {
        knoticeln!(
            "sys_epoll_ctl: nested/self epoll is not supported epfd={:?} target={:?}",
            epfd,
            target_fd,
        );
        return Err(SysError::InvalidArgument);
    }
    let target = target_desc
        .opened_description_capability()
        .ok_or(SysError::BadFileDescriptor)?;

    match op {
        EpollCtlOp::Add => epoll.ctl_add(target_fd, target, watch_policy(event.unwrap())?)?,
        EpollCtlOp::Modify => epoll.ctl_modify(target_fd, target, watch_policy(event.unwrap())?)?,
        EpollCtlOp::Delete => epoll.ctl_delete(target_fd, &target)?,
    }
    Ok(0)
}
