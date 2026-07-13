//! read & write system calls.

use crate::{
    prelude::*,
    task::files::{Fd, FileDesc},
};

mod request;

pub mod pread64;
pub mod preadv;
pub mod pwrite64;
pub mod pwritev;
pub mod pwritev2;
pub mod read;
pub mod readv;
pub mod write;
pub mod writev;

fn current_file_and_uspace(fd: Fd) -> Result<(Arc<FileDesc>, Arc<UserSpaceHandle>), SysError> {
    let task = get_current_task();
    let file = task.get_fd(fd)?;
    let uspace = task.clone_uspace_handle();

    Ok((file, uspace))
}
