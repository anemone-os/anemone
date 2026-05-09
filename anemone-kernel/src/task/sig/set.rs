use crate::task::sig::SigNo;

/// Bitmask of signals. Each bit represents a signal, 0-63.
///
/// TODO: abstract this to a more general bitmap type?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SigSet(u64);

// intra-set operations.
impl SigSet {
    /// Create an empty signal mask.
    pub const fn new() -> Self {
        Self(0)
    }

    /// Create a signal mask with the given raw value.
    pub const fn new_with_mask(mask: u64) -> Self {
        Self(mask)
    }

    /// Whether the signal set is empty.
    pub const fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// Set the bit of the given [SigNo], and return the old value of that bit.
    pub const fn set(&mut self, signo: SigNo) -> bool {
        let bit = 1u64 << signo.as_usize();
        let old = (self.0 & bit) != 0;
        self.0 |= bit;
        old
    }

    /// Clear the bit of the given [SigNo], and return the old value of that
    /// bit.
    pub const fn clear(&mut self, signo: SigNo) -> bool {
        let bit = 1u64 << signo.as_usize();
        let old = (self.0 & bit) != 0;
        self.0 &= !bit;
        old
    }

    /// Get the value of the bit of the given [SigNo].
    pub const fn get(&self, signo: SigNo) -> bool {
        let bit = 1u64 << signo.as_usize();
        (self.0 & bit) != 0
    }

    /// Get the raw value of the signal mask.
    pub const fn as_u64(&self) -> u64 {
        self.0
    }
}

// inter-set operations.
impl SigSet {
    pub const fn union(&self, other: &Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn union_with(&mut self, other: &Self) {
        self.0 |= other.0;
    }

    pub const fn intersection(&self, other: &Self) -> Self {
        Self(self.0 & other.0)
    }

    pub const fn intersection_with(&mut self, other: &Self) {
        self.0 &= other.0;
    }

    pub const fn difference(&self, other: &Self) -> Self {
        Self(self.0 & !other.0)
    }

    pub const fn difference_with(&mut self, other: &Self) {
        self.0 &= !other.0;
    }
}
