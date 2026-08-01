//! Device number namespace and minor-number allocation helpers.
//!
//! Reference:
//! - https://www.kernel.org/doc/Documentation/admin-guide/devices.txt

use core::fmt::Display;

use idalloc::{IdAllocatorWithReserve, IdentityBijection, OneShotAllocWithReserve};

use crate::prelude::*;

pub const MAJOR_BITS: usize = 12;
pub const MINOR_BITS: usize = 20;

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

/// Category-neutral device number stored by generic inode metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceNumber {
    major: MajorNum,
    minor: MinorNum,
}

impl DeviceNumber {
    pub const fn new(major: MajorNum, minor: MinorNum) -> Self {
        Self { major, minor }
    }

    pub const fn decompose(self) -> (MajorNum, MinorNum) {
        (self.major, self.minor)
    }

    pub const fn major(self) -> MajorNum {
        self.major
    }

    pub const fn minor(self) -> MinorNum {
        self.minor
    }
}

impl Display for DeviceNumber {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}:{}", self.major.get(), self.minor.get())
    }
}

macro_rules! gen_typed_devnum {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(DeviceNumber);

        impl $name {
            pub const fn new(major: MajorNum, minor: MinorNum) -> Self {
                Self(DeviceNumber::new(major, minor))
            }

            pub const fn number(self) -> DeviceNumber {
                self.0
            }

            pub const fn decompose(self) -> (MajorNum, MinorNum) {
                self.0.decompose()
            }

            pub const fn major(self) -> MajorNum {
                self.0.major()
            }

            pub const fn minor(self) -> MinorNum {
                self.0.minor()
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                Display::fmt(&self.0, f)
            }
        }

        impl From<DeviceNumber> for $name {
            fn from(number: DeviceNumber) -> Self {
                Self(number)
            }
        }

        impl From<$name> for DeviceNumber {
            fn from(devnum: $name) -> Self {
                devnum.number()
            }
        }
    };
}

gen_typed_devnum!(CharDevNum);
gen_typed_devnum!(BlockDevNum);

impl From<u64> for MajorNum {
    fn from(value: u64) -> Self {
        assert!(value < (1 << MAJOR_BITS) as u64);
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
        assert!(value < (1 << MINOR_BITS) as u64);
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
    fn device_number_uses_linux_12_20_domain() {
        let devnum = DeviceNumber::new(
            MajorNum::new((1 << MAJOR_BITS) - 1),
            MinorNum::new((1 << MINOR_BITS) - 1),
        );
        assert_eq!(
            devnum.decompose(),
            (
                MajorNum::new((1 << MAJOR_BITS) - 1),
                MinorNum::new((1 << MINOR_BITS) - 1)
            )
        );
    }

    #[kunit]
    fn typed_keys_explicitly_wrap_the_common_number() {
        let number = DeviceNumber::new(MajorNum::new(2048), MinorNum::new(0x12345));
        let char_dev = CharDevNum::from(number);
        let block_dev = BlockDevNum::from(number);

        assert_eq!(char_dev.number(), number);
        assert_eq!(block_dev.number(), number);
        assert_eq!(DeviceNumber::from(char_dev), number);
        assert_eq!(DeviceNumber::from(block_dev), number);
    }

    #[kunit]
    fn linux_dev_t_codec_roundtrips_representative_and_boundary_numbers() {
        use anemone_abi::fs::linux::dev_t;

        let numbers = [
            DeviceNumber::new(MajorNum::new(0), MinorNum::new(0)),
            DeviceNumber::new(MajorNum::new(1), MinorNum::new(3)),
            DeviceNumber::new(MajorNum::new(2048), MinorNum::new(0x12345)),
            DeviceNumber::new(
                MajorNum::new((1 << MAJOR_BITS) - 1),
                MinorNum::new((1 << MINOR_BITS) - 1),
            ),
        ];

        for number in numbers {
            let (major, minor) = number.decompose();
            let encoded = dev_t::encode(major.get() as u32, minor.get() as u32);
            assert_eq!(
                dev_t::decode(encoded),
                (major.get() as u32, minor.get() as u32)
            );
        }
    }
}
