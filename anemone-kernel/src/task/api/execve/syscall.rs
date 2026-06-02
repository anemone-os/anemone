use anemone_abi::syscall::SYS_EXECVE;

use crate::prelude::{
    user_access::{c_readonly_string, c_readonly_string_array},
    *,
};

// arbitrary limit. we should make this a kconfig item later.
const MAX_ARG_BYTES_LEN: usize = MAX_PATH_LEN_BYTES * 2;

// the same as above.
const MAX_ARG_COUNT: usize = 128;

fn nullable_c_readonly_string_array<
    const MAX_ARRAY_LEN: usize,
    const MAX_BYTES_EACH_STRING: usize,
>(
    arg: u64,
) -> Result<Vec<Box<str>>, SysError> {
    if arg == 0 {
        return Ok(Vec::new());
    }

    c_readonly_string_array::<MAX_ARRAY_LEN, MAX_BYTES_EACH_STRING>(arg)
}

#[syscall(SYS_EXECVE, preparse = |_, _, _| {
    kdebugln!("preparsing execve syscall arguments");
})]
pub fn execve(
    #[validate_with(c_readonly_string::<MAX_PATH_LEN_BYTES>)] path: Box<str>,
    #[validate_with(nullable_c_readonly_string_array::<MAX_ARG_COUNT, MAX_ARG_BYTES_LEN>)] argv: Vec<
        Box<str>,
    >,
    #[validate_with(nullable_c_readonly_string_array::<MAX_ARG_COUNT, MAX_ARG_BYTES_LEN>)] envp: Vec<
        Box<str>,
    >,
) -> Result<u64, SysError> {
    let path = Path::new(path.as_ref());
    let argv = if argv.is_empty() {
        vec![Box::<str>::from("")]
    } else {
        argv
    };

    kernel_execve(
        &path.to_str().expect("we've already validated path to be a valid C string, whose encoding is a subset of UTF-8"), 
        argv.as_slice(),
        envp.as_slice(),
    )?;
    unreachable!();
}
