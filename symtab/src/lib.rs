//! Symbol table structures and utilities.
//! Layout:
//! - Header - magic and num_symbols
//! - Symbol Entries - [struct SymbolEntry; num_symbols]
//! - String Table - struct StrTable - null-terminated C strings
//! Symbols are sorted by address for efficient lookup.
#![cfg_attr(not(any(feature = "std", test)), no_std)]
#![feature(ptr_metadata)]
use core::ffi::{c_char, CStr};

#[cfg(feature = "builder")]
pub mod builder;

macro_rules! static_assert {
    ($prediction:expr) => {
        static_assert!($prediction, "static assertion failed");
    };
    ($prediction:expr, $msg:expr) => {
        const _: () = assert!($prediction, $msg);
    };
}

#[derive(Clone, Copy, Debug)]
pub struct Symbol<'a> {
    pub address: u64,
    pub name: &'a CStr,
    pub type_: SymbolType,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct SymbolEntry {
    address: u64,
    /// Offset from the start of the StrTable
    name_offset: u32,
    type_: u8,
}
static_assert!(size_of::<SymbolEntry>() == 16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SymbolType {
    Text = 0,
    Rodata = 1,
    Data = 2,
    Bss = 3,
    WeakRef = 4,
    WeakObj = 5,
    Unknown = 255,
}

impl From<u8> for SymbolType {
    fn from(value: u8) -> Self {
        match value {
            0 => SymbolType::Text,
            1 => SymbolType::Rodata,
            2 => SymbolType::Data,
            3 => SymbolType::Bss,
            4 => SymbolType::WeakRef,
            5 => SymbolType::WeakObj,
            _ => SymbolType::Unknown,
        }
    }
}

#[derive(Debug)]
#[repr(C)]
pub struct SymbolTable {
    magic: u64,
    num_symbols: u64,
    /// [SymbolEntry; num_symbols] and StrTable
    raw: [u8],
}

/// A transparent wrapper around the string table.
/// Users should not assume any particular implementation details.
/// Currently, it is just a byte array of null-terminated C strings.
#[derive(Debug)]
#[repr(C)]
struct StrTable {
    strs: [c_char],
}

#[derive(Debug)]
pub struct SymbolsIterator<'a> {
    table: &'a SymbolTable,
    index: usize,
}

impl<'a> Iterator for SymbolsIterator<'a> {
    type Item = Symbol<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.table.num_symbols() {
            return None;
        }
        let entry = &self.table.entries()[self.index];
        let strtab = self.table.strtab();
        let name = unsafe { strtab.get_str(entry.name_offset) };
        self.index += 1;
        Some(Symbol {
            address: entry.address,
            name,
            type_: SymbolType::from(entry.type_),
        })
    }
}

impl<'a> DoubleEndedIterator for SymbolsIterator<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.index == 0 {
            return None;
        }
        self.index -= 1;
        let entry = &self.table.entries()[self.index];
        let strtab = self.table.strtab();
        let name = unsafe { strtab.get_str(entry.name_offset) };
        Some(Symbol {
            address: entry.address,
            name,
            type_: SymbolType::from(entry.type_),
        })
    }
}

impl SymbolTable {
    pub const MAGIC: u64 = 0xdead_beef_cafe_babe;

    /// Construct a SymbolTable from a raw pointer.
    /// Returns None if:
    /// - The magic number does not match. Any more checks will not be
    ///   performed.
    /// - The number of symbols is zero.
    /// Safety
    /// - `ptr` must point to a valid symbol table in memory, whose layout must
    ///   match the expected, otherwise undefined behavior can occur.
    pub unsafe fn from_ptr<'a>(ptr: *const u8) -> Option<&'a SymbolTable> {
        unsafe {
            let magic = *(ptr as *const u64);
            if magic != Self::MAGIC {
                return None;
            }
            let num_symbols = *(ptr as *const u64).add(1);

            if num_symbols == 0 {
                return None;
            }

            let entries_size = size_of::<SymbolEntry>() * (num_symbols as usize);
            let strtab_ptr = ptr.add(size_of::<u64>() * 2 + entries_size) as *const c_char;
            let final_symbol_ptr = ptr
                .add(size_of::<u64>() * 2 + entries_size - size_of::<SymbolEntry>())
                as *const SymbolEntry;
            let final_name_offset = (*final_symbol_ptr).name_offset as usize;
            let final_name_ptr = strtab_ptr.add(final_name_offset);
            let final_name_len = {
                let mut len = 0;
                while *final_name_ptr.add(len) != 0 {
                    len += 1;
                }
                len
            };
            let total_size =
                Self::header_size() + entries_size + final_name_offset + final_name_len + 1; // null terminator
            let raw_ptr = core::ptr::from_raw_parts(ptr, total_size - Self::header_size());
            Some(&*raw_ptr)
        }
    }

    pub fn num_symbols(&self) -> usize {
        self.num_symbols as usize
    }

    pub fn lookup(&self, address: u64) -> Option<Symbol<'_>> {
        let symbols = self.entries();
        let idx = match symbols.binary_search_by_key(&address, |e| e.address) {
            Ok(i) => i,
            Err(0) => return None,
            Err(i) => i - 1,
        };

        let entry = &symbols[idx];
        let strtab = self.strtab();
        let name = unsafe { strtab.get_str(entry.name_offset) };

        Some(Symbol {
            address: entry.address,
            name,
            type_: SymbolType::from(entry.type_),
        })
    }

    pub fn iter(&self) -> SymbolsIterator<'_> {
        SymbolsIterator {
            table: self,
            index: 0,
        }
    }
}

impl<'a> IntoIterator for &'a SymbolTable {
    type Item = Symbol<'a>;
    type IntoIter = SymbolsIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl SymbolTable {
    const fn header_size() -> usize {
        size_of::<u64>() * 2
    }

    fn entries(&self) -> &[SymbolEntry] {
        unsafe {
            let ptr =
                (self as *const Self as *const u8).add(size_of::<u64>() * 2) as *const SymbolEntry;
            core::slice::from_raw_parts(ptr, self.num_symbols())
        }
    }
    fn strtab(&self) -> &StrTable {
        unsafe {
            let entries_len = size_of::<SymbolEntry>() * self.num_symbols();
            let len = self.raw.len() - entries_len;
            let ptr = (self as *const Self as *const u8).add(Self::header_size() + entries_len);
            let raw_ptr = core::ptr::from_raw_parts(ptr, len);
            &*raw_ptr
        }
    }
}

impl StrTable {
    /// Get a CStr from the string table at the given offset.
    /// Safety
    /// - `offset` must be a valid offset within the string table. (i.e.
    ///   retrieved from a SymbolEntry)
    unsafe fn get_str(&self, offset: u32) -> &CStr {
        unsafe {
            debug_assert!((offset as usize) < self.strs.len());
            let ptr = (self as *const Self as *const u8).add(offset as usize) as *const c_char;
            CStr::from_ptr(ptr)
        }
    }
}

#[cfg(test)]
mod tests {}
