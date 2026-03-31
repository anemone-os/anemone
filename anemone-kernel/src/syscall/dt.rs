use core::{ffi::CStr, slice, str};

use crate::{fs::vfs_lookup, prelude::*};

/// Check the pages containing the user pointer for the specified permissions
/// and return a pointer to the user data if valid.
pub fn user_pointer<T: Sized>(rwx_flags: PteFlags, arg: u64) -> Result<*const T, SysError> {
    let flags = PteFlags::USER | rwx_flags;
    with_current_task(|task| -> Result<(), SysError> {
        let Some(memspace) = task.clone_uspace() else {
            return Err(MmError::NotMapped.into());
        };
        let mut table = memspace.page_table_mut();
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

/// Validate that the address is inside user space and return it as a virtual
/// address.
pub fn user_vaddr(arg: u64) -> Result<VirtAddr, SysError> {
    if arg < KernelLayout::USPACE_TOP_ADDR {
        Ok(VirtAddr::new(arg))
    } else {
        Err(MmError::InvalidArgument.into())
    }
}

/// Validate a user C array
pub fn c_readonly_array_ptr<const MAX_LEN: usize, T: Eq>(
    terminator: T,
    include_terminator: bool,
    arg: u64,
) -> Result<*const [T], SysError> {
    if arg % align_of::<T>() as u64 != 0 {
        return Err(SysError::Mm(MmError::NotAligned));
    }
    let st_pointer = arg as *const T;
    user_pointer::<T>(PteFlags::READ, arg)?;
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
            user_pointer::<u8>(PteFlags::READ, next_ed_pointer as u64)?;
            ed_vpn = ed_vpn_new;
        }
        ed_pointer = unsafe { ed_pointer.add(1) };
        len += 1;
        if len > MAX_LEN {
            return Err(SysError::Kernel(KernelError::InvalidArgument));
        }
    }
    if include_terminator {
        Ok(unsafe { slice::from_raw_parts(st_pointer, len + 1) })
    } else {
        Ok(unsafe { slice::from_raw_parts(st_pointer, len) })
    }
}

/// Validate a user C string pointer and return a borrowed raw string slice
/// pointer.
pub fn c_readonly_string_ptr<const MAX_LEN: usize>(arg: u64) -> Result<*const str, SysError> {
    unsafe {
        let ptr = unsafe { &*c_readonly_array_ptr::<MAX_LEN, _>(0u8, true, arg)? };
        let str = CStr::from_ptr(&ptr[0]);
        let str = str
            .to_str()
            .map_err(|_| SysError::Mm(MmError::InvalidArgument))?;
        Ok(str)
    }
}

/// Validate a user C string pointer and copy it into a boxed string.
pub fn c_readonly_string<const MAX_LEN: usize>(arg: u64) -> Result<Box<str>, SysError> {
    let c_str = c_readonly_string_ptr::<MAX_LEN>(arg)?;
    Ok(Box::from(unsafe { &*c_str }))
}

pub fn c_readonly_string_array(arg: u64) -> Result<Vec<Box<str>>, SysError> {
    let array = unsafe { &*c_readonly_array_ptr::<1024, _>(0u64, false, arg)? };
    let mut res = vec![];
    for ptr in array {
        let str = c_readonly_string::<1024>(*ptr)?;
        res.push(str);
    }
    Ok(res)
}

/// Validate a user path string and convert it into a kernel-owned `Path`.
pub fn file_path(arg: u64) -> Result<Box<Path>, SysError> {
    let c_str = c_readonly_string_ptr::<1024>(arg)?;
    let path = Box::from(Path::new(unsafe { &*c_str }));
    Ok(path)
}

/// Validate a user path string and resolve it to an existing file path.
pub fn existing_file(arg: u64) -> Result<PathRef, SysError> {
    let path_ref = file_path(arg)?;
    vfs_lookup(unsafe { path_ref.as_ref() }).map_err(|e| e.into())
}

/// Validate that the argument is non-zero and convert it to `usize`.
fn nonzero(arg: u64) -> Result<usize, SysError> {
    if arg == 0 {
        Err(KernelError::InvalidArgument.into())
    } else {
        Ok(arg as usize)
    }
}

/// Validate that the argument is non-zero and convert it to `i32`.
fn greater_than_zero(arg: u64) -> Result<i32, SysError> {
    if arg == 0 {
        Err(KernelError::InvalidArgument.into())
    } else {
        Ok(arg as i32)
    }
}
