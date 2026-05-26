use crate::{
    fs::api::{args::LinuxInodePerm, fchmod::kernel_fchmod},
    prelude::*,
    task::files::Fd,
};

#[syscall(SYS_FCHMOD)]
fn sys_fchmod(fd: Fd, linux_perm: LinuxInodePerm) -> Result<u64, SysError> {
    knoticeln!("fchmod: fd={:?}, perm={:#o}", fd, linux_perm.bits());

    let task = get_current_task();
    let file_desc = task.get_fd(fd)?;
    let pathref = file_desc.vfs_file().path().clone();
    let ctime = Instant::now().to_duration();
    let perm = InodePerm::try_from(linux_perm)?;

    let r = kernel_fchmod(&pathref, perm, ctime).map(|()| 0);

    kdebugln!("fchmod: r={:?}", r);
    r
}
