//! Syscall argument validation helpers for user-controlled data.
//!
//! TODO: tlb shootdown, out of mutex lock.

use core::{mem::MaybeUninit, str};

use crate::{
    arch::UserPtrAccessor,
    exception::trap::{UserPtrAccessError, UserPtrAccessorArch},
    prelude::*,
};

fn user_pointer_addr(arg: u64) -> Result<VirtAddr, SysError> {
    if arg < KernelLayout::USPACE_TOP_ADDR {
        Ok(VirtAddr::new(arg))
    } else {
        Err(SysError::BadAddress)
    }
}

fn user_memory_error(err: SysError) -> SysError {
    match err {
        SysError::InvalidArgument
        | SysError::PermissionDenied
        | SysError::NotMapped
        | SysError::RangeNotMapped => SysError::BadAddress,
        other => other,
    }
}

fn validate_user_range(start: VirtAddr, len: usize) -> Result<(), SysError> {
    if start.get() >= KernelLayout::USPACE_TOP_ADDR {
        return Err(SysError::BadAddress);
    }

    let end = start
        .get()
        .checked_add(len as u64)
        .ok_or(SysError::BadAddress)?;
    if end > KernelLayout::USPACE_TOP_ADDR {
        return Err(SysError::BadAddress);
    }
    if len == 0 {
        return Ok(());
    }

    Ok(())
}

fn fault_in_user_range(
    usp: &mut UserSpace,
    start: VirtAddr,
    len: usize,
    access: PageFaultType,
) -> Result<(), SysError> {
    validate_user_range(start, len)?;
    if len == 0 {
        return Ok(());
    }

    let end = VirtAddr::new(start.get() + len as u64);
    let svpn = start.page_down();
    let evpn = end.page_up();
    for vpn in VirtPageRange::new(svpn, evpn - svpn).iter() {
        let fence = usp
            .inject_page_fault(vpn.to_virt_addr(), access)
            .map_err(user_memory_error)?;
        drop(fence);
    }
    Ok(())
}

fn map_user_pointer_error(error: UserPtrAccessError) -> UserPtrAccessError {
    UserPtrAccessError::new(user_memory_error(error.error()), error.copied())
}

fn read_user_bytes(
    usp: &mut UserSpace,
    dst: &mut [u8],
    src: VirtAddr,
) -> Result<usize, UserPtrAccessError> {
    let result = UserPtrAccessor::read(usp, dst, src).map_err(map_user_pointer_error);
    if let Ok(copied) = result {
        assert_eq!(copied, dst.len(), "successful user read was short");
    }
    result
}

fn write_user_bytes(
    usp: &mut UserSpace,
    dst: VirtAddr,
    src: &[u8],
) -> Result<usize, UserPtrAccessError> {
    let result = UserPtrAccessor::write(usp, dst, src).map_err(map_user_pointer_error);
    if let Ok(copied) = result {
        assert_eq!(copied, src.len(), "successful user write was short");
    }
    result
}

mod ptrs {
    use super::*;

    #[derive(Debug)]
    pub struct UserReadPtr<'a, T: ?Sized> {
        pub(super) ptr: *const T,
        pub(super) usp: &'a mut UserSpace,
    }

    #[derive(Debug)]
    pub struct UserWritePtr<'a, T: ?Sized> {
        pub(super) ptr: *mut T,
        pub(super) usp: &'a mut UserSpace,
    }

    pub type UserReadSlice<'a, T> = UserReadPtr<'a, [T]>;
    pub type UserWriteSlice<'a, T> = UserWritePtr<'a, [T]>;

    impl<'a, T: Copy> UserReadPtr<'a, T> {
        pub fn try_new(addr: VirtAddr, usp: &'a mut UserSpace) -> Result<Self, SysError> {
            let addr = user_pointer_addr(addr.get())?;
            validate_user_range(addr, size_of::<T>())?;

            Ok(UserReadPtr {
                ptr: addr.as_ptr(),
                usp,
            })
        }

        pub fn read(&mut self) -> Result<T, SysError> {
            let mut value = MaybeUninit::<T>::zeroed();
            let bytes = unsafe {
                core::slice::from_raw_parts_mut(value.as_mut_ptr().cast::<u8>(), size_of::<T>())
            };
            read_user_bytes(self.usp, bytes, VirtAddr::new(self.ptr as u64))
                .map_err(|error| error.error())?;
            Ok(unsafe { value.assume_init() })
        }
    }

    impl<'a, T: Copy> UserWritePtr<'a, T> {
        pub fn try_new(addr: VirtAddr, usp: &'a mut UserSpace) -> Result<Self, SysError> {
            let addr = user_pointer_addr(addr.get())?;
            validate_user_range(addr, size_of::<T>())?;
            Ok(UserWritePtr {
                ptr: addr.as_ptr_mut(),
                usp,
            })
        }

        pub fn write(&mut self, val: T) -> Result<(), SysError> {
            let bytes = unsafe {
                core::slice::from_raw_parts((&val as *const T).cast::<u8>(), size_of::<T>())
            };
            write_user_bytes(self.usp, VirtAddr::new(self.ptr as u64), bytes)
                .map_err(|error| error.error())?;
            Ok(())
        }

        /// Resolve and validate the complete write range before a syscall
        /// performs an external side effect. Ordinary copyout must call
        /// [Self::write] directly and use the exception-backed fast path.
        pub(crate) fn fault_in(&mut self) -> Result<(), SysError> {
            fault_in_user_range(
                self.usp,
                VirtAddr::new(self.ptr as u64),
                size_of::<T>(),
                PageFaultType::Write,
            )
        }
    }

    impl<'a, T: Copy> UserReadPtr<'a, [T]> {
        pub fn try_new(
            addr: VirtAddr,
            len: usize,
            usp: &'a mut UserSpace,
        ) -> Result<Self, SysError> {
            let addr = user_pointer_addr(addr.get())?;
            let byte_len = len
                .checked_mul(size_of::<T>())
                .ok_or(SysError::InvalidArgument)?;
            validate_user_range(addr, byte_len)?;

            Ok(UserReadPtr {
                ptr: core::ptr::slice_from_raw_parts(addr.as_ptr(), len),
                usp,
            })
        }

        /// Panics if kernel buffer is too small to hold the slice.
        ///
        /// We don't return a [SysError::BufferTooSmall]. We want callers to
        /// explicitly check the buffer size.
        pub fn copy_to_slice(&mut self, dst: &mut [T]) -> Result<(), SysError> {
            debug_assert!(self.ptr.len() <= dst.len(), "kernel buffer is too small");

            let byte_len = self.ptr.len() * size_of::<T>();
            let bytes =
                unsafe { core::slice::from_raw_parts_mut(dst.as_mut_ptr().cast::<u8>(), byte_len) };
            read_user_bytes(self.usp, bytes, VirtAddr::new(self.ptr.cast::<T>() as u64))
                .map_err(|error| error.error())?;
            Ok(())
        }
    }

    impl<'a> UserReadPtr<'a, [u8]> {
        /// Ordinary byte-stream I/O needs the architecture-reported prefix so
        /// its cursor can publish short progress instead of erasing it as an
        /// exact typed-copy failure.
        pub(crate) fn copy_to_slice_partial(
            &mut self,
            dst: &mut [u8],
        ) -> Result<usize, UserPtrAccessError> {
            debug_assert!(self.ptr.len() <= dst.len(), "kernel buffer is too small");
            read_user_bytes(
                self.usp,
                &mut dst[..self.ptr.len()],
                VirtAddr::new(self.ptr.cast::<u8>() as u64),
            )
        }
    }

    impl<'a, T: Copy> UserWritePtr<'a, [T]> {
        pub fn try_new(
            addr: VirtAddr,
            len: usize,
            usp: &'a mut UserSpace,
        ) -> Result<Self, SysError> {
            let addr = user_pointer_addr(addr.get())?;
            let byte_len = len
                .checked_mul(size_of::<T>())
                .ok_or(SysError::InvalidArgument)?;
            validate_user_range(addr, byte_len)?;

            Ok(UserWritePtr {
                ptr: core::ptr::slice_from_raw_parts_mut(addr.as_ptr_mut(), len),
                usp,
            })
        }

        /// Panics if kernel buffer is too large for user slice to hold.
        ///
        /// We don't return a [SysError::BufferTooSmall]. We want callers to
        /// explicitly check the buffer size.
        pub fn copy_from_slice(&mut self, src: &[T]) -> Result<(), SysError> {
            debug_assert!(self.ptr.len() >= src.len(), "kernel buffer is too large");

            let byte_len = src.len() * size_of::<T>();
            let bytes = unsafe { core::slice::from_raw_parts(src.as_ptr().cast::<u8>(), byte_len) };
            write_user_bytes(self.usp, VirtAddr::new(self.ptr.cast::<T>() as u64), bytes)
                .map_err(|error| error.error())?;
            Ok(())
        }

        /// See [UserWritePtr::fault_in]. This deliberately retains the old MM
        /// walk only for callers that require complete prevalidation.
        pub(crate) fn fault_in(&mut self) -> Result<(), SysError> {
            let byte_len = self
                .ptr
                .len()
                .checked_mul(size_of::<T>())
                .expect("we already checked this in try_new");
            fault_in_user_range(
                self.usp,
                VirtAddr::new(self.ptr.cast::<T>() as u64),
                byte_len,
                PageFaultType::Write,
            )
        }
    }

    impl<'a> UserWritePtr<'a, [u8]> {
        /// See [`UserReadPtr::copy_to_slice_partial`]. This byte-only entry
        /// keeps partial progress out of scalar and structured copy APIs.
        pub(crate) fn copy_from_slice_partial(
            &mut self,
            src: &[u8],
        ) -> Result<usize, UserPtrAccessError> {
            debug_assert!(self.ptr.len() >= src.len(), "kernel buffer is too large");
            write_user_bytes(self.usp, VirtAddr::new(self.ptr.cast::<u8>() as u64), src)
        }

        /// Panics if the string is too long to fit in the user slice (including
        /// the null terminator).
        ///
        /// A null-terminator will be appended after the string automatically.
        pub fn write_utf8_str(&mut self, s: &str) -> Result<(), SysError> {
            debug_assert!(
                s.as_bytes().len() + 1 <= self.ptr.len(),
                "string too long for user slice: {} bytes, but slice length is {}",
                s.as_bytes().len(),
                self.ptr.len()
            );
            self.copy_from_slice(s.as_bytes())?;
            let terminator = VirtAddr::new(self.ptr.cast::<u8>() as u64 + s.len() as u64);
            write_user_bytes(self.usp, terminator, &[0]).map_err(|error| error.error())?;
            Ok(())
        }

        /// Panics if the bytes are too long to fit in the user slice (including
        /// the null terminator).
        ///
        /// A null-terminator will be appended after the bytes automatically. So
        /// passed-in `bytes` don't need to have a null terminator.
        pub fn write_bytes_with_null_terminator(&mut self, bytes: &[u8]) -> Result<(), SysError> {
            debug_assert!(
                bytes.len() + 1 <= self.ptr.len(),
                "bytes too long for user slice: {} bytes, but slice length is {}",
                bytes.len(),
                self.ptr.len()
            );
            self.copy_from_slice(bytes)?;
            let terminator = VirtAddr::new(self.ptr.cast::<u8>() as u64 + bytes.len() as u64);
            write_user_bytes(self.usp, terminator, &[0]).map_err(|error| error.error())?;
            Ok(())
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
            Err(SysError::BadAddress)
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
        usp: &mut UserSpace,
        start: VirtAddr,
        terminator: T,
        include_terminator: bool,
    ) -> Result<Box<[T]>, SysError> {
        let elem_size = size_of::<T>();
        if elem_size == 0 {
            return Err(SysError::InvalidArgument);
        }
        let elem_size_u64 = elem_size as u64;
        let mut current = start;
        let mut values = Vec::new();

        loop {
            let elem_end = current
                .get()
                .checked_add(elem_size_u64)
                .ok_or(SysError::BadAddress)?;
            if current.get() >= KernelLayout::USPACE_TOP_ADDR
                || elem_end > KernelLayout::USPACE_TOP_ADDR
            {
                return Err(SysError::BadAddress);
            }

            let mut user_value = UserReadPtr::<T>::try_new(current, usp)?;
            let value = user_value.read()?;
            if value == terminator {
                if include_terminator {
                    values.push(value);
                }
                return Ok(values.into_boxed_slice());
            }

            if values.len() == MAX_LEN {
                return Err(SysError::ListTooLong);
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
        let start = user_pointer_addr(arg)?;
        let usp = get_current_task().clone_uspace_handle();
        let bytes =
            usp.with_usp(|usp| c_readonly_array_from_addr::<MAX_BYTES, u8>(usp, start, 0, false))?;
        let s = str::from_utf8(&bytes).map_err(|_| SysError::InvalidArgument)?;
        Ok(Box::from(s))
    }

    /// Validate a user C string pointer as a filesystem pathname.
    pub fn c_readonly_path(arg: u64) -> Result<Box<str>, SysError> {
        c_readonly_string::<MAX_PATH_LEN_BYTES>(arg).map_err(|err| match err {
            SysError::ListTooLong => SysError::NameTooLong,
            other => other,
        })
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
        let start = user_pointer_addr(arg)?;
        let usp = get_current_task().clone_uspace_handle();

        let strings = usp.with_usp(|usp| {
            let ptrs = c_readonly_array_from_addr::<MAX_ARRAY_LEN, u64>(usp, start, 0, false)?;

            let mut strings = Vec::with_capacity(ptrs.len());
            for &ptr in ptrs.iter() {
                let bytes = c_readonly_array_from_addr::<MAX_BYTES_EACH_STRING, u8>(
                    usp,
                    user_pointer_addr(ptr)?,
                    0,
                    false,
                )?;
                let s = str::from_utf8(&bytes).map_err(|_| SysError::InvalidArgument)?;
                strings.push(Box::from(s));
            }

            Ok(strings)
        })?;
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
