//! Syscall argument validation helpers for user-controlled data.

use core::{
    ffi::{CStr, c_char},
    marker::PhantomData,
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
    _marker: PhantomData<(A, *const T)>,
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
        with_current_task(|t| {
            let memsp = t
                .clone_uspace()
                .expect("user task should have a user space");
            let mut table = memsp.page_table_mut();
            validate_user_pointer::<T>(A::PTE_FLAGS, &mut *table, arg)?;
            drop(table);
            Ok(Self {
                addr: arg,
                _marker: PhantomData,
            })
        })
    }

    pub fn addr(self) -> u64 {
        self.addr
    }

    pub fn as_ptr(self) -> *const T {
        self.addr as *const T
    }

    /// # Safety
    ///
    /// `task` must be the task that owns this pointer.
    pub unsafe fn slice(self, len: usize, task: &Task) -> Result<UserSlice<T, A>, SysError> {
        let memsp = task
            .clone_uspace()
            .expect("user task should have a user space");
        let mut table = memsp.page_table_mut();
        validate_user_array::<T>(A::PTE_FLAGS, &mut *table, self.addr, len)?;
        drop(table);
        Ok(UserSlice { ptr: self, len })
    }
}

impl<T: Sized> UserPtr<T, UserWrite> {
    pub fn as_mut_ptr(self) -> *mut T {
        self.addr as *mut T
    }
}

impl<T: Sized, A: UserAccess> TryFromSyscallArg for UserPtr<T, A> {
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

impl<T: Sized, A: UserAccess> Copy for UserSlice<T, A> {}

impl<T: Sized, A: UserAccess> Clone for UserSlice<T, A> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Sized, A: UserAccess> UserSlice<T, A> {
    pub fn from_raw_parts(ptr: *const T, len: usize) -> Result<Self, SysError> {
        with_current_task(|t| {
            let memsp = t
                .clone_uspace()
                .expect("user task should have a user space");
            let mut table = memsp.page_table_mut();
            validate_user_array::<T>(A::PTE_FLAGS, &mut *table, ptr as u64, len)?;
            drop(table);
            Ok(Self {
                ptr: UserPtr {
                    addr: ptr as u64,
                    _marker: PhantomData,
                },
                len,
            })
        })
    }

    pub fn len(self) -> usize {
        self.len
    }

    pub fn is_empty(self) -> bool {
        self.len == 0
    }

    pub fn addr(self) -> u64 {
        self.ptr.addr()
    }

    pub fn as_ptr(self) -> *const T {
        self.ptr.as_ptr()
    }

    pub fn as_slice_ptr(self) -> *const [T] {
        core::ptr::slice_from_raw_parts(self.as_ptr(), self.len)
    }

    pub unsafe fn copy_to(&self, dest: &mut [T]) -> Result<(), SysError> {
        if dest.len() < self.len {
            return Err(SysError::Kernel(KernelError::BufferTooSmall));
        }
        unsafe {
            core::ptr::copy_nonoverlapping(self.as_ptr(), dest.as_mut_ptr(), self.len);
        }
        Ok(())
    }
}

impl<T: Sized> UserSlice<T, UserWrite> {
    pub fn as_mut_ptr(self) -> *mut T {
        self.ptr.as_mut_ptr()
    }

    pub fn as_mut_slice_ptr(self) -> *mut [T] {
        core::ptr::slice_from_raw_parts_mut(self.as_mut_ptr(), self.len)
    }

    pub unsafe fn copy_from(&mut self, src: &[T]) -> Result<(), SysError> {
        if src.len() > self.len {
            return Err(SysError::Kernel(KernelError::BufferTooSmall));
        }
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), self.as_mut_ptr(), src.len());
        }
        Ok(())
    }
}

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

fn validate_user_array<T: Sized>(
    rwx_flags: PteFlags,
    table: &mut PageTable,
    arg: u64,
    len: usize,
) -> Result<*const [T], SysError> {
    if arg % align_of::<T>() as u64 != 0 {
        return Err(MmError::NotAligned.into());
    }
    let flags = PteFlags::USER | rwx_flags;

    let va = VirtAddr::new(arg);
    let va_end = VirtAddr::new(arg.wrapping_add((size_of::<T>() * len) as u64));
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

    Ok(core::ptr::slice_from_raw_parts(arg as *const T, len))
}

pub fn user_addr(arg: u64) -> Result<VirtAddr, SysError> {
    if arg < KernelLayout::USPACE_TOP_ADDR {
        Ok(VirtAddr::new(arg))
    } else {
        Err(KernelError::InvalidArgument.into())
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
            let next_ed_pointer = (ed_pointer as u64).wrapping_add(size_of::<T>() as u64);
            if next_ed_pointer <= arg {
                return Err(MmError::InvalidArgument.into());
            }
            let ed_vpn_new = VirtAddr::new(next_ed_pointer).page_down();
            if ed_vpn_new != ed_vpn {
                validate_user_pointer::<u8>(PteFlags::READ, &mut *table, next_ed_pointer as u64)?;
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
