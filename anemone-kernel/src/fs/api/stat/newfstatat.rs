use anemone_abi::fs::linux::stat::Stat;

use crate::{
    fs::api::{
        args::AtFd,
        stat::{args::StatAtFlag, kernel_fstatat},
    },
    prelude::{
        dt::{UserWritePtr, c_readonly_string},
        *,
    },
};

#[syscall(SYS_NEWFSTATAT)]
fn sys_newfstatat(
    dirfd: AtFd,
    #[validate_with(c_readonly_string)] filename: Box<str>,
    statbuf: UserWritePtr<Stat>,
    flags: StatAtFlag,
) -> Result<u64, SysError> {
    let mut kbuf = Stat::default();

    kernel_fstatat(dirfd, &filename, &mut kbuf, flags)?;

    statbuf.safe_write(kbuf)?;

    Ok(0)
}
