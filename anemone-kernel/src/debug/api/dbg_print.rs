use kernel_macros::syscall;

use crate::prelude::{user_access::c_readonly_string, *};

/// TODO: make this a kconfig item.
const MAX_DBG_PRINT_LEN: usize = 1024;

/// Directly print a message into kernel console.
///
/// Intended to be used for debugging purposes.
#[syscall(SYS_DBG_PRINT)]
fn sys_dbg_print(
    #[validate_with(c_readonly_string::<MAX_DBG_PRINT_LEN>)] val: Box<str>,
) -> Result<u64, SysError> {
    kprint!("{}", val);
    Ok(0)
}
