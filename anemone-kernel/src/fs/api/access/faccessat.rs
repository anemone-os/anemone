use crate::{
    fs::api::{
        access::{
            args::{AccessFlag, AccessMode},
            kernel_faccess,
        },
        args::AtFd,
    },
    prelude::{user_access::c_readonly_string, *},
};

#[syscall(SYS_FACCESSAT)]
fn sys_faccessat(
    dirfd: AtFd,
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] pathname: Box<str>,
    mode: AccessMode,
) -> Result<u64, SysError> {
    knoticeln!(
        "faccessat: dirfd={:?}, pathname={:?}, mode={:?}",
        dirfd,
        pathname,
        mode
    );

    let r = kernel_faccess(dirfd, pathname.as_ref(), mode, AccessFlag::empty()).map(|()| 0);

    kdebugln!("faccessat: r={:?}", r);

    r
}
