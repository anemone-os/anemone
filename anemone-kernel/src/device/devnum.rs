//! Device number namespace and minor-number allocation helpers.
//!
//! Reference:
//! - https://www.kernel.org/doc/Documentation/admin-guide/devices.txt

use core::fmt::Display;

use idalloc::{IdAllocatorWithReserve, IdentityBijection, OneShotAllocWithReserve};

use crate::prelude::*;

pub const MAJOR_BITS: usize = 16;
pub const MINOR_BITS: usize = 16;

pub const UNNAMED_MAJOR: usize = 0;

pub mod char {
    pub mod major {
        pub const MEMORY: usize = 1;
        pub const TTY: usize = 4;
        pub const TTY_AUX: usize = 5;
        pub const MISC: usize = 10;
    }
    pub mod minor {
        pub const NULL: usize = 3;
        pub const ZERO: usize = 5;
        pub const FULL: usize = 7;
        pub const RANDOM: usize = 8;
        pub const URANDOM: usize = 9;
        pub const CONSOLE: usize = 1;
    }
}

pub mod block {
    pub mod major {
        pub const RAMDISK: usize = 1;
        pub const LOOP: usize = 7;
        pub const SCSI: usize = 8;
        pub const MMC: usize = 179;
        pub const VIRTIO: usize = 2048;
    }
    pub mod minor {
        pub const INITRD: usize = 0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Opaque)]
pub struct MajorNum(usize);

impl MajorNum {
    pub const fn new(x: usize) -> Self {
        assert!(x < (1 << MAJOR_BITS));
        Self(x)
    }

    pub const fn get(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Opaque)]
pub struct MinorNum(usize);

impl MinorNum {
    pub const fn new(x: usize) -> Self {
        assert!(x < (1 << MINOR_BITS));
        Self(x)
    }

    pub const fn get(self) -> usize {
        self.0
    }
}

macro_rules! gen_devnum {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name {
            major: MajorNum,
            minor: MinorNum,
        }
        impl Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "{}:{}", self.major.get(), self.minor.get())
            }
        }
        impl $name {
            pub const fn new(major: MajorNum, minor: MinorNum) -> Self {
                assert!(major.get() < (1 << MAJOR_BITS));
                assert!(minor.get() < (1 << MINOR_BITS));

                Self { major, minor }
            }

            pub fn raw(&self) -> usize {
                (self.major.get() << MINOR_BITS) | self.minor.get()
            }

            pub fn decompose(&self) -> (MajorNum, MinorNum) {
                (self.major, self.minor)
            }

            pub fn major(&self) -> MajorNum {
                self.major
            }

            pub fn minor(&self) -> MinorNum {
                self.minor
            }
        }
    };
}

gen_devnum!(CharDevNum);
gen_devnum!(BlockDevNum);

impl From<u64> for MajorNum {
    fn from(value: u64) -> Self {
        debug_assert!(value < (1 << MAJOR_BITS) as u64);
        Self(value as usize)
    }
}

impl Into<u64> for MajorNum {
    fn into(self) -> u64 {
        self.get() as u64
    }
}

impl From<u64> for MinorNum {
    fn from(value: u64) -> Self {
        debug_assert!(value < (1 << MINOR_BITS) as u64);
        Self(value as usize)
    }
}

impl Into<u64> for MinorNum {
    fn into(self) -> u64 {
        self.get() as u64
    }
}

/// If your driver has no special requirements for minor number allocation, use
/// this.
pub struct GeneralMinorAllocator(
    IdAllocatorWithReserve<OneShotAllocWithReserve, IdentityBijection<MinorNum>>,
);

impl GeneralMinorAllocator {
    pub fn new() -> Self {
        use devnum::MINOR_BITS;
        Self(IdAllocatorWithReserve::new(OneShotAllocWithReserve::new(
            0,
            (1 << MINOR_BITS) as u64,
        )))
    }

    pub fn alloc(&mut self) -> Option<MinorNum> {
        self.0.alloc()
    }

    pub fn dealloc(&mut self, minor: MinorNum) {
        self.0.dealloc(minor);
    }

    pub fn try_reserve(&mut self, minor: MinorNum) -> Result<(), ()> {
        self.0.try_reserve(minor)
    }
}

#[cfg(feature = "kunit")]
mod kunits {
    use super::*;

    #[kunit]
    fn static_major_namespaces_do_not_overlap() {
        let char_majors = [
            char::major::MEMORY,
            char::major::TTY,
            char::major::TTY_AUX,
            char::major::MISC,
        ];
        let block_majors = [
            block::major::RAMDISK,
            block::major::LOOP,
            block::major::SCSI,
            block::major::MMC,
            block::major::VIRTIO,
        ];

        for (idx, major) in char_majors.iter().enumerate() {
            assert!(!char_majors[idx + 1..].contains(major));
        }
        for (idx, major) in block_majors.iter().enumerate() {
            assert!(!block_majors[idx + 1..].contains(major));
        }
    }

    #[kunit]
    fn internal_device_key_remains_16_bit_major_and_minor() {
        let devnum = BlockDevNum::new(MajorNum::new(2048), MinorNum::new(0x1234));
        assert_eq!(devnum.raw(), 0x0800_1234);
        assert_eq!(
            devnum.decompose(),
            (MajorNum::new(2048), MinorNum::new(0x1234))
        );
    }
}
