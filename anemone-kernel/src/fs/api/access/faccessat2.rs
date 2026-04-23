use crate::{
    fs::api::{
        access::{
            args::{AccessFlag, AccessMode},
            kernel_faccess,
        },
        args::AtFd,
    },
    prelude::{dt::c_readonly_string, *},
};

#[syscall(SYS_FACCESSAT2)]
fn sys_faccessat2(
    dirfd: AtFd,
    #[validate_with(c_readonly_string)] pathname: Box<str>,
    mode: AccessMode,
    flags: AccessFlag,
) -> Result<u64, SysError> {
    knoticeln!(
        "faccessat2: dirfd={:?}, pathname={:?}, mode={:?}, flags={:?}",
        dirfd,
        pathname,
        mode,
        flags
    );

    kernel_faccess(dirfd, pathname.as_ref(), mode, flags).map(|()| 0)
}
