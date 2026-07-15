//! Task nice-value domain.

/// A valid task nice value.
///
/// This type owns the internal `[-20, 19]` invariant. Linux ABI callers may
/// explicitly clamp with [`Nice::clamp`]; internal callers should use
/// [`Nice::new`], which exposes an internal bug immediately on invalid input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Nice(i8);

impl Nice {
    pub const MIN: Self = Self(-20);
    pub const MAX: Self = Self(19);
    pub const ZERO: Self = Self(0);
    pub const WIDTH: usize = (Self::MAX.0 - Self::MIN.0 + 1) as usize;

    pub const fn new(value: i32) -> Self {
        assert!(
            value >= Self::MIN.0 as i32 && value <= Self::MAX.0 as i32,
            "nice value is outside [-20, 19]"
        );
        Self(value as i8)
    }

    /// Clamp a Linux ABI nice argument into the supported nice domain.
    pub const fn clamp(value: i32) -> Self {
        if value < Self::MIN.0 as i32 {
            Self::MIN
        } else if value > Self::MAX.0 as i32 {
            Self::MAX
        } else {
            Self(value as i8)
        }
    }

    pub const fn get(self) -> i8 {
        self.0
    }

    pub(crate) const fn table_index(self) -> usize {
        (self.0 - Self::MIN.0) as usize
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::Nice;
    use crate::prelude::*;

    #[kunit]
    fn test_nice_internal_constructor_and_index() {
        assert_eq!(Nice::new(-20), Nice::MIN);
        assert_eq!(Nice::new(0), Nice::ZERO);
        assert_eq!(Nice::new(19), Nice::MAX);
        assert_eq!(Nice::MIN.table_index(), 0);
        assert_eq!(Nice::ZERO.table_index(), 20);
        assert_eq!(Nice::MAX.table_index(), Nice::WIDTH - 1);
    }

    #[kunit]
    fn test_nice_linux_abi_clamp() {
        assert_eq!(Nice::clamp(i32::MIN), Nice::MIN);
        assert_eq!(Nice::clamp(-20), Nice::MIN);
        assert_eq!(Nice::clamp(0), Nice::ZERO);
        assert_eq!(Nice::clamp(19), Nice::MAX);
        assert_eq!(Nice::clamp(i32::MAX), Nice::MAX);
    }
}
