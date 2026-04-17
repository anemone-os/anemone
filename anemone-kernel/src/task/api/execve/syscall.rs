use anemone_abi::syscall::SYS_EXECVE;

use crate::prelude::{
    dt::{c_readonly_string, c_readonly_string_array},
    *,
};

#[syscall(SYS_EXECVE)]
pub fn execve(
    #[validate_with(c_readonly_string)] path: Box<str>,
    #[validate_with(c_readonly_string_array)] argv: Vec<Box<str>>,
    #[validate_with(c_readonly_string_array)] envp: Vec<Box<str>>,
) -> Result<u64, SysError> {
    let path = Path::new(path.as_ref());

    kernel_execve(
        &path.to_str().expect("we've already validated path to be a valid C string, whose encoding is a subset of UTF-8"), 
        argv.as_slice(),
        envp.as_slice(),
    )?;
    unreachable!();
}
