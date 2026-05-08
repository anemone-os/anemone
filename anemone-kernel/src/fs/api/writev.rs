//! writev system call.
//!
//! Reference:
//! - https://www.man7.org/linux/man-pages/man2/writev.2.html

use core::ptr::null_mut;

use anemone_abi::fs::linux::IoVec;

use crate::{
    prelude::{
        user_access::{UserReadSlice, user_addr},
        *,
    },
    task::files::Fd,
};

// TODO: make this a kconfig item.
const MAX_IOVEC_CNT: usize = 1024;

#[syscall(SYS_WRITEV)]
fn sys_writev(
    fd: Fd,
    #[validate_with(user_addr)] iov: VirtAddr,
    iovcnt: usize,
) -> Result<u64, SysError> {
    if iovcnt == 0 {
        return Ok(0);
    }
    if iovcnt > MAX_IOVEC_CNT {
        return Err(SysError::InvalidArgument);
    }

    let (file, uspace) = {
        let task = get_current_task();
        let file = task.get_fd(fd).ok_or(SysError::BadFileDescriptor)?;
        let uspace = task.clone_uspace();

        (file, uspace)
    };

    let mut iovecs = vec![
        IoVec {
            iov_base: null_mut(),
            iov_len: 0,
        };
        iovcnt
    ];
    {
        let mut guard = uspace.write();
        let ptr_slice = UserReadSlice::try_new(iov, iovcnt, &mut guard)?;
        ptr_slice.copy_to_slice(&mut iovecs);
    }

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
    let base_addr = user_addr(iovec.iov_base as u64)?;

    let mut guard = uspace.write();
    let slice = UserReadSlice::try_new(base_addr, iovec.iov_len as usize, &mut guard)?;

    let mut kbuf = vec![0u8; iovec.iov_len as usize];
    slice.copy_to_slice(&mut kbuf);

    Ok(kbuf)
}
