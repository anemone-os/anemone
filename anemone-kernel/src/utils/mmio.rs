use core::ptr::NonNull;
use safe_mmio::{
    UniqueMmioPointer, field,
    fields::{ReadOnly, ReadPure, ReadPureWrite, ReadWrite, WriteOnly},
};

macro_rules! define_combined {
    (read, $type: ident, $name: ident) => {
        paste::paste! {
            #[repr(C)]
            struct [<$name Fields>]{
                lower: $type<u32>,
                upper: $type<u32>,
            }
            pub struct $name<'a>{
                fields: UniqueMmioPointer<'a, [<$name Fields>]>,
            }

            impl<'a> $name<'a>{
                pub unsafe fn new(address: usize) -> Option<Self> {
                    unsafe {
                        Some(Self {
                            fields: UniqueMmioPointer::new(NonNull::new(address as _)?),
                        })
                    }
                }
                pub fn read_lower(&mut self) -> u32 {
                    field!(self.fields, lower).read()
                }
                pub fn read_higher(&mut self) -> u32 {
                    field!(self.fields, upper).read()
                }
                pub fn read(&mut self) -> u64 {
                    let lower = self.read_lower() as u64;
                    let higher = self.read_higher() as u64;
                    (higher << 32) | lower
                }
                pub fn read_bit(&mut self, bit: usize) -> bool {
                    assert!(bit < 64);
                    if bit < 32 {
                        self.read_lower() & (1 << bit) != 0
                    } else {
                        self.read_higher() & (1 << (bit - 32)) != 0
                    }
                }
            }
        }
    };
    (rw, $type: ident, $name: ident) => {
        paste::paste! {
            #[repr(C)]
            struct [<$name Fields>]{
                lower: $type<u32>,
                upper: $type<u32>,
            }
            pub struct $name<'a>{
                fields: UniqueMmioPointer<'a, [<$name Fields>]>,
            }

            impl<'a> $name<'a>{
                pub unsafe fn new(address: usize) -> Option<Self> {
                    unsafe {
                        Some(Self {
                            fields: UniqueMmioPointer::new(NonNull::new(address as _)?),
                        })
                    }
                }
                pub fn read_lower(&mut self) -> u32 {
                    field!(self.fields, lower).read()
                }
                pub fn read_higher(&mut self) -> u32 {
                    field!(self.fields, upper).read()
                }

                pub fn read(&mut self) -> u64 {
                    let lower = self.read_lower() as u64;
                    let higher = self.read_higher() as u64;
                    (higher << 32) | lower
                }
                pub fn read_bit(&mut self, bit: usize) -> bool {
                    assert!(bit < 64);
                    if bit < 32 {
                        self.read_lower() & (1 << bit) != 0
                    } else {
                        self.read_higher() & (1 << (bit - 32)) != 0
                    }
                }

                pub fn write_lower(&mut self, value: u32) {
                    field!(self.fields, lower).write(value)
                }
                pub fn write_higher(&mut self, value: u32) {
                    field!(self.fields, upper).write(value)
                }

                pub unsafe fn write(&mut self, value: u64) {
                    let lower = value as u32;
                    let higher = (value >> 32) as u32;
                    self.write_lower(lower);
                    self.write_higher(higher);
                }

                pub fn write_bit(&mut self, bit: usize, set: bool) {
                    assert!(bit < 64);
                    if bit < 32 {
                        let mut val = self.read_lower();
                        if set {
                            val |= 1 << bit;
                        } else {
                            val &= !(1 << bit);
                        }
                        self.write_lower(val);
                    } else {
                        let mut val = self.read_higher();
                        if set {
                            val |= 1 << (bit - 32);
                        } else {
                            val &= !(1 << (bit - 32));
                        }
                        self.write_higher(val);
                    }
                }
            }
        }
    };

    (write, $type: ident, $name: ident) => {
        paste::paste! {
            #[repr(C)]
            struct [<$name Fields>]{
                lower: $type<u32>,
                upper: $type<u32>,
            }
            pub struct $name<'a>{
                fields: UniqueMmioPointer<'a, [<$name Fields>]>,
            }

            impl<'a> $name<'a>{
                pub unsafe fn new(address: usize) -> Option<Self> {
                    unsafe {
                        Some(Self {
                            fields: UniqueMmioPointer::new(NonNull::new(address as _)?),
                        })
                    }
                }
                pub fn write_lower(&mut self, value: u32) {
                    field!(self.fields, lower).write(value)
                }
                pub fn write_higher(&mut self, value: u32) {
                    field!(self.fields, upper).write(value)
                }

                pub unsafe fn write(&mut self, value: u64) {
                    let lower = value as u32;
                    let higher = (value >> 32) as u32;
                    self.write_lower(lower);
                    self.write_higher(higher);
                }
            }
        }
    };
}

define_combined!(read, ReadOnly, CombinedReadOnly);
define_combined!(read, ReadPure, CombinedReadPure);
define_combined!(rw, ReadPureWrite, CombinedReadPureWrite);
define_combined!(rw, ReadWrite, CombinedReadWrite);
define_combined!(write, WriteOnly, CombinedWriteOnly);
