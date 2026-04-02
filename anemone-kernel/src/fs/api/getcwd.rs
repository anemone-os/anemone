use core::ffi::c_char;

use crate::prelude::{dt::UserWritePtr, *};

#[syscall(SYS_GETCWD)]
fn sys_getcwd(buf: UserWritePtr<c_char>, size: usize) -> Result<u64, SysError> {
    with_current_task(|task| {
        let cwd = task.rel_cwd();

        let cwd_bytes = cwd.as_bytes();
        if cwd_bytes.len() + 1 > size {
            return Err(KernelError::BufferTooSmall.into());
        }

        let mut slice = unsafe { buf.slice(size, task)? };
        unsafe {
            slice.copy_from(cwd_bytes)?;
            slice.as_mut_ptr().add(cwd_bytes.len()).write(0);
        }

        Ok(cwd_bytes.len() as u64 + 1)
    })
}
