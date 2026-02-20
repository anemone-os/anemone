#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct AlignedAddr<const ALIGN: usize>(usize);

impl<const ALIGN: usize> AlignedAddr<ALIGN> {
    pub const ZERO: Self = Self(0);

    pub const fn new(addr: usize) -> Option<Self> {
        if addr % ALIGN == 0 {
            Some(Self(addr))
        } else {
            None
        }
    }

    pub unsafe fn new_unchecked(addr: usize) -> Self {
        Self(addr)
    }

    pub const fn align_up(addr: usize) -> Self {
        Self((addr + ALIGN - 1) & !(ALIGN - 1))
    }

    pub const fn align_down(addr: usize) -> Self {
        Self(addr & !(ALIGN - 1))
    }

    pub const fn as_usize(&self) -> usize {
        self.0
    }

    pub const fn cast<const NEW_ALIGN: usize>(&self) -> Option<AlignedAddr<NEW_ALIGN>> {
        if self.0 % NEW_ALIGN == 0 {
            Some(AlignedAddr::<NEW_ALIGN>(self.0))
        } else {
            None
        }
    }

    pub const unsafe fn cast_unchecked<const NEW_ALIGN: usize>(&self) -> AlignedAddr<NEW_ALIGN> {
        AlignedAddr::<NEW_ALIGN>(self.0)
    }
}

impl<const ALIGN: usize> From<AlignedAddr<ALIGN>> for usize {
    fn from(addr: AlignedAddr<ALIGN>) -> Self {
        addr.0
    }
}

impl<const ALIGN: usize> AsRef<usize> for AlignedAddr<ALIGN> {
    fn as_ref(&self) -> &usize {
        &self.0
    }
}

impl<const ALIGN: usize> core::fmt::Display for AlignedAddr<ALIGN> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "0x{:x}", self.0)
    }
}
