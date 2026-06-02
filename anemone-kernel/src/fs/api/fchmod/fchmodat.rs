use crate::{
    fs::api::{
        args::{AtFd, LinuxInodePerm},
        fchmod::kernel_fchmod,
    },
    prelude::{user_access::c_readonly_path, *},
};

#[syscall(SYS_FCHMODAT)]
fn sys_fchmodat(
    dirfd: AtFd,
    #[validate_with(c_readonly_path)] pathname: Box<str>,
    linux_perm: LinuxInodePerm,
) -> Result<u64, SysError> {
    knoticeln!(
        "fchmodat: dirfd={:?}, pathname={:?}, perm={:#o}",
        dirfd,
        pathname,
        linux_perm.bits(),
    );

    let path = Path::new(pathname.as_ref());

    let task = get_current_task();
    let pathref = if path.is_absolute() {
        task.lookup_path(path, ResolveFlags::empty())?
    } else {
        let dir_path = dirfd.to_pathref(true)?;
        task.lookup_path_from(&dir_path, path, ResolveFlags::empty())?
    };

    let perm = InodePerm::try_from(linux_perm)?;
    let ctime = Instant::now().to_duration();

    let r = kernel_fchmod(&pathref, perm, ctime).map(|()| 0);

    kdebugln!("fchmodat: r={:?}", r);
    r
}
