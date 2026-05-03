use anemone_abi::syscall::SYS_EXECVE;

use crate::prelude::{
    user_access::{c_readonly_string, c_readonly_string_array},
    *,
};

// arbitrary limit. we should make this a kconfig item later.
const MAX_ARG_BYTES_LEN: usize = MAX_PATH_LEN_BYTES * 2;

// the same as above.
const MAX_ARG_COUNT: usize = 128;

#[syscall(SYS_EXECVE, preparse = |_, _, _| {
    kdebugln!("preparsing execve syscall arguments");
})]
pub fn execve(
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] path: Box<str>,
    #[validate_with(c_readonly_string_array::<MAX_ARG_COUNT, MAX_ARG_BYTES_LEN>)] argv: Vec<
        Box<str>,
    >,
    #[validate_with(c_readonly_string_array::<MAX_ARG_COUNT, MAX_ARG_BYTES_LEN>)] envp: Vec<
        Box<str>,
    >,
) -> Result<u64, SysError> {
    let path = Path::new(path.as_ref());

    kernel_execve(
        &path.to_str().expect("we've already validated path to be a valid C string, whose encoding is a subset of UTF-8"), 
        argv.as_slice(),
        envp.as_slice(),
    )?;
    unreachable!();
}
