mod epoll_create1;
mod epoll_ctl;
mod epoll_pwait;
mod epoll_pwait2;
mod wait;

use crate::{
    fs::epoll::{Epoll, epoll_from_file},
    prelude::*,
    task::files::{Fd, FileDesc},
};

fn resolve_epoll_fd(task: &Task, fd: Fd) -> Result<(Arc<FileDesc>, Arc<Epoll>), SysError> {
    let file_desc = task.get_fd(fd)?;
    let epoll = epoll_from_file(file_desc.vfs_file()).ok_or(SysError::InvalidArgument)?;
    Ok((file_desc, epoll))
}
