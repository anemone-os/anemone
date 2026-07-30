use super::*;
use sys::anemone::debug;

pub fn dbg_print(msg: &str) -> Result<(), Errno> {
    let cstr = CString::new(msg).map_err(|_| EINVAL)?;
    debug::dbg_print(cstr.as_ptr() as _)
}
