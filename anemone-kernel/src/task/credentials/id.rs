use core::fmt::{Debug, Display};

pub trait UserId: Copy + Eq {
    const ROOT: Self;

    fn get(&self) -> u32;
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uid(u32);

impl Uid {
    pub const ROOT: Self = Self(0);

    #[inline(always)]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }
}

impl UserId for Uid {
    const ROOT: Self = Self::ROOT;

    fn get(&self) -> u32 {
        self.0
    }
}

impl Debug for Uid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("user #{}", self.0))
    }
}

impl Display for Uid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("user #{}", self.0))
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Gid(u32);

impl Gid {
    pub const ROOT: Self = Self(0);

    #[inline(always)]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }
}

impl UserId for Gid {
    const ROOT: Self = Self::ROOT;

    fn get(&self) -> u32 {
        self.0
    }
}

impl Debug for Gid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("group #{}", self.0))
    }
}

impl Display for Gid {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("group #{}", self.0))
    }
}
