use alloc::boxed::Box;

/// Bitmap with a fixed number of bits (NUM_DWORDS * 64).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitmap<const NUM_DWORDS: usize> {
    bits: Box<[u64; NUM_DWORDS]>,
}

impl<const NUM_DWORDS: usize> Bitmap<NUM_DWORDS> {
    fn position(index: usize) -> (usize, usize) {
        let dword_index = index / 64;
        let bit_index = index % 64;
        (dword_index, bit_index)
    }
}
impl<const NUM_DWORDS: usize> Bitmap<NUM_DWORDS> {
    pub fn new() -> Self {
        const_assert!(NUM_DWORDS > 0);

        // avoid stack overflow.
        let bits = unsafe { Box::new_zeroed().assume_init() };

        Self { bits }
    }

    pub fn capacity(&self) -> usize {
        NUM_DWORDS * 64
    }

    pub fn set(&mut self, index: usize) {
        let (dword_index, bit_index) = Self::position(index);
        self.bits[dword_index] |= 1u64 << bit_index;
    }

    pub fn clear(&mut self, index: usize) {
        let (dword_index, bit_index) = Self::position(index);
        self.bits[dword_index] &= !(1u64 << bit_index);
    }

    pub fn test(&self, index: usize) -> bool {
        let (dword_index, bit_index) = Self::position(index);
        (self.bits[dword_index] & (1u64 << bit_index)) != 0
    }

    pub fn toggle(&mut self, index: usize) {
        let (dword_index, bit_index) = Self::position(index);
        self.bits[dword_index] ^= 1u64 << bit_index;
    }

    pub fn set_all(&mut self) {
        self.bits.fill(u64::MAX);
    }

    pub fn clear_all(&mut self) {
        self.bits.fill(0);
    }

    pub fn count_ones(&self) -> usize {
        self.bits.iter().map(|b| b.count_ones() as usize).sum()
    }

    pub fn count_zeros(&self) -> usize {
        self.capacity() - self.count_ones()
    }

    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|&b| b == 0)
    }

    pub fn is_full(&self) -> bool {
        self.bits.iter().all(|&b| b == u64::MAX)
    }

    pub fn find_first_zero(&self) -> Option<usize> {
        for (dword_index, &dword) in self.bits.iter().enumerate() {
            if dword != u64::MAX {
                // available bit exists in this dword.
                let bit_index = (!dword).trailing_zeros() as usize;
                return Some(dword_index * 64 + bit_index);
            }
        }
        None
    }

    pub fn find_and_set_first_zero(&mut self) -> Option<usize> {
        for (dword_index, dword) in self.bits.iter_mut().enumerate() {
            if *dword != u64::MAX {
                // available bit exists in this dword.
                let bit_index = (!*dword).trailing_zeros() as usize;
                *dword |= 1u64 << bit_index;
                return Some(dword_index * 64 + bit_index);
            }
        }
        None
    }

    pub fn find_first_zero_from(&self, start_index: usize) -> Option<usize> {
        assert!(
            start_index < self.capacity(),
            "start_index out of bounds, which possibly indicates a bug in caller"
        );

        let (start_dword_index, start_bit_index) = Self::position(start_index);

        // first dword should be handled separately since we need to ignore bits before
        // start_bit_index.
        let mask = (1u64 << start_bit_index) - 1;
        let masked_dword = self.bits[start_dword_index] | mask;
        if masked_dword != u64::MAX {
            let bit_index = masked_dword.trailing_ones() as usize;
            return Some(start_dword_index * 64 + bit_index);
        }

        for dword_index in (start_dword_index + 1)..NUM_DWORDS {
            let dword = self.bits[dword_index];
            if dword != u64::MAX {
                // available bit exists in this dword.
                let bit_index = dword.trailing_ones() as usize;
                return Some(dword_index * 64 + bit_index);
            }
        }

        None
    }

    pub fn find_and_set_first_zero_from(&mut self, start_index: usize) -> Option<usize> {
        let index = self.find_first_zero_from(start_index)?;
        self.set(index);
        Some(index)
    }

    pub fn find_first_one(&self) -> Option<usize> {
        for (dword_index, &dword) in self.bits.iter().enumerate() {
            if dword != 0 {
                // set bit exists in this dword.
                let bit_index = dword.trailing_zeros() as usize;
                return Some(dword_index * 64 + bit_index);
            }
        }
        None
    }

    pub fn find_and_clear_first_one(&mut self) -> Option<usize> {
        for (dword_index, dword) in self.bits.iter_mut().enumerate() {
            if *dword != 0 {
                // set bit exists in this dword.
                let bit_index = dword.trailing_zeros() as usize;
                *dword &= !(1u64 << bit_index);
                return Some(dword_index * 64 + bit_index);
            }
        }
        None
    }
}
