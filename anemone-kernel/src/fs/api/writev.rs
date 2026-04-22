//! writev system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/writev.2.html

use core::{ops::DerefMut, ptr::null_mut};

use anemone_abi::fs::linux::IoVec;

use crate::{
    prelude::{dt::UserReadPtr, *},
    task::files::Fd,
};

#[syscall(SYS_WRITEV)]
fn sys_writev(fd: Fd, iov: UserReadPtr<IoVec>, iovcnt: usize) -> Result<u64, SysError> {
    if iovcnt == 0 {
        return Ok(0);
    }

    let file = with_current_task(|task| task.get_fd(fd).ok_or(SysError::BadFileDescriptor))?;
    let uspace = with_current_task(|task| {
        task.clone_uspace()
            .expect("user task should have a user space")
    });

    let mut iovecs = vec![
        IoVec {
            iov_base: null_mut(),
            iov_len: 0,
        };
        iovcnt
    ];
    iov.slice(iovcnt).safe_read(&mut iovecs)?;

    let mut total = 0u64;

    for iovec in iovecs {
        if iovec.iov_len == 0 {
            continue;
        }

        // in linux kernel, the logical equivalence function (`iov_iter`) will not
        // return error. here we do a minor twist.
        let kbuf = match copy_iovec_to_kernel(&uspace, iovec) {
            Ok(buf) => buf,
            Err(err) if total > 0 => return Ok(total),
            Err(err) => return Err(err),
        };

        match file.write(&kbuf) {
            Ok(written) => {
                total += written as u64;
                if written != kbuf.len() {
                    // refer to https://elixir.bootlin.com/linux/v6.6.32/source/fs/read_write.c#L743 for why we break here.
                    break;
                }
            },
            Err(err) => return Err(err),
        }
    }

    Ok(total)
}

fn copy_iovec_to_kernel(uspace: &UserSpace, iovec: IoVec) -> Result<Vec<u8>, SysError> {
    let base = UserReadPtr::<u8>::from_raw(iovec.iov_base as u64)?;
    let slice = base.slice(iovec.iov_len as usize);

    let mut kbuf = vec![0u8; iovec.iov_len as usize];
    let mut usp = uspace.write();
    let ptr = unsafe { slice.validate_with(usp.deref_mut())? };

    unsafe {
        kbuf[..iovec.iov_len as usize].copy_from_slice(core::slice::from_raw_parts(
            ptr.cast::<u8>(),
            iovec.iov_len as usize,
        ));
    }

    Ok(kbuf)
}
