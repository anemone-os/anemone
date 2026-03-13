#[derive(Debug)]

pub(crate) struct BitSlice<'a> {
    /// length of the bit slice in bits, not bytes
    len: usize,
    bits: &'a [u8],
}

#[derive(Debug)]
pub(crate) struct BitSliceMut<'a> {
    /// length of the bit slice in bits, not bytes
    len: usize,
    bits: &'a mut [u8],
}

#[derive(Debug)]
pub(crate) enum BitSliceError {
    OutOfBounds,
}

macro_rules! impl_bitslice_basic {
    ($bitslice:ty) => {
        impl $bitslice {
            /// Tests if the bit at the given index is set.
            pub(crate) fn test(&self, index: usize) -> Result<bool, BitSliceError> {
                if index >= self.len {
                    return Err(BitSliceError::OutOfBounds);
                }
                let byte_index = index / 8;
                let bit_index = index % 8;
                Ok((self.bits[byte_index] & (1 << bit_index)) != 0)
            }

            /// Returns the length of the bit slice in bits.
            pub(crate) fn len(&self) -> usize {
                self.len
            }
        }
    };
}

impl_bitslice_basic!(BitSlice<'_>);
impl_bitslice_basic!(BitSliceMut<'_>);

impl BitSlice<'_> {
    pub(crate) unsafe fn from_raw_parts(bits: *const u8, len: usize) -> Self {
        unsafe {
            Self {
                len,
                bits: core::slice::from_raw_parts(bits, (len + 7) / 8),
            }
        }
    }
}

impl BitSliceMut<'_> {
    pub(crate) unsafe fn from_raw_parts(bits: *mut u8, len: usize) -> Self {
        unsafe {
            Self {
                len,
                bits: core::slice::from_raw_parts_mut(bits, (len + 7) / 8),
            }
        }
    }

    /// Sets the bit at the given index.
    pub(crate) fn set(&mut self, index: usize) -> Result<(), BitSliceError> {
        if index >= self.len {
            return Err(BitSliceError::OutOfBounds);
        }
        let byte_index = index / 8;
        let bit_index = index % 8;
        self.bits[byte_index] |= 1 << bit_index;
        Ok(())
    }

    /// Clears the bit at the given index.
    pub(crate) fn clear(&mut self, index: usize) -> Result<(), BitSliceError> {
        if index >= self.len {
            return Err(BitSliceError::OutOfBounds);
        }
        let byte_index = index / 8;
        let bit_index = index % 8;
        self.bits[byte_index] &= !(1 << bit_index);
        Ok(())
    }
}

// TODO: implement more bitmap operations like find_first_zero,
// find_first_one, etc. TODO: implement iterators for BitSlice and
// BitSliceMut
