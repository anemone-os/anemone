//! Syscall argument validation helpers for user-controlled data.
use core::{
    ffi::{CStr, c_char},
    slice, str,
};

use crate::prelude::*;

pub const MAX_USER_STRING_LEN: usize = 4096;
pub const MAX_USER_ARRAY_LEN: usize = 1024;

/// Check the pages containing the user pointer for the specified permissions
/// and return a pointer to the user data if valid.
///
/// The user pointer in `arg` is validated against `rwx_flags` using `table`.
fn validate_user_pointer<T: Sized>(
    rwx_flags: PteFlags,
    table: &mut PageTable,
    arg: u64,
) -> Result<*const T, SysError> {
    let flags = PteFlags::USER | rwx_flags;
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
    Ok(arg as *const T)
}

/// Validate that the address in `arg` is inside user space and return it as a
/// [VirtAddr].
pub fn user_nullable_vaddr(arg: u64) -> Result<VirtAddr, SysError> {
    if arg < KernelLayout::USPACE_TOP_ADDR {
        Ok(VirtAddr::new(arg))
    } else {
        Err(MmError::InvalidArgument.into())
    }
}

/// Validate a user C array and create a copy of it.
///
/// `terminator` marks the end of the array, `include_terminator` controls
/// whether the copied result includes it, and `MAX_LEN` limits the scan.
pub fn c_readonly_array_ptr<const MAX_LEN: usize, T: Eq + Copy>(
    terminator: T,
    include_terminator: bool,
    arg: u64,
) -> Result<Box<[T]>, SysError> {
    if arg % align_of::<T>() as u64 != 0 {
        return Err(SysError::Mm(MmError::NotAligned));
    }
    with_current_task(|t| {
        let memsp = t
            .clone_uspace()
            .expect("user task should have a user space");
        let mut table = memsp.page_table_mut();

        let st_pointer = arg as *const T;
        validate_user_pointer::<T>(PteFlags::READ, &mut *table, arg)?;
        let mut ed_pointer = st_pointer;
        let mut ed_vpn = VirtAddr::new(arg).page_down();
        let mut len = 0;
        while !unsafe { &*ed_pointer }.eq(&terminator) {
            let next_ed_pointer = (ed_pointer as u64).wrapping_add(1);
            if next_ed_pointer <= arg {
                return Err(MmError::InvalidArgument.into());
            }
            let ed_vpn_new = VirtAddr::new(next_ed_pointer).page_down();
            if ed_vpn_new != ed_vpn {
                validate_user_pointer::<u8>(PteFlags::READ, &mut *table, next_ed_pointer as u64)?;
                ed_vpn = ed_vpn_new;
            }
            ed_pointer = unsafe { ed_pointer.add(1) };
            len += 1;
            if len > MAX_LEN {
                return Err(SysError::Kernel(KernelError::InvalidArgument));
            }
        }
        let slice = unsafe {
            slice::from_raw_parts(st_pointer, len + if include_terminator { 1 } else { 0 })
        };
        let mut res = unsafe { Box::new_uninit_slice(slice.len()).assume_init() };
        res.copy_from_slice(slice);
        drop(table);
        Ok(res)
    })
}

/// Validate a user C string pointer and return a copied string slice.
///
/// `MAX_LEN` limits the scan for the terminating byte.
pub fn c_readonly_string(arg: u64) -> Result<Box<str>, SysError> {
    unsafe {
        let ptr = unsafe { &*c_readonly_array_ptr::<MAX_USER_STRING_LEN, _>(0u8, true, arg)? };
        let str = CStr::from_ptr(&ptr[0] as *const u8 as *const c_char);
        let str = str
            .to_str()
            .map_err(|_| SysError::Mm(MmError::InvalidArgument))?;
        Ok(Box::from(str))
    }
}

/// Validate a user pointer to an array of C strings and return copied
/// strings.
pub fn c_readonly_string_array(arg: u64) -> Result<Vec<Box<str>>, SysError> {
    let array = unsafe { &*c_readonly_array_ptr::<MAX_USER_ARRAY_LEN, _>(0u64, false, arg)? };
    let mut res = vec![];
    for ptr in array {
        let str = c_readonly_string(*ptr)?;
        res.push(str);
    }
    Ok(res)
}

/// Validate that `arg` is non-zero and convert it to `usize`.
fn nonzero(arg: u64) -> Result<usize, SysError> {
    if arg == 0 {
        Err(KernelError::InvalidArgument.into())
    } else {
        Ok(arg as usize)
    }
}

/// Validate that `arg` is non-zero and convert it to `i32`.
fn greater_than_zero(arg: u64) -> Result<i32, SysError> {
    if arg == 0 {
        Err(KernelError::InvalidArgument.into())
    } else {
        Ok(arg as i32)
    }
}
