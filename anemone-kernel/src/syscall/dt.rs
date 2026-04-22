//! Syscall argument validation helpers for user-controlled data.

use core::{
    ffi::{CStr, c_char},
    marker::PhantomData,
    ops::DerefMut,
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
            return Err(SysError::NotAligned);
        }
        if arg >= KernelLayout::USPACE_TOP_ADDR
            || arg.wrapping_add(size_of::<T>() as u64) > KernelLayout::USPACE_TOP_ADDR
        {
            return Err(SysError::InvalidArgument);
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

impl<T: Sized + Copy, A: UserAccess> UserPtr<T, A> {
    /// This does not guarantee the validity of the pointer, which will be
    /// lazily checked when memory is accessed.
    pub fn slice(self, len: usize) -> UserSlice<T, A> {
        UserSlice { ptr: self, len }
    }
}

impl<T: Sized + Clone, A: UserAccess> UserPtr<T, A> {
    pub fn safe_read(&self) -> Result<T, SysError> {
        let usp =
            with_current_task(|t| t.clone_uspace()).expect("user task should have a user space");
        let mut usp_data = usp.write();
        let ptr: *const T = validate_user_pointer(A::PTE_FLAGS, usp_data.deref_mut(), self.addr)?;
        let res = unsafe { (*ptr).clone() };
        drop(usp_data);
        Ok(res)
    }

    pub fn validate_with(&self, data: &mut UserSpaceData) -> Result<*const T, SysError> {
        let ptr = validate_user_pointer(A::PTE_FLAGS, data, self.addr)?;
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

    pub fn validate_mut_with(&self, data: &mut UserSpaceData) -> Result<*mut T, SysError> {
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

impl<T: Sized, A: UserAccess> TryFromSyscallArg for Option<UserPtr<T, A>> {
    fn try_from_syscall_arg(raw: u64) -> Result<Self, SysError> {
        if raw == 0 {
            Ok(None)
        } else {
            UserPtr::from_raw(raw).map(Some)
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct UserSlice<T: Sized, A: UserAccess> {
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
        debug_assert!(buf.len() >= self.len, "buffer too small for user slice");
        let usp =
            with_current_task(|t| t.clone_uspace()).expect("user task should have a user space");
        let mut usp_data = usp.write();
        let ptr: *const [T] = validate_user_array(
            A::PTE_FLAGS,
            usp_data.deref_mut(),
            self.ptr.addr(),
            self.len,
        )?;
        buf[0..self.len].copy_from_slice(unsafe { &*ptr });
        drop(usp_data);
        Ok(())
    }

    pub fn validate_with(&self, data: &mut UserSpaceData) -> Result<*const [T], SysError> {
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
        debug_assert!(buf.len() >= self.len, "buffer too small for user slice");
        let mut usp_data = usp.write();
        let ptr = validate_user_array_for_write(
            UserWrite::PTE_FLAGS,
            usp_data.deref_mut(),
            self.ptr.addr,
            self.len,
        )?;

        unsafe {
            (*ptr).copy_from_slice(&buf[0..self.len]);
        }
        drop(usp_data);
        Ok(())
    }

    pub fn validate_mut_with(&self, data: &mut UserSpaceData) -> Result<*mut [T], SysError> {
        let ptr =
            validate_user_array_for_write(UserWrite::PTE_FLAGS, data, self.ptr.addr, self.len)?;
        Ok(ptr)
    }

    pub fn safe_write_str(&self, s: &str) -> Result<(), SysError> {
        let bytes = s.as_bytes();
        debug_assert!(
            bytes.len() + 1 <= self.len,
            "string too long for user slice: {} bytes, but slice length is {}",
            bytes.len(),
            self.len
        );
        let usp =
            with_current_task(|t| t.clone_uspace()).expect("user task should have a user space");
        let mut usp_data = usp.write();
        let ptr = validate_user_array_for_write(
            UserWrite::PTE_FLAGS,
            usp_data.deref_mut(),
            self.ptr.addr,
            self.len,
        )?;
        unsafe {
            let ptr_ref = &mut *ptr;
            ptr_ref[0..bytes.len()].copy_from_slice(bytes);
            ptr_ref[bytes.len()] = 0;
        }
        drop(usp_data);
        Ok(())
    }
    pub fn safe_write_bytes_str(&self, bytes: &[u8]) -> Result<(), SysError> {
        debug_assert!(
            bytes.len() + 1 <= self.len,
            "string too long for user slice: {} bytes, but slice length is {}",
            bytes.len(),
            self.len
        );
        let usp =
            with_current_task(|t| t.clone_uspace()).expect("user task should have a user space");
        let mut usp_data = usp.write();
        let ptr = validate_user_array_for_write(
            UserWrite::PTE_FLAGS,
            usp_data.deref_mut(),
            self.ptr.addr,
            self.len,
        )?;
        unsafe {
            let ptr_ref = &mut *ptr;
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
    usp: &mut UserSpaceData,
    arg: u64,
) -> Result<*const T, SysError> {
    let va = VirtAddr::new(arg);
    let va_end = VirtAddr::new(arg.wrapping_add(size_of::<T>() as u64));
    if va_end < va {
        return Err(SysError::InvalidArgument.into());
    }
    let vpn_va = va.page_down();
    let vpn_va_end = va_end.page_up();
    for vpn in VirtPageRange::new(vpn_va, vpn_va_end - vpn_va).iter() {
        usp.check_permission(vpn, rwx_flags)?;
    }
    for vpn in VirtPageRange::new(vpn_va, vpn_va_end - vpn_va).iter() {
        usp.inject_page_fault(vpn.to_virt_addr(), PageFaultType::Read)?;
    }
    Ok(arg as *const T)
}
fn validate_user_pointer_for_write<T: Sized>(
    rwx_flags: PteFlags,
    usp: &mut UserSpaceData,
    arg: u64,
) -> Result<*mut T, SysError> {
    let va = VirtAddr::new(arg);
    let va_end = VirtAddr::new(arg.wrapping_add(size_of::<T>() as u64));
    if va_end < va {
        return Err(SysError::InvalidArgument.into());
    }
    let vpn_va = va.page_down();
    let vpn_va_end = va_end.page_up();
    for vpn in VirtPageRange::new(vpn_va, vpn_va_end - vpn_va).iter() {
        usp.check_permission(vpn, rwx_flags)?;
    }
    for vpn in VirtPageRange::new(vpn_va, vpn_va_end - vpn_va).iter() {
        //usp.copy_on_write(vpn.to_virt_addr())?;
        usp.inject_page_fault(vpn.to_virt_addr(), PageFaultType::Write)?;
    }
    Ok(arg as *mut T)
}

fn validate_user_array<T: Sized>(
    rwx_flags: PteFlags,
    usp: &mut UserSpaceData,
    arg: u64,
    len: usize,
) -> Result<*const [T], SysError> {
    if arg % align_of::<T>() as u64 != 0 {
        return Err(SysError::NotAligned.into());
    }

    let va = VirtAddr::new(arg);
    let va_end = VirtAddr::new(arg.wrapping_add((size_of::<T>() * len) as u64));
    if va_end < va {
        return Err(SysError::InvalidArgument.into());
    }

    let vpn_va = va.page_down();
    let vpn_va_end = va_end.page_up();
    for vpn in VirtPageRange::new(vpn_va, vpn_va_end - vpn_va).iter() {
        usp.check_permission(vpn, rwx_flags)?;
    }
    for vpn in VirtPageRange::new(vpn_va, vpn_va_end - vpn_va).iter() {
        usp.inject_page_fault(vpn.to_virt_addr(), PageFaultType::Read)?;
    }

    Ok(core::ptr::slice_from_raw_parts(arg as *const T, len))
}

fn validate_user_array_for_write<T: Sized>(
    rwx_flags: PteFlags,
    usp: &mut UserSpaceData,
    arg: u64,
    len: usize,
) -> Result<*mut [T], SysError> {
    /*kdebugln!(
        "validating user array pointer {:#x} with length {}: {:?}",
        arg,
        len,
        flags
    );*/
    if arg % align_of::<T>() as u64 != 0 {
        return Err(SysError::NotAligned.into());
    }

    let va = VirtAddr::new(arg);
    let va_end = VirtAddr::new(arg.wrapping_add((size_of::<T>() * len) as u64));
    if va_end < va {
        return Err(SysError::InvalidArgument.into());
    }

    let vpn_va = va.page_down();
    let vpn_va_end = va_end.page_up();
    for vpn in VirtPageRange::new(vpn_va, vpn_va_end - vpn_va).iter() {
        usp.check_permission(vpn, rwx_flags)?;
    }
    if rwx_flags.contains(PteFlags::WRITE) {
        for vpn in VirtPageRange::new(vpn_va, vpn_va_end - vpn_va).iter() {
            usp.inject_page_fault(vpn.to_virt_addr(), PageFaultType::Write)?;
        }
    }

    Ok(core::ptr::slice_from_raw_parts_mut(arg as *mut T, len))
}

/// Validate that the address in `arg` is inside user space and return it as a
/// [VirtAddr].
pub fn user_addr(arg: u64) -> Result<VirtAddr, SysError> {
    if arg < KernelLayout::USPACE_TOP_ADDR {
        Ok(VirtAddr::new(arg))
    } else {
        Err(SysError::InvalidArgument)
    }
}

pub trait SyscallArgValidatorExt<T>: FnOnce(u64) -> Result<T, SysError> + Sized {
    /// Overlay this validator with a mapper that transforms the validated value
    /// into another type.
    fn map<U, F>(self, mapper: F) -> impl FnOnce(u64) -> Result<U, SysError>
    where
        F: FnOnce(T) -> U,
    {
        move |arg| self(arg).map(mapper)
    }

    /// Overlay this validator with a mapper that transforms the validated value
    /// into another type, where the mapping can also fail.
    fn and_then<U, F>(self, mapper: F) -> impl FnOnce(u64) -> Result<U, SysError>
    where
        F: FnOnce(T) -> Result<U, SysError>,
    {
        move |arg| self(arg).and_then(mapper)
    }

    /// Lift this validator into an optional validator where a zero raw argument
    /// means the argument is absent.
    fn nullable(self) -> impl FnOnce(u64) -> Result<Option<T>, SysError> {
        move |arg| {
            if arg == 0 {
                Ok(None)
            } else {
                self(arg).map(Some)
            }
        }
    }
}

impl<T, V> SyscallArgValidatorExt<T> for V where V: FnOnce(u64) -> Result<T, SysError> {}

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
        return Err(SysError::NotAligned);
    }
    with_current_task(|t| {
        let usp = t
            .clone_uspace()
            .expect("user task should have a user space");
        let mut usp_data = usp.write();
        let st_pointer = arg as *const T;
        validate_user_pointer::<T>(PteFlags::READ, usp_data.deref_mut(), arg)?;
        let mut ed_pointer = st_pointer;
        let mut ed_vpn = VirtAddr::new(arg).page_down();
        let mut len = 0;
        while !unsafe { &*ed_pointer }.eq(&terminator) {
            let next_ed_pointer = (ed_pointer as u64).wrapping_add(size_of::<T>() as u64);
            if next_ed_pointer <= arg {
                return Err(SysError::InvalidArgument.into());
            }
            let ed_vpn_new = VirtAddr::new(next_ed_pointer).page_down();
            if ed_vpn_new != ed_vpn {
                validate_user_pointer::<u8>(
                    PteFlags::READ,
                    usp_data.deref_mut(),
                    next_ed_pointer as u64,
                )?;
                ed_vpn = ed_vpn_new;
            }
            ed_pointer = next_ed_pointer as *const T;
            len += 1;
            if len > MAX_LEN {
                return Err(SysError::InvalidArgument);
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
        let ptr = &*c_readonly_array_ptr::<MAX_USER_STRING_LEN, _>(0u8, true, arg)?;
        let str = CStr::from_ptr(&ptr[0] as *const u8 as *const c_char);
        let str = str.to_str().map_err(|_| SysError::InvalidArgument)?;
        Ok(Box::from(str))
    }
}

/// Validate a user pointer to an array of C strings and return copied
/// strings.
pub fn c_readonly_string_array(arg: u64) -> Result<Vec<Box<str>>, SysError> {
    let array = &*c_readonly_array_ptr::<MAX_USER_ARRAY_LEN, _>(0u64, false, arg)?;
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
        Err(SysError::InvalidArgument)
    }
}

/// Interpret the argument as an unsigned integer and validate that it is
/// nonzero.
pub fn nonzero(arg: u64) -> Result<u64, SysError> {
    if arg != 0 {
        Ok(arg)
    } else {
        Err(SysError::InvalidArgument)
    }
}

/// Validate that the argument is aligned to `ALIGN` bytes.
pub fn aligned_to<const ALIGN: usize>(arg: u64) -> Result<u64, SysError> {
    if arg % ALIGN as u64 == 0 {
        Ok(arg)
    } else {
        Err(SysError::InvalidArgument)
    }
}
