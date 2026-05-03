//! Syscall argument validation helpers for user-controlled data.

use core::{ops::DerefMut, str};

use crate::prelude::*;

/// The validated range is only valid when caller holds the lock of the
/// [UserSpaceData].
///
/// We don't consider [Protection::EXECUTE] here since syscalls only read/write
/// user memory, and the execute permission is only relevant for instruction
/// fetches.
///
/// Note: Write does not means the address is readable. A page
/// can be mapped write-only.
unsafe fn validate_user_range(
    write: bool,
    usp: &mut UserSpaceData,
    start: VirtAddr,
    len: usize,
) -> Result<(), SysError> {
    let end = start
        .get()
        .checked_add(len as u64)
        .ok_or(SysError::InvalidArgument)?;
    if end < start.get() {
        return Err(SysError::InvalidArgument);
    }

    let svpn = start.page_down();
    let evpn = VirtAddr::new(end).page_up();

    for vpn in VirtPageRange::new(svpn, evpn - svpn).iter() {
        usp.inject_page_fault(
            vpn.to_virt_addr(),
            if write {
                PageFaultType::Write
            } else {
                PageFaultType::Read
            },
        )?;
    }
    Ok(())
}

// explain this weird state machine... why write pointer can't be readable
// naturally?
mod ptrs {
    use super::*;

    #[derive(Debug)]
    pub struct UserReadPtr<'a, T: ?Sized> {
        pub(super) ptr: *const T,
        pub(super) writable: bool,
        pub(super) usp: &'a mut UserSpaceData,
    }

    #[derive(Debug)]
    pub struct UserWritePtr<'a, T: ?Sized> {
        pub(super) ptr: *mut T,
        pub(super) readable: bool,
        pub(super) usp: &'a mut UserSpaceData,
    }

    pub type UserReadSlice<'a, T> = UserReadPtr<'a, [T]>;
    pub type UserWriteSlice<'a, T> = UserWritePtr<'a, [T]>;

    fn validate_aligned_addr<T>(addr: VirtAddr) -> Result<VirtAddr, SysError> {
        let addr = user_addr(addr.get())?;
        if addr.get() % align_of::<T>() as u64 != 0 {
            return Err(SysError::NotAligned);
        }
        Ok(addr)
    }

    fn slice_byte_len<T>(len: usize) -> Result<usize, SysError> {
        len.checked_mul(size_of::<T>())
            .ok_or(SysError::InvalidArgument)
    }

    impl<'a, T: Copy> UserReadPtr<'a, T> {
        pub fn try_new(addr: VirtAddr, usp: &'a mut UserSpaceData) -> Result<Self, SysError> {
            let addr = user_addr(addr.get())?;
            if addr.get() % align_of::<T>() as u64 != 0 {
                return Err(SysError::NotAligned);
            }

            unsafe {
                validate_user_range(false, usp, addr, size_of::<T>())?;
            }

            Ok(UserReadPtr {
                ptr: addr.as_ptr(),
                writable: false,
                usp,
            })
        }

        pub fn read(&self) -> T {
            unsafe { self.ptr.read() }
        }

        pub fn to_write(mut self) -> Result<UserWritePtr<'a, T>, SysError> {
            if !self.writable {
                unsafe {
                    validate_user_range(
                        true,
                        self.usp,
                        VirtAddr::new(self.ptr as u64),
                        size_of::<T>(),
                    )?;
                }
                self.writable = true;
            }
            Ok(UserWritePtr {
                ptr: self.ptr as *mut T,
                readable: true,
                usp: self.usp,
            })
        }
    }

    impl<'a, T: Copy> UserWritePtr<'a, T> {
        pub fn try_new(addr: VirtAddr, usp: &'a mut UserSpaceData) -> Result<Self, SysError> {
            let addr = user_addr(addr.get())?;
            if addr.get() % align_of::<T>() as u64 != 0 {
                return Err(SysError::NotAligned);
            }

            unsafe {
                validate_user_range(true, usp, addr, size_of::<T>())?;
            }
            Ok(UserWritePtr {
                ptr: addr.as_ptr_mut(),
                readable: false,
                usp,
            })
        }

        pub fn write(&mut self, val: T) {
            unsafe {
                self.ptr.write(val);
            }
        }

        pub fn to_read(mut self) -> Result<UserReadPtr<'a, T>, SysError> {
            if !self.readable {
                unsafe {
                    validate_user_range(
                        false,
                        self.usp,
                        VirtAddr::new(self.ptr as u64),
                        size_of::<T>(),
                    )?;
                }
                self.readable = true;
            }
            Ok(UserReadPtr {
                ptr: self.ptr as *const T,
                writable: true,
                usp: self.usp,
            })
        }
    }

    impl<'a, T: Copy> UserReadPtr<'a, [T]> {
        pub fn try_new(
            addr: VirtAddr,
            len: usize,
            usp: &'a mut UserSpaceData,
        ) -> Result<Self, SysError> {
            let addr = user_addr(addr.get())?;
            if addr.get() % align_of::<T>() as u64 != 0 {
                return Err(SysError::NotAligned);
            }

            let byte_len = len
                .checked_mul(size_of::<T>())
                .ok_or(SysError::InvalidArgument)?;
            unsafe {
                validate_user_range(false, usp, addr, byte_len)?;
            }

            Ok(UserReadPtr {
                ptr: core::ptr::slice_from_raw_parts(addr.as_ptr(), len),
                writable: false,
                usp,
            })
        }

        /// Panics if kernel buffer is too small to hold the slice.
        ///
        /// We don't return a [SysError::BufferTooSmall]. We want callers to
        /// explicitly check the buffer size.
        pub fn copy_to_slice(&self, dst: &mut [T]) {
            debug_assert!(self.ptr.len() <= dst.len(), "kernel buffer is too small");

            unsafe {
                dst[..self.ptr.len()].copy_from_slice(&*self.ptr);
            }
        }

        pub fn to_write(mut self) -> Result<UserWritePtr<'a, [T]>, SysError> {
            if !self.writable {
                let byte_len = self
                    .ptr
                    .len()
                    .checked_mul(size_of::<T>())
                    .expect("we already checked this in try_new");
                unsafe {
                    validate_user_range(
                        true,
                        self.usp,
                        VirtAddr::new(self.ptr.cast::<T>() as u64),
                        byte_len,
                    )?;
                }
                self.writable = true;
            }
            Ok(UserWritePtr {
                ptr: self.ptr as *mut [T],
                readable: true,
                usp: self.usp,
            })
        }

        /// You want to perform some sophisticated pointer arithmetic/operations
        /// that are not covered by the provided APIs? Use this method.
        ///
        /// **The pointer is only valid for read operations.**
        pub unsafe fn with_ptr<F, R>(&self, f: F) -> R
        where
            F: FnOnce(*const [T]) -> R,
        {
            f(self.ptr)
        }

        /// See [Self::with_ptr].
        ///
        /// This method will validate the writable permission if it is not
        /// validated yet.
        pub unsafe fn with_writable_ptr<F, R>(&mut self, f: F) -> Result<R, SysError>
        where
            F: FnOnce(*mut [T]) -> R,
        {
            if !self.writable {
                let byte_len = self
                    .ptr
                    .len()
                    .checked_mul(size_of::<T>())
                    .expect("we already checked this in try_new");
                unsafe {
                    validate_user_range(
                        true,
                        self.usp,
                        VirtAddr::new(self.ptr.cast::<T>() as u64),
                        byte_len,
                    )?;
                }
                self.writable = true;
            }
            // TODO: this is ub.
            Ok(f(self.ptr as *mut [T]))
        }
    }

    impl<'a, T: Copy> UserWritePtr<'a, [T]> {
        pub fn try_new(
            addr: VirtAddr,
            len: usize,
            usp: &'a mut UserSpaceData,
        ) -> Result<Self, SysError> {
            let addr = user_addr(addr.get())?;
            if addr.get() % align_of::<T>() as u64 != 0 {
                return Err(SysError::NotAligned);
            }

            let byte_len = len
                .checked_mul(size_of::<T>())
                .ok_or(SysError::InvalidArgument)?;
            unsafe {
                validate_user_range(true, usp, addr, byte_len)?;
            }

            Ok(UserWritePtr {
                ptr: core::ptr::slice_from_raw_parts_mut(addr.as_ptr_mut(), len),
                readable: false,
                usp,
            })
        }

        /// Panics if kernel buffer is too large for user slice to hold.
        ///
        /// We don't return a [SysError::BufferTooSmall]. We want callers to
        /// explicitly check the buffer size.
        pub fn copy_from_slice(&mut self, src: &[T]) {
            debug_assert!(self.ptr.len() >= src.len(), "kernel buffer is too large");

            unsafe {
                (&mut *self.ptr)[..src.len()].copy_from_slice(src);
            }
        }

        pub fn to_read(mut self) -> Result<UserReadPtr<'a, [T]>, SysError> {
            if !self.readable {
                let byte_len = self
                    .ptr
                    .len()
                    .checked_mul(size_of::<T>())
                    .expect("we already checked this in try_new");
                unsafe {
                    validate_user_range(
                        false,
                        self.usp,
                        VirtAddr::new(self.ptr.cast::<T>() as u64),
                        byte_len,
                    )?;
                }
                self.readable = true;
            }
            Ok(UserReadPtr {
                ptr: self.ptr as *const [T],
                writable: true,
                usp: self.usp,
            })
        }

        /// You want to perform some sophisticated pointer arithmetic/operations
        /// that are not covered by the provided APIs? Use this method.
        ///
        /// **The pointer is only valid for write operations.**
        pub unsafe fn with_ptr<F, R>(&mut self, f: F) -> R
        where
            F: FnOnce(*mut [T]) -> R,
        {
            f(self.ptr)
        }

        /// See [Self::with_ptr].
        ///
        /// This method will validate the readable permission if it is not
        /// validated yet.
        pub unsafe fn with_readable_ptr<F, R>(&mut self, f: F) -> Result<R, SysError>
        where
            F: FnOnce(*mut [T]) -> R,
        {
            if !self.readable {
                let byte_len = self
                    .ptr
                    .len()
                    .checked_mul(size_of::<T>())
                    .expect("we already checked this in try_new");
                unsafe {
                    validate_user_range(
                        false,
                        self.usp,
                        VirtAddr::new(self.ptr.cast::<T>() as u64),
                        byte_len,
                    )?;
                }
                self.readable = true;
            }
            Ok(f(self.ptr as *mut [T]))
        }
    }

    impl<'a> UserWritePtr<'a, [u8]> {
        /// Panics if the string is too long to fit in the user slice (including
        /// the null terminator).
        ///
        /// A null-terminator will be appended after the string automatically.
        pub fn write_utf8_str(&mut self, s: &str) {
            debug_assert!(
                s.as_bytes().len() + 1 <= self.ptr.len(),
                "string too long for user slice: {} bytes, but slice length is {}",
                s.as_bytes().len(),
                self.ptr.len()
            );
            unsafe {
                let ptr_ref = &mut *self.ptr;
                ptr_ref[0..s.as_bytes().len()].copy_from_slice(s.as_bytes());
                ptr_ref[s.as_bytes().len()] = 0;
            }
        }

        /// Panics if the bytes are too long to fit in the user slice (including
        /// the null terminator).
        ///
        /// A null-terminator will be appended after the bytes automatically. So
        /// passed-in `bytes` don't need to have a null terminator.
        pub fn write_bytes_with_null_terminator(&mut self, bytes: &[u8]) {
            debug_assert!(
                bytes.len() + 1 <= self.ptr.len(),
                "bytes too long for user slice: {} bytes, but slice length is {}",
                bytes.len(),
                self.ptr.len()
            );
            unsafe {
                let ptr_ref = &mut *self.ptr;
                ptr_ref[0..bytes.len()].copy_from_slice(bytes);
                ptr_ref[bytes.len()] = 0;
            }
        }
    }
}
pub use ptrs::*;

pub trait SyscallArgValidatorExt<T>: FnOnce(u64) -> Result<T, SysError> + Sized {
    /// Overlay this validator with a mapper that transforms the validated
    /// value into another type.
    fn map<U, F>(self, mapper: F) -> impl FnOnce(u64) -> Result<U, SysError>
    where
        F: FnOnce(T) -> U,
    {
        move |arg| self(arg).map(mapper)
    }

    /// Overlay this validator with a mapper that transforms the validated
    /// value into another type, where the mapping can also fail.
    fn and_then<U, F>(self, mapper: F) -> impl FnOnce(u64) -> Result<U, SysError>
    where
        F: FnOnce(T) -> Result<U, SysError>,
    {
        move |arg| self(arg).and_then(mapper)
    }

    /// Lift this validator into an optional validator where a zero raw
    /// argument means the argument is absent.
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

mod validators {
    use super::*;

    /// Validate that the address in `arg` is inside user space and return it as
    /// a [VirtAddr].
    pub fn user_addr(arg: u64) -> Result<VirtAddr, SysError> {
        if arg < KernelLayout::USPACE_TOP_ADDR {
            Ok(VirtAddr::new(arg))
        } else {
            Err(SysError::InvalidArgument)
        }
    }

    /// `terminator` defines the end of the array.
    ///
    /// If `include_terminator` is true, the returned array will include the
    /// terminator as the last element. Otherwise, the terminator is not
    /// included in the returned array.
    ///
    /// In fact, this function almost always only serves as a helper for parsing
    /// C strings and arrays of C strings.
    fn c_readonly_array_from_addr<const MAX_LEN: usize, T: Eq + Copy>(
        usp: &mut UserSpaceData,
        start: VirtAddr,
        terminator: T,
        include_terminator: bool,
    ) -> Result<Box<[T]>, SysError> {
        let elem_size = size_of::<T>();
        if elem_size == 0 {
            return Err(SysError::InvalidArgument);
        }
        if start.get() % align_of::<T>() as u64 != 0 {
            return Err(SysError::NotAligned);
        }

        let elem_size_u64 = elem_size as u64;
        let mut current = start;
        let mut validated_until = 0u64;
        let mut values = Vec::new();

        loop {
            let elem_end = current
                .get()
                .checked_add(elem_size_u64)
                .ok_or(SysError::InvalidArgument)?;
            if current.get() >= KernelLayout::USPACE_TOP_ADDR
                || elem_end > KernelLayout::USPACE_TOP_ADDR
            {
                return Err(SysError::InvalidArgument);
            }

            if elem_end > validated_until {
                unsafe {
                    validate_user_range(false, usp, current, elem_size)?;
                }
                validated_until = VirtAddr::new(elem_end).page_up().to_virt_addr().get();
            }

            let value = unsafe { (current.get() as *const T).read() };
            if value == terminator {
                if include_terminator {
                    values.push(value);
                }
                return Ok(values.into_boxed_slice());
            }

            if values.len() == MAX_LEN {
                return Err(SysError::InvalidArgument);
            }
            values.push(value);
            current = VirtAddr::new(elem_end);
        }
    }

    /// Validate a user C string pointer and return a copied string slice.
    ///
    /// `MAX_BYTES` defines the maximum allowed length of the string in bytes,
    /// excluding the null terminator, which is syscall-specific.
    pub fn c_readonly_string<const MAX_BYTES: usize>(arg: u64) -> Result<Box<str>, SysError> {
        let start = user_addr(arg)?;
        let usp = get_current_task().clone_uspace();
        let mut usp_data = usp.write();
        let bytes =
            c_readonly_array_from_addr::<MAX_BYTES, u8>(usp_data.deref_mut(), start, 0, false)?;
        let s = str::from_utf8(&bytes).map_err(|_| SysError::InvalidArgument)?;
        Ok(Box::from(s))
    }

    /// Validate a user pointer to an array of C strings and return copied
    /// strings.
    ///
    /// `MAX_ARRAY_LEN` defines the maximum allowed number of strings in the
    /// array.
    ///
    /// `MAX_BYTES_EACH_STRING` defines the maximum allowed length of each
    /// string in bytes, excluding the null terminator, which is
    /// syscall-specific.
    pub fn c_readonly_string_array<
        const MAX_ARRAY_LEN: usize,
        const MAX_BYTES_EACH_STRING: usize,
    >(
        arg: u64,
    ) -> Result<Vec<Box<str>>, SysError> {
        let start = user_addr(arg)?;
        let usp = get_current_task().clone_uspace();
        let mut usp_data = usp.write();
        let ptrs = c_readonly_array_from_addr::<MAX_ARRAY_LEN, u64>(
            usp_data.deref_mut(),
            start,
            0,
            false,
        )?;

        let mut strings = Vec::with_capacity(ptrs.len());
        for &ptr in ptrs.iter() {
            let bytes = c_readonly_array_from_addr::<MAX_BYTES_EACH_STRING, u8>(
                usp_data.deref_mut(),
                user_addr(ptr)?,
                0,
                false,
            )?;
            let s = str::from_utf8(&bytes).map_err(|_| SysError::InvalidArgument)?;
            strings.push(Box::from(s));
        }
        Ok(strings)
    }

    /// Interpret the argument as a signed integer and validate that it is
    /// greater than zero.
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
}
pub use validators::*;
