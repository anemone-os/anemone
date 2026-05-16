use crate::{prelude::*, task::sig::SigNo};

const UNUSED_MASK: u64 = 1u64 << 63;
const VALID_MASK: u64 = !UNUSED_MASK;

const fn bit_of(signo: SigNo) -> u64 {
    1u64 << (signo.as_usize() - 1)
}

/// Bitmask of signals in Linux ABI layout.
///
/// Bit 0 corresponds to signal 1, and bit 62 corresponds to signal 63.
/// Bit 63 is unused.
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
    pub fn new_with_mask(mask: u64) -> Self {
        if mask & UNUSED_MASK != 0 {
            kdebugln!(
                "SigSet::new_with_mask: invalid signal mask with bit 63 set: {:#x}",
                mask
            );
        }

        Self(mask & VALID_MASK)
    }

    /// Create a signal mask with the given signal numbers.
    pub const fn new_with_signos(signos: &[SigNo]) -> Self {
        let mut mask = 0;
        {
            let mut i = 0;
            while i < signos.len() {
                let sig = signos[i];
                mask |= bit_of(sig);
                i += 1;
            }
        }

        Self(mask)
    }

    /// Whether the signal set is empty.
    pub const fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// Set the bit of the given [SigNo], and return the old value of that bit.
    pub const fn set(&mut self, signo: SigNo) -> bool {
        let bit = bit_of(signo);
        let old = (self.0 & bit) != 0;
        self.0 |= bit;
        old
    }

    /// Clear the bit of the given [SigNo], and return the old value of that
    /// bit.
    pub const fn clear(&mut self, signo: SigNo) -> bool {
        let bit = bit_of(signo);
        let old = (self.0 & bit) != 0;
        self.0 &= !bit;
        old
    }

    /// Get the value of the bit of the given [SigNo].
    pub const fn get(&self, signo: SigNo) -> bool {
        let bit = bit_of(signo);
        (self.0 & bit) != 0
    }

    /// Get the raw value of the signal mask.
    pub const fn as_u64(&self) -> u64 {
        self.0
    }

    pub const fn iter(&self) -> SigSetIter {
        SigSetIter {
            set: *self,
            next_bit: 0,
        }
    }
}

pub struct SigSetIter {
    set: SigSet,
    next_bit: usize,
}

impl Iterator for SigSetIter {
    type Item = SigNo;

    fn next(&mut self) -> Option<Self::Item> {
        while self.next_bit < 63 {
            let bit = 1u64 << self.next_bit;
            self.next_bit += 1;
            if (self.set.0 & bit) != 0 {
                return Some(SigNo::new(self.next_bit));
            }
        }
        None
    }
}

impl IntoIterator for SigSet {
    type Item = SigNo;
    type IntoIter = SigSetIter;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
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

    pub const fn contains(&self, other: &Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub const fn complement(&self) -> Self {
        let ret = Self(!self.0);
        Self(ret.0 & VALID_MASK)
    }
}
