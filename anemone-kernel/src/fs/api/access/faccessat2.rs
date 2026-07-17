use crate::{
    fs::api::{
        access::{
            args::{AccessFlag, AccessMode},
            kernel_faccess,
        },
        args::RawAtFd,
    },
    prelude::{user_access::c_readonly_path, *},
};

#[syscall(SYS_FACCESSAT2)]
fn sys_faccessat2(
    dirfd: RawAtFd,
    #[validate_with(c_readonly_path)] pathname: Box<str>,
    mode: AccessMode,
    flags: AccessFlag,
) -> Result<u64, SysError> {
    kdebugln!(
        "faccessat2: dirfd={:?}, pathname={:?}, mode={:?}, flags={:?}",
        dirfd,
        pathname,
        mode,
        flags
    );

    kernel_faccess(dirfd, pathname.as_ref(), mode, flags).map(|()| 0)
}
