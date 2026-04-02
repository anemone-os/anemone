use kernel_macros::syscall;

use crate::prelude::{dt::c_readonly_string, *};

/// Directly print a message into kernel console.
///
/// Intended to be used for debugging purposes.
#[syscall(anemone_abi::syscall::SYS_DBG_PRINT)]
fn sys_dbg_print(#[validate_with(c_readonly_string)] val: Box<str>) -> Result<u64, SysError> {
    kprint!("{}", val);
    Ok(0)
}
