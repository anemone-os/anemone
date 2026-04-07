//! Syscall argument validation helpers for user-controlled data.
use core::{
    ffi::{CStr, c_char},
    marker::PhantomData,
    ops::{Deref, DerefMut},
    slice, str,
};

use crate::{prelude::*, syscall::handler::TryFromSyscallArg};

pub const MAX_USER_STRING_LEN: usize = 4096;
pub const MAX_USER_ARRAY_LEN: usize = 1024;

pub trait UserAccess: Copy {
    const PTE_FLAGS: PteFlags;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserRead;

impl UserAccess for UserRead {
    const PTE_FLAGS: PteFlags = PteFlags::READ;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserWrite;

impl UserAccess for UserWrite {
    const PTE_FLAGS: PteFlags = PteFlags::WRITE;
}

#[derive(Debug, PartialEq, Eq)]
pub struct UserPtr<T: Sized, A: UserAccess> {
    addr: u64,
    _marker: PhantomData<(A, T)>,
}

pub type UserReadPtr<T> = UserPtr<T, UserRead>;
pub type UserWritePtr<T> = UserPtr<T, UserWrite>;

impl<T: Sized, A: UserAccess> Copy for UserPtr<T, A> {}

impl<T: Sized, A: UserAccess> Clone for UserPtr<T, A> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Sized, A: UserAccess> UserPtr<T, A> {
    pub fn from_raw(arg: u64) -> Result<Self, SysError> {
        if arg % align_of::<T>() as u64 != 0 {
            return Err(SysError::Mm(MmError::NotAligned));
        }
        if arg >= KernelLayout::USPACE_TOP_ADDR
            || arg.wrapping_add(size_of::<T>() as u64) > KernelLayout::USPACE_TOP_ADDR
        {
            return Err(SysError::Mm(MmError::InvalidArgument));
        }
        Ok(Self {
            addr: arg,
            _marker: PhantomData,
        })
    }

    pub fn addr(self) -> u64 {
        self.addr
    }
}
impl<T: Sized + Clone, A: UserAccess> UserPtr<T, A> {
    pub fn safe_read(&self) -> Result<T, SysError> {
        let usp =
            with_current_task(|t| t.clone_uspace()).expect("user task should have a user space");
        let usp_data: ReadNoPreemptGuard<'_, UserSpaceData> = usp.read();
        let ptr: *const T = validate_user_pointer(A::PTE_FLAGS, usp_data.deref(), self.addr)?;
        let res = unsafe { (*ptr).clone() };
        drop(usp_data);
        Ok(res)
    }
    pub fn validate_with(&self, data: &UserSpaceData) -> Result<*const T, SysError> {
        let ptr = validate_user_pointer(UserWrite::PTE_FLAGS, data, self.addr)? as *const T;
        Ok(ptr)
    }
}

impl<T: Sized> UserWritePtr<T> {
    pub fn safe_write(&self, value: T) -> Result<(), SysError> {
        let usp =
            with_current_task(|t| t.clone_uspace()).expect("user task should have a user space");
        let mut usp_data = usp.write();
        let ptr =
            validate_user_pointer_for_write(UserWrite::PTE_FLAGS, usp_data.deref_mut(), self.addr)?
                as *const T as *mut T;
        unsafe { *ptr = value };
        drop(usp_data);
        Ok(())
    }

    pub fn validate_with_mut(&self, data: &mut UserSpaceData) -> Result<*mut T, SysError> {
        let ptr = validate_user_pointer_for_write(UserWrite::PTE_FLAGS, data, self.addr)?
            as *const T as *mut T;
        Ok(ptr)
    }
}

impl<T: Sized + Clone, A: UserAccess> TryFromSyscallArg for UserPtr<T, A> {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        Self::from_raw(raw)
    }
}

impl<T: Sized + Copy, A: UserAccess> UserPtr<T, A> {
    pub fn slice(&self, len: usize) -> UserSlice<T, A> {
        UserSlice { ptr: *self, len }
    }
}

pub struct UserSlice<T, A: UserAccess> {
    ptr: UserPtr<T, A>,
    len: usize,
}

pub type UserReadSlice<T> = UserSlice<T, UserRead>;
pub type UserWriteSlice<T> = UserSlice<T, UserWrite>;

impl<T: Sized, A: UserAccess> UserSlice<T, A> {
    pub fn addr(&self) -> u64 {
        self.ptr.addr()
    }
    pub fn len(&self) -> usize {
        self.len
    }
}
impl<T: Sized + Copy, A: UserAccess> UserSlice<T, A> {
    pub fn safe_read(&self, buf: &mut [T]) -> Result<(), SysError> {
        if buf.len() < self.len {
            return Err(SysError::Kernel(KernelError::BufferTooSmall));
        }
        let usp =
            with_current_task(|t| t.clone_uspace()).expect("user task should have a user space");
        let usp_data: ReadNoPreemptGuard<'_, UserSpaceData> = usp.read();
        let ptr: *const [T] =
            validate_user_array(A::PTE_FLAGS, usp_data.deref(), self.ptr.addr(), self.len)?;
        buf[0..self.len].copy_from_slice(unsafe { &*ptr });
        drop(usp_data);
        Ok(())
    }
    pub fn validate_with(&self, data: &UserSpaceData) -> Result<*const [T], SysError> {
        let ptr = validate_user_array(A::PTE_FLAGS, data, self.ptr.addr, self.len)? as *const [T]
            as *mut [T];
        Ok(ptr)
    }
}

impl<T: Sized + Copy> UserWriteSlice<T> {
    pub fn safe_write(&self, buf: &[T]) -> Result<(), SysError> {
        let usp =
            with_current_task(|t| t.clone_uspace()).expect("user task should have a user space");
        self.safe_write_with(buf, &usp)
    }
    pub fn safe_write_with(&self, buf: &[T], usp: &UserSpace) -> Result<(), SysError> {
        if buf.len() < self.len {
            return Err(SysError::Kernel(KernelError::BufferTooSmall));
        }
        let mut usp_data = usp.write();
        let ptr = validate_user_array_for_write(
            UserWrite::PTE_FLAGS,
            usp_data.deref_mut(),
            self.ptr.addr,
            self.len,
        )? as *const [T] as *mut [T];

        unsafe {
            (*ptr).copy_from_slice(&buf[0..self.len]);
        }
        drop(usp_data);
        Ok(())
    }

    pub fn validate_with_mut(&self, data: &mut UserSpaceData) -> Result<*mut [T], SysError> {
        let ptr =
            validate_user_array_for_write(UserWrite::PTE_FLAGS, data, self.ptr.addr, self.len)?
                as *const [T] as *mut [T];
        Ok(ptr)
    }

    pub fn safe_write_str(&self, s: &str) -> Result<(), SysError> {
        let bytes = s.as_bytes();
        if bytes.len() + 1 > self.len {
            return Err(SysError::Kernel(KernelError::BufferTooSmall));
        }
        let usp =
            with_current_task(|t| t.clone_uspace()).expect("user task should have a user space");
        let mut usp_data = usp.write();
        let ptr = validate_user_array_for_write(
            UserWrite::PTE_FLAGS,
            usp_data.deref_mut(),
            self.ptr.addr,
            self.len,
        )? as *const [u8] as *mut [u8];
        unsafe {
            let mut ptr_ref = &mut *ptr;
            ptr_ref[0..bytes.len()].copy_from_slice(bytes);
            ptr_ref[bytes.len()] = 0;
        }
        drop(usp_data);
        Ok(())
    }
    pub fn safe_write_bytes_str(&self, bytes: &[u8]) -> Result<(), SysError> {
        if bytes.len() + 1 > self.len {
            return Err(SysError::Kernel(KernelError::BufferTooSmall));
        }
        let usp =
            with_current_task(|t| t.clone_uspace()).expect("user task should have a user space");
        let mut usp_data = usp.write();
        let ptr = validate_user_array_for_write(
            UserWrite::PTE_FLAGS,
            usp_data.deref_mut(),
            self.ptr.addr,
            self.len,
        )? as *const [u8] as *mut [u8];
        unsafe {
            let mut ptr_ref = &mut *ptr;
            ptr_ref[0..bytes.len()].copy_from_slice(bytes);
            ptr_ref[bytes.len()] = 0;
        }
        drop(usp_data);
        Ok(())
    }
}

/// Check the pages containing the user pointer for the specified permissions
/// and return a pointer to the user data if valid.
///
/// The user pointer in `arg` is validated against `rwx_flags` using `table`.
fn validate_user_pointer<T: Sized>(
    rwx_flags: PteFlags,
    usp: &UserSpaceData,
    arg: u64,
) -> Result<*const T, SysError> {
    let flags = PteFlags::USER | rwx_flags;
    // kdebugln!("validating user pointer {:#x}: {:?}", arg, flags);
    let va = VirtAddr::new(arg);
    let va_end = VirtAddr::new(arg.wrapping_add(size_of::<T>() as u64));
    if va_end < va {
        return Err(MmError::InvalidArgument.into());
    }
    let vpn_va = va.page_down();
    let vpn_va_end = va_end.page_up();
    for vpn in VirtPageRange::new(vpn_va, vpn_va_end - vpn_va).iter() {
        usp.check_permission(vpn, rwx_flags)?;
    }
    Ok(arg as *const T)
}
fn validate_user_pointer_for_write<T: Sized>(
    rwx_flags: PteFlags,
    usp: &mut UserSpaceData,
    arg: u64,
) -> Result<*const T, SysError> {
    let flags = PteFlags::USER | rwx_flags;
    // kdebugln!("validating user pointer {:#x}: {:?}", arg, flags);
    let va = VirtAddr::new(arg);
    let va_end = VirtAddr::new(arg.wrapping_add(size_of::<T>() as u64));
    if va_end < va {
        return Err(MmError::InvalidArgument.into());
    }
    let vpn_va = va.page_down();
    let vpn_va_end = va_end.page_up();
    for vpn in VirtPageRange::new(vpn_va, vpn_va_end - vpn_va).iter() {
        usp.check_permission(vpn, rwx_flags)?;
    }
    for vpn in VirtPageRange::new(vpn_va, vpn_va_end - vpn_va).iter() {
        usp.copy_on_write(vpn.to_virt_addr())?;
    }
    Ok(arg as *const T)
}

fn validate_user_array<T: Sized>(
    rwx_flags: PteFlags,
    usp: &UserSpaceData,
    arg: u64,
    len: usize,
) -> Result<*const [T], SysError> {
    let flags = PteFlags::USER | rwx_flags;
    /*kdebugln!(
        "validating user array pointer {:#x} with length {}: {:?}",
        arg,
        len,
        flags
    );*/
    if arg % align_of::<T>() as u64 != 0 {
        return Err(MmError::NotAligned.into());
    }

    let va = VirtAddr::new(arg);
    let va_end = VirtAddr::new(arg.wrapping_add((size_of::<T>() * len) as u64));
    if va_end < va {
        return Err(MmError::InvalidArgument.into());
    }

    let vpn_va = va.page_down();
    let vpn_va_end = va_end.page_up();
    for vpn in VirtPageRange::new(vpn_va, vpn_va_end - vpn_va).iter() {
        usp.check_permission(vpn, rwx_flags)?;
    }

    Ok(core::ptr::slice_from_raw_parts(arg as *const T, len))
}

fn validate_user_array_for_write<T: Sized>(
    rwx_flags: PteFlags,
    usp: &mut UserSpaceData,
    arg: u64,
    len: usize,
) -> Result<*const [T], SysError> {
    let flags = PteFlags::USER | rwx_flags;
    /*kdebugln!(
        "validating user array pointer {:#x} with length {}: {:?}",
        arg,
        len,
        flags
    );*/
    if arg % align_of::<T>() as u64 != 0 {
        return Err(MmError::NotAligned.into());
    }

    let va = VirtAddr::new(arg);
    let va_end = VirtAddr::new(arg.wrapping_add((size_of::<T>() * len) as u64));
    if va_end < va {
        return Err(MmError::InvalidArgument.into());
    }

    let vpn_va = va.page_down();
    let vpn_va_end = va_end.page_up();
    for vpn in VirtPageRange::new(vpn_va, vpn_va_end - vpn_va).iter() {
        usp.check_permission(vpn, rwx_flags)?;
    }
    if rwx_flags.contains(PteFlags::WRITE) {
        for vpn in VirtPageRange::new(vpn_va, vpn_va_end - vpn_va).iter() {
            usp.copy_on_write(vpn.to_virt_addr())?;
        }
    }

    Ok(core::ptr::slice_from_raw_parts(arg as *const T, len))
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
        let usp = t
            .clone_uspace()
            .expect("user task should have a user space");
        let usp_data: ReadNoPreemptGuard<'_, UserSpaceData> = usp.read();
        let st_pointer = arg as *const T;
        validate_user_pointer::<T>(PteFlags::READ, usp_data.deref(), arg)?;
        let mut ed_pointer = st_pointer;
        let mut ed_vpn = VirtAddr::new(arg).page_down();
        let mut len = 0;
        while !unsafe { &*ed_pointer }.eq(&terminator) {
            let next_ed_pointer = (ed_pointer as u64).wrapping_add(size_of::<T>() as u64);
            if next_ed_pointer <= arg {
                return Err(MmError::InvalidArgument.into());
            }
            let ed_vpn_new = VirtAddr::new(next_ed_pointer).page_down();
            if ed_vpn_new != ed_vpn {
                validate_user_pointer::<u8>(
                    PteFlags::READ,
                    usp_data.deref(),
                    next_ed_pointer as u64,
                )?;
                ed_vpn = ed_vpn_new;
            }
            ed_pointer = next_ed_pointer as *const T;
            len += 1;
            if len > MAX_LEN {
                return Err(SysError::Kernel(KernelError::InvalidArgument));
            }
        }
        let slice = unsafe {
            slice::from_raw_parts(st_pointer, len + if include_terminator { 1 } else { 0 })
        };
        let res: Box<[T]> = slice.into();
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

/// Interpret the argument as a signed integer and validate that it is greater
/// than zero.
pub fn greater_than_zero(arg: u64) -> Result<u64, SysError> {
    let arg = arg as i64;
    if arg > 0 {
        Ok(arg as u64)
    } else {
        Err(SysError::Kernel(KernelError::InvalidArgument))
    }
}

/// Interpret the argument as an unsigned integer and validate that it is
/// nonzero.
pub fn nonzero(arg: u64) -> Result<u64, SysError> {
    if arg != 0 {
        Ok(arg)
    } else {
        Err(SysError::Kernel(KernelError::InvalidArgument))
    }
}
