use crate::{
    fs::api::fchown::{group_from_syscall, kernel_fchown, owner_from_syscall},
    prelude::*,
    task::files::Fd,
};

#[syscall(SYS_FCHOWN)]
fn sys_fchown(fd: Fd, owner: Uid, group: Gid) -> Result<u64, SysError> {
    knoticeln!("fchown: fd={:?}, owner={}, group={}", fd, owner, group);

    let task = get_current_task();
    let file_desc = task.get_fd(fd)?;
    if file_desc.is_path_only() {
        return Err(SysError::BadFileDescriptor);
    }
    let pathref = file_desc.vfs_file().path().clone();
    let ctime = Instant::now().to_duration();

    let r = kernel_fchown(
        &pathref,
        owner_from_syscall(owner),
        group_from_syscall(group),
        ctime,
    )
    .map(|()| 0);

    kdebugln!("fchown: r={:?}", r);
    r
}
