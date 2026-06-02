use core::mem::size_of;

use anemone_abi::process::linux::clone::CloneArgs;

use crate::{
    prelude::{
        handler::TryFromSyscallArg,
        user_access::{UserReadSlice, UserWritePtr, user_addr},
        *,
    },
    task::{
        clone::{CloneFlags, CloneFlagsWithSignal, CloneStack, kernel_clone},
        sig::SigNo,
    },
};

const CLONE_ARGS_SIZE_VER0: usize = 64;
const CLONE_ARGS_SIZE_VER1: usize = 80;
const CLONE_ARGS_SIZE_VER2: usize = size_of::<CloneArgs>();
const CSIGNAL_MASK: u64 = 0xff;

#[syscall(SYS_CLONE3, preparse = |uargs, size| {
    kdebugln!("sys_clone3 called with uargs={:#x}, size={}", uargs, size);
})]
pub fn sys_clone3(uargs: u64, size: usize) -> Result<u64, SysError> {
    let args = read_clone_args(uargs, size)?;

    kdebugln!("sys_clone3 parsed args: {:?}", args);

    let exit_signal = parse_exit_signal(args.exit_signal)?;
    let flags = parse_raw_clone3_flags(args.flags)?;
    let new_sp = parse_clone3_stack(args.stack, args.stack_size)?;

    reject_deferred_clone3_features(&args, size, flags)?;

    let flags = clone3_flags_with_signal(flags, exit_signal)?;
    let tls = if flags.flags().contains(CloneFlags::SETTLS) {
        user_addr(args.tls)?
    } else {
        VirtAddr::new(0)
    };
    let parent_tid = if flags.flags().contains(CloneFlags::PARENT_SETTID) {
        optional_user_addr(args.parent_tid)?
    } else {
        None
    };
    let child_tid = if flags
        .flags()
        .intersects(CloneFlags::CHILD_SETTID | CloneFlags::CHILD_CLEARTID)
    {
        optional_user_addr(args.child_tid)?
    } else {
        None
    };

    kernel_clone(flags, *__trapframe__, new_sp, tls, parent_tid, child_tid)
        .map(|tid| tid.get() as u64)
}

fn read_clone_args(uargs: u64, size: usize) -> Result<CloneArgs, SysError> {
    if size > PagingArch::PAGE_SIZE_BYTES {
        return Err(SysError::ArgumentTooLarge);
    }
    if size < CLONE_ARGS_SIZE_VER0 {
        return Err(SysError::InvalidArgument);
    }

    let uargs = user_addr(uargs)?;
    let bytes = get_current_task().clone_uspace_handle().with_usp(|usp| {
        let uslice = UserReadSlice::<u8>::try_new(uargs, size, usp)?;
        let mut bytes = vec![0u8; size];
        uslice.copy_to_slice(&mut bytes);
        Ok::<_, SysError>(bytes)
    })?;

    if size > CLONE_ARGS_SIZE_VER2 && bytes[CLONE_ARGS_SIZE_VER2..].iter().any(|&b| b != 0) {
        return Err(SysError::ArgumentTooLarge);
    }

    let mut known = [0u8; CLONE_ARGS_SIZE_VER2];
    let copied = core::cmp::min(size, CLONE_ARGS_SIZE_VER2);
    known[..copied].copy_from_slice(&bytes[..copied]);

    Ok(CloneArgs {
        flags: clone_arg_field(&known, 0),
        pidfd: clone_arg_field(&known, 1),
        child_tid: clone_arg_field(&known, 2),
        parent_tid: clone_arg_field(&known, 3),
        exit_signal: clone_arg_field(&known, 4),
        stack: clone_arg_field(&known, 5),
        stack_size: clone_arg_field(&known, 6),
        tls: clone_arg_field(&known, 7),
        set_tid: clone_arg_field(&known, 8),
        set_tid_size: clone_arg_field(&known, 9),
        cgroup: clone_arg_field(&known, 10),
    })
}

fn clone_arg_field(bytes: &[u8; CLONE_ARGS_SIZE_VER2], index: usize) -> u64 {
    let start = index * size_of::<u64>();
    let mut raw = [0u8; size_of::<u64>()];
    raw.copy_from_slice(&bytes[start..start + size_of::<u64>()]);
    u64::from_ne_bytes(raw)
}

fn parse_exit_signal(raw: u64) -> Result<Option<SigNo>, SysError> {
    if raw & !CSIGNAL_MASK != 0 {
        return Err(SysError::InvalidArgument);
    }
    if raw == 0 {
        Ok(None)
    } else {
        SigNo::try_from_syscall_arg(raw).map(Some)
    }
}

fn parse_raw_clone3_flags(raw: u64) -> Result<CloneFlags, SysError> {
    if raw & CSIGNAL_MASK != 0 {
        return Err(SysError::InvalidArgument);
    }

    let flags = CloneFlags::from_bits(raw).ok_or(SysError::InvalidArgument)?;

    if flags.contains(CloneFlags::DETACHED) {
        return Err(SysError::InvalidArgument);
    }
    if flags.contains(CloneFlags::FS) && flags.contains(CloneFlags::NEWNS) {
        return Err(SysError::InvalidArgument);
    }

    Ok(flags)
}

fn parse_clone3_stack(stack: u64, stack_size: u64) -> Result<CloneStack, SysError> {
    if stack == 0 {
        if stack_size != 0 {
            return Err(SysError::InvalidArgument);
        }
        return Ok(CloneStack::SameAsParent);
    }
    if stack_size == 0 {
        return Err(SysError::InvalidArgument);
    }

    let end = stack
        .checked_add(stack_size)
        .ok_or(SysError::InvalidArgument)?;
    if stack >= KernelLayout::USPACE_TOP_ADDR || end > KernelLayout::USPACE_TOP_ADDR {
        return Err(SysError::InvalidArgument);
    }

    Ok(CloneStack::New(VirtAddr::new(end)))
}

fn reject_deferred_clone3_features(
    args: &CloneArgs,
    size: usize,
    flags: CloneFlags,
) -> Result<(), SysError> {
    if args.set_tid_size != 0 && args.set_tid == 0 {
        return Err(SysError::InvalidArgument);
    }
    if args.set_tid != 0 && args.set_tid_size == 0 {
        return Err(SysError::InvalidArgument);
    }
    if args.set_tid != 0 || args.set_tid_size != 0 {
        knoticeln!(
            "[NYI] clone3: set_tid/set_tid_size require pid namespace aware TID allocation"
        );
        return Err(SysError::NotYetImplemented);
    }

    if flags.contains(CloneFlags::INTO_CGROUP) {
        if size < CLONE_ARGS_SIZE_VER2 || args.cgroup > i32::MAX as u64 {
            return Err(SysError::InvalidArgument);
        }
        knoticeln!("[NYI] clone3: CLONE_INTO_CGROUP requires cgroup task placement support");
        return Err(SysError::NotYetImplemented);
    }

    if flags.contains(CloneFlags::PIDFD) {
        if flags.contains(CloneFlags::PARENT_SETTID) && args.pidfd == args.parent_tid {
            return Err(SysError::InvalidArgument);
        }
        validate_pidfd_ptr(args.pidfd)?;
        knoticeln!("[NYI] clone3: CLONE_PIDFD requires pidfd file object support");
        return Err(SysError::NotYetImplemented);
    }

    Ok(())
}

fn validate_pidfd_ptr(pidfd: u64) -> Result<(), SysError> {
    let pidfd = user_addr(pidfd)?;
    get_current_task().clone_uspace_handle().with_usp(|usp| {
        UserWritePtr::<i32>::try_new(pidfd, usp)?;
        Ok::<_, SysError>(())
    })
}

fn clone3_flags_with_signal(
    flags: CloneFlags,
    exit_signal: Option<SigNo>,
) -> Result<CloneFlagsWithSignal, SysError> {
    if (flags.contains(CloneFlags::THREAD) || flags.contains(CloneFlags::PARENT))
        && exit_signal.is_some()
    {
        return Err(SysError::InvalidArgument);
    }

    let supported = CloneFlags::SIGHAND
        | CloneFlags::CLEAR_SIGHAND
        | CloneFlags::THREAD
        | CloneFlags::VFORK
        | CloneFlags::VM
        | CloneFlags::FS
        | CloneFlags::SYSVSEM
        | CloneFlags::FILES
        | CloneFlags::PARENT
        | CloneFlags::SETTLS
        | CloneFlags::PARENT_SETTID
        | CloneFlags::CHILD_CLEARTID
        | CloneFlags::CHILD_SETTID;

    if !supported.contains(flags) {
        knoticeln!("nyi clone3 flags: {:?}", flags);
        return Err(SysError::NotYetImplemented);
    }

    flags.validate()?;

    Ok(CloneFlagsWithSignal::new(flags, exit_signal))
}

fn optional_user_addr(raw: u64) -> Result<Option<VirtAddr>, SysError> {
    if raw == 0 {
        Ok(None)
    } else {
        user_addr(raw).map(Some)
    }
}
