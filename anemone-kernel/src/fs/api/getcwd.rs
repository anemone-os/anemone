use crate::prelude::{dt::UserWritePtr, *};

#[syscall(SYS_GETCWD)]
fn sys_getcwd(buf: UserWritePtr<u8>, size: usize) -> Result<u64, SysError> {
    let cwd = with_current_task(|task| task.rel_cwd());
    let cwd_bytes = cwd.as_bytes();
    let slice = buf.slice(size);
    slice.safe_write_bytes_str(cwd_bytes)?;
    Ok(buf.addr())
}
