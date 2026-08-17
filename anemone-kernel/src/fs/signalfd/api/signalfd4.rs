use core::mem::size_of;

use anemone_abi::{
    fs::linux::signalfd::{SFD_CLOEXEC, SFD_NONBLOCK},
    process::linux::signal as linux_signal,
    syscall::SYS_SIGNALFD4,
};

use crate::{
    prelude::{
        user_access::{UserReadPtr, user_addr},
        *,
    },
    task::{
        files::{Fd, FdFlags, FileStatusFlags, LinuxOpenCompat, OpenAccessMode},
        sig::set::SigSet,
    },
};

use super::super::{create_signalfd, description_ops, reconfigure_signalfd, sanitize_mask};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct SignalFdFlags: u32 {
        const CLOEXEC = SFD_CLOEXEC;
        const NONBLOCK = SFD_NONBLOCK;
    }
}

#[syscall(SYS_SIGNALFD4)]
fn sys_signalfd4(
    fd: i32,
    #[validate_with(user_addr)] mask_addr: VirtAddr,
    sigsetsize: usize,
    raw_flags: i32,
) -> Result<u64, SysError> {
    if sigsetsize != size_of::<linux_signal::SigSet>() {
        return Err(SysError::InvalidArgument);
    }

    // Linux observes the mask copy before validating flags. Preserve that
    // ordering so a bad pointer wins over an invalid flag combination.
    let task = get_current_task();
    let uspace = task.clone_uspace_handle();
    let mask = sanitize_mask({
        let mut guard = uspace.lock();
        SigSet::new_with_mask(
            UserReadPtr::<linux_signal::SigSet>::try_new(mask_addr, &mut guard)?
                .read()?
                .bits,
        )
    });

    let raw_flags = u32::try_from(raw_flags).map_err(|_| SysError::InvalidArgument)?;
    let flags = SignalFdFlags::from_bits(raw_flags).ok_or(SysError::InvalidArgument)?;

    if fd != -1 {
        if fd < -1 {
            return Err(SysError::BadFileDescriptor);
        }
        let fd = Fd::new(fd as u32).ok_or(SysError::BadFileDescriptor)?;
        let description = task.get_fd(fd)?;
        reconfigure_signalfd(description.vfs_file(), mask)?;

        // Reconfiguration changes only the opened-description mask. Its
        // capability routes wake readers/watchers in every caller group that
        // registered this shared description; status/fd flags remain untouched.
        return Ok(fd.raw() as u64);
    }

    let file = create_signalfd(mask)?;
    let mut status_flags = FileStatusFlags::empty();
    status_flags.set(
        FileStatusFlags::NONBLOCK,
        flags.contains(SignalFdFlags::NONBLOCK),
    );
    file.check_status_flags(status_flags.to_file_op_status_flags())?;
    let fd_flags = if flags.contains(SignalFdFlags::CLOEXEC) {
        FdFlags::CLOSE_ON_EXEC
    } else {
        FdFlags::empty()
    };

    let fd = task.open_fd_with_description_ops(
        file,
        OpenAccessMode::ReadWrite,
        status_flags,
        LinuxOpenCompat::empty(),
        fd_flags,
        description_ops(),
    )?;
    Ok(fd.raw() as u64)
}
