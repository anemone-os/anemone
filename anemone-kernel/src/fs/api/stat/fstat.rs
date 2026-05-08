use anemone_abi::fs::linux::stat::Stat;

use crate::{
    fs::api::{
        args::AtFd,
        stat::{args::StatAtFlag, kernel_fstatat},
    },
    prelude::{
        user_access::{UserWritePtr, user_addr},
        *,
    },
    task::files::Fd,
};

#[syscall(SYS_FSTAT)]
fn sys_fstat(fd: Fd, #[validate_with(user_addr)] statbuf: VirtAddr) -> Result<u64, SysError> {
    let mut kbuf = Stat::default();

    kernel_fstatat(AtFd::Fd(fd), "", &mut kbuf, StatAtFlag::EMPTY_PATH)?;

    let usp = get_current_task().clone_uspace();
    let mut guard = usp.write();

    let mut statbuf = UserWritePtr::<Stat>::try_new(statbuf, &mut guard)?;
    statbuf.write(kbuf);

    Ok(0)
}
