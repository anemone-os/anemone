use alloc::ffi::CString;

use crate::{prelude::*, sys};

pub mod debug {
    use super::*;
    use sys::anemone::debug;

    pub fn dbg_print(msg: &str) -> Result<(), Errno> {
        let cstr = CString::new(msg).map_err(|_| EINVAL)?;
        debug::dbg_print(cstr.as_ptr() as _)
    }
}

pub mod power {
    use super::*;
    use sys::anemone::power;

    pub fn shutdown(magic: u64) -> Result<(), Errno> {
        power::shutdown(magic)
    }
}
