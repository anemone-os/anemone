#![doc = include_str!("../README.md")]
#![cfg_attr(not(test), no_std)]

mod parser;
mod unflattened;
pub use parser::FdtParser;
pub use unflattened::{
    DeviceNode, DeviceStatus, DeviceTreeHandle, PropEncodedArray, Property, RangesEncoding,
    RegEncoding, StringList, U32ArrayEncoding, U64ArrayEncoding,
};

pub mod endian {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Be32(u32);
    impl Be32 {
        pub const fn to_host(self) -> u32 {
            u32::from_be(self.0)
        }
    }
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Be64(u64);
    impl Be64 {
        pub const fn to_host(self) -> u64 {
            u64::from_be(self.0)
        }
    }
}
use endian::*;

#[inline]
fn align_up(v: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (v + align - 1) & !(align - 1)
}

/// FDT header. All fields are in big-endian format.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct FdtHeader {
    pub magic: Be32,
    pub total_size: Be32,
    pub off_dt_struct: Be32,
    pub off_dt_strings: Be32,
    pub off_mem_rsvmap: Be32,
    pub version: Be32,
    pub last_comp_version: Be32,
    pub boot_cpuid_phys: Be32,
    pub size_dt_strings: Be32,
    pub size_dt_struct: Be32,
}

#[derive(Debug)]
#[repr(C)]
pub struct FdtReserveEntry {
    pub address: Be64,
    pub size: Be64,
}

#[cfg(test)]
mod tests {
    use std::{alloc::Layout, ptr::NonNull};

    use crate::parser::FdtParser;

    #[test]
    fn parse_dtb() {
        let fdt_blob: &[u8] = include_bytes!("../testfiles/qemu-virt-rv64.dtb");
        let word_aligned_fdt = unsafe {
            let layout = core::alloc::Layout::from_size_align(fdt_blob.len(), 8).unwrap();
            let ptr = std::alloc::alloc(layout);
            core::ptr::copy_nonoverlapping(fdt_blob.as_ptr(), ptr, fdt_blob.len());
            core::slice::from_raw_parts_mut(ptr, fdt_blob.len())
        };
        let parser = unsafe { FdtParser::new(word_aligned_fdt.as_ptr().cast()) };
        let mut unflattened_layout = Layout::new::<()>();
        let dt_handle = parser.parse(|layout| {
            unflattened_layout = layout;
            let ptr = unsafe { std::alloc::alloc(layout) };
            NonNull::new(core::ptr::slice_from_raw_parts_mut(ptr, layout.size()))
        });

        // print some info to verify the parsing result
        for node in dt_handle.all_nodes() {
            eprintln!("name: {} unit: {:?}", node.name(), node.unit_addr());
        }

        unsafe {
            std::alloc::dealloc(
                word_aligned_fdt.as_mut_ptr(),
                core::alloc::Layout::from_size_align(word_aligned_fdt.len(), 8).unwrap(),
            );
            std::alloc::dealloc(dt_handle.arena.as_ptr().cast::<u8>(), unflattened_layout);
        }
    }
}
