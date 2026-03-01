use core::ops::Deref;

/// [`Identity`] is a simple wrapper around `heapless::String` that can be used
/// as `name` field or something similar in kernel objects.
///
/// Based on [`heapless::String`], it has a fixed capacity defined by the `LEN`
/// generic parameter, and does not allocate memory on the heap. This makes
/// it suitable for use in kernel, where we want to avoid dynamic memory
/// allocation as much as possible.
///
/// Pay attention that [`Identity`] is commonly created on stack, so it should
/// not be too large.
///
/// `kconfig` defines a `MAX_IDENTITY_LEN` constant that every identity type's
/// length cannot exceed, otherwise it will cause a compile error.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Identity<const LEN: usize>(heapless::String<LEN>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapacityError;

impl core::fmt::Display for CapacityError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Identity capacity exceeded")
    }
}

impl core::error::Error for CapacityError {}

impl<const LEN: usize> Identity<LEN> {
    const __VALIDATE: () = assert!(
        LEN <= crate::kconfig_defs::MAX_IDENT_LEN_BYTES,
        "Identity length cannot exceed MAX_IDENT_LEN_BYTES"
    );

    pub fn new() -> Self {
        let _ = Self::__VALIDATE;
        Self(heapless::String::new())
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub const fn capacity() -> usize {
        LEN
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn push(&mut self, ch: char) -> Result<(), CapacityError> {
        self.0.push(ch).map_err(|_| CapacityError)
    }

    pub fn push_str(&mut self, value: &str) -> Result<(), CapacityError> {
        self.0.push_str(value).map_err(|_| CapacityError)
    }

    pub fn try_from_fmt(args: core::fmt::Arguments<'_>) -> Result<Self, CapacityError> {
        let _ = Self::__VALIDATE;
        let mut s = heapless::String::new();
        core::fmt::write(&mut s, args).map_err(|_| CapacityError)?;
        Ok(Self(s))
    }
}

impl<const LEN: usize> TryFrom<&str> for Identity<LEN> {
    type Error = CapacityError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let _ = Self::__VALIDATE;
        let mut s = heapless::String::new();
        s.push_str(value).map_err(|_| CapacityError)?;
        Ok(Self(s))
    }
}

impl<const LEN: usize> Default for Identity<LEN> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const LEN: usize> core::fmt::Display for Identity<LEN> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<const LEN: usize> AsRef<str> for Identity<LEN> {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl<const LEN: usize> Deref for Identity<LEN> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl<const LEN: usize> core::str::FromStr for Identity<LEN> {
    type Err = CapacityError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

#[macro_export]
macro_rules! ident_format {
    ($($arg:tt)*) => {
        $crate::utils::identity::Identity::<crate::kconfig_defs::MAX_IDENT_LEN_BYTES>::try_from_fmt(core::format_args!($($arg)*))
    };
}
