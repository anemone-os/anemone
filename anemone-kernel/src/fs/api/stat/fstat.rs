use anemone_abi::fs::linux::stat::Stat;

use crate::{
    fs::api::{
        args::AtFd,
        stat::{args::StatAtFlag, kernel_fstatat},
    },
    prelude::{dt::UserWritePtr, *},
    task::files::Fd,
};

#[syscall(SYS_FSTAT)]
fn sys_fstat(fd: Fd, statbuf: UserWritePtr<Stat>) -> Result<u64, SysError> {
    let mut kbuf = Stat::default();

    kernel_fstatat(AtFd::Fd(fd), "", &mut kbuf, StatAtFlag::EMPTY_PATH)?;

    statbuf.safe_write(kbuf)?;

    Ok(0)
}
