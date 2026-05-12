use anemone_abi::fs::linux::stat::Stat;

use crate::{
    fs::api::{
        args::AtFd,
        stat::{args::StatAtFlag, kernel_fstatat},
    },
    prelude::{
        user_access::{UserWritePtr, c_readonly_string, user_addr},
        *,
    },
};

#[syscall(SYS_NEWFSTATAT)]
fn sys_newfstatat(
    dirfd: AtFd,
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] filename: Box<str>,
    #[validate_with(user_addr)] statbuf: VirtAddr,
    flags: StatAtFlag,
) -> Result<u64, SysError> {
    let mut kbuf = Stat::default();

    kernel_fstatat(dirfd, &filename, &mut kbuf, flags)?;

    let usp = get_current_task().clone_uspace_handle();
    let mut guard = usp.lock();

    let mut statbuf = UserWritePtr::<Stat>::try_new(statbuf, &mut guard)?;
    statbuf.write(kbuf);

    Ok(0)
}
