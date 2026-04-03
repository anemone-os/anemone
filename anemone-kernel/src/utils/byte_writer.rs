use core::ptr::NonNull;

/// Helper for writing bytes to a buffer.
///
/// Internally this struct does not store a reference to the buffer, so it's
/// safe to use even when writing to user space.
///
/// Alignment is automatically handled. But you can also exlicitly specify
/// stronger alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteWriter {
    buffer: NonNull<[u8]>,
    offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteWriterError {
    BufferOverflow,
}

impl ByteWriter {
    /// Create a new `ByteWriter` with the given buffer. The offset is
    /// initialized to `0`.
    ///
    /// # Safety
    ///
    /// Caller must ensure that the buffer is valid for the entire lifetime of
    /// the `ByteWriter`. All operations are not marked as unsafe,
    /// which relies on that guarantee.
    pub const unsafe fn new(buffer: NonNull<[u8]>) -> Self {
        Self { buffer, offset: 0 }
    }

    pub const fn current_offset(&self) -> usize {
        self.offset
    }

    /// Align the offset to the alignment of `T` and write `val` to the buffer.
    ///
    /// Returns the offset where `val` is written.
    pub fn write_val<T: Sized + Copy>(&mut self, val: &T) -> Result<usize, ByteWriterError> {
        self.write_val_inner::<T, true>(val)
    }

    /// Write `val` to the buffer without aligning the offset. This is useful
    /// when the caller wants to write a packed struct.
    ///
    /// Returns the offset where `val` is written.
    pub fn write_val_unaligned<T: Sized + Copy>(
        &mut self,
        val: &T,
    ) -> Result<usize, ByteWriterError> {
        self.write_val_inner::<T, false>(val)
    }

    /// Align the offset to the alignment of `T` and write `slice` to the
    /// buffer.
    ///
    /// Returns the offset where `slice` is written.
    pub fn write_slice<T: Sized + Copy>(&mut self, slice: &[T]) -> Result<usize, ByteWriterError> {
        self.write_slice_inner::<T, true>(slice)
    }

    /// Write `slice` to the buffer without aligning the offset. This is useful
    /// when the caller wants to write a packed struct.
    ///
    /// Returns the offset where `slice` is written.
    pub fn write_slice_unaligned<T: Sized + Copy>(
        &mut self,
        slice: &[T],
    ) -> Result<usize, ByteWriterError> {
        self.write_slice_inner::<T, false>(slice)
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<usize, ByteWriterError> {
        let soffset = self.offset;
        self.ensure_capacity(bytes.len())?;

        unsafe {
            let ptr = self.buffer.as_ptr().cast::<u8>().add(self.offset);
            ptr.copy_from_nonoverlapping(bytes.as_ptr(), bytes.len());
        }

        self.offset += bytes.len();
        Ok(soffset)
    }

    /// Write a null-terminated string to the buffer.
    ///
    /// The `s` itself is just a Rust string slice and not null-terminated. This
    /// method will write the bytes of `s` followed by a null terminator
    /// (`0u8`).
    ///
    /// Returns the offset where the string is written (i.e. the offset of the
    /// first byte of `s`).
    pub fn write_null_terminated_str(&mut self, s: &str) -> Result<usize, ByteWriterError> {
        let soffset = self.offset;
        self.write_bytes(s.as_bytes())?;
        self.write_val(&0u8)?; // null terminator
        Ok(soffset)
    }

    /// Align the offset to `align`. `align` must be a power of two, which is
    /// only checked in debug mode.
    pub fn align_to(&mut self, align: usize) -> Result<(), ByteWriterError> {
        debug_assert!(align.is_power_of_two());

        if self.offset % align != 0 {
            self.offset += align - (self.offset % align);
        }
        self.ensure_capacity(0)
    }
}

impl ByteWriter {
    fn write_val_inner<T: Sized + Copy, const ALIGNED: bool>(
        &mut self,
        val: &T,
    ) -> Result<usize, ByteWriterError> {
        if ALIGNED {
            self.align_to(align_of::<T>())?;
        }

        let soffset = self.offset;
        self.ensure_capacity(size_of::<T>())?;

        unsafe {
            let ptr = self
                .buffer
                .as_ptr()
                .cast::<u8>()
                .add(self.offset)
                .cast::<T>();
            if ALIGNED {
                ptr.write(*val);
            } else {
                ptr.write_unaligned(*val);
            }
        }

        self.offset += size_of::<T>();
        Ok(soffset)
    }

    fn write_slice_inner<T: Sized + Copy, const ALIGNED: bool>(
        &mut self,
        slice: &[T],
    ) -> Result<usize, ByteWriterError> {
        if ALIGNED {
            self.align_to(align_of::<T>())?;
        }

        let soffset = self.offset;
        let byte_len = slice.len() * size_of::<T>();
        self.ensure_capacity(byte_len)?;

        unsafe {
            let ptr = self
                .buffer
                .as_ptr()
                .cast::<u8>()
                .add(self.offset)
                .cast::<T>();

            if ALIGNED {
                ptr.copy_from_nonoverlapping(slice.as_ptr(), slice.len());
            } else {
                let ptr = ptr.cast::<u8>();
                let slice_bytes =
                    core::slice::from_raw_parts(slice.as_ptr().cast::<u8>(), byte_len);
                ptr.copy_from_nonoverlapping(slice_bytes.as_ptr(), byte_len);
            }
        }

        self.offset += byte_len;
        Ok(soffset)
    }

    fn ensure_capacity(&self, additional: usize) -> Result<(), ByteWriterError> {
        match self.buffer.len().checked_sub(self.offset) {
            Some(remaining) if remaining >= additional => Ok(()),
            _ => Err(ByteWriterError::BufferOverflow),
        }
    }
}
