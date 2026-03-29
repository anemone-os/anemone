use core::{slice, str};

use crate::prelude::*;

/// Check the pages containing the user pointer for the specified permissions
/// and return a pointer to the user data if valid.
pub fn user_pointer<T: Sized>(rwx_flags: PteFlags, arg: u64) -> Result<*const T, SysError> {
    let flags = PteFlags::USER | rwx_flags;
    with_current_task(|task| -> Result<(), SysError> {
        let Some(memspace) = task.memspace() else {
            return Err(MmError::NotMapped.into());
        };
        let mut table = memspace.table_locked().write();
        let va = VirtAddr::new(arg);
        let va_end = VirtAddr::new(arg.wrapping_add(size_of::<T>() as u64));
        if va_end < va {
            return Err(MmError::InvalidArgument.into());
        }
        let vpn_va = va.page_down();
        let vpn_va_end = va_end.page_up();
        for vpn in vpn_va.get()..vpn_va_end.get() {
            let Some(va_t) = table.mapper().translate(VirtPageNum::new(vpn)) else {
                return Err(MmError::NotMapped.into());
            };
            if !va_t.flags.contains(flags) {
                return Err(MmError::PermissionDenied.into());
            }
        }
        Ok(())
    })?;
    Ok(arg as *const T)
}

pub fn c_string<const MAX_LEN: usize>(arg: u64) -> Result<*const str, SysError> {
    let st_pointer = arg as *const u8;
    user_pointer::<u8>(PteFlags::READ, arg)?;
    let mut ed_pointer = st_pointer;
    let mut ed_vpn = VirtAddr::new(arg).page_down();
    while unsafe { *ed_pointer } != 0 {
        let next_ed_pointer = (ed_pointer as u64).wrapping_add(1);
        if next_ed_pointer <= arg {
            return Err(MmError::InvalidArgument.into());
        }
        let ed_vpn_new = VirtAddr::new(next_ed_pointer).page_down();
        if ed_vpn_new != ed_vpn {
            user_pointer::<u8>(PteFlags::READ, next_ed_pointer as u64)?;
            ed_vpn = ed_vpn_new;
        }
        ed_pointer = unsafe { ed_pointer.add(1) };
    }
    let str = unsafe {
        str::from_utf8(slice::from_raw_parts(
            st_pointer,
            ed_pointer.offset_from(st_pointer) as usize,
        ))
    }
    .unwrap();
    Ok(str as *const str)
}
