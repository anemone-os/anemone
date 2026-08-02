//! Checked, versioned symbol-table format shared by the kernel and xtask.
//!
//! The byte format is deliberately independent of Rust layout. All integers
//! are little-endian and every offset is validated before a view is returned.
#![cfg_attr(not(any(feature = "std", test)), no_std)]

use core::{fmt, str};

#[cfg(feature = "builder")]
pub mod builder;

pub const MAGIC: [u8; 8] = *b"ANEMSYMB";
pub const VERSION: u16 = 1;
pub const HEADER_SIZE: usize = 32;
pub const ENTRY_SIZE: usize = 24;

const VERSION_OFFSET: usize = 8;
const HEADER_SIZE_OFFSET: usize = 10;
const TOTAL_SIZE_OFFSET: usize = 12;
const ENTRY_COUNT_OFFSET: usize = 16;
const STRINGS_OFFSET_OFFSET: usize = 20;
const RESERVED_OFFSET: usize = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    Truncated,
    BadMagic,
    UnsupportedVersion,
    BadHeaderSize,
    BadTotalSize,
    BadStringsOffset,
    ReservedField,
    ZeroSizedSymbol,
    AddressOverflow,
    UnsortedSymbols,
    BadNameRange,
    EmptyName,
    InvalidUtf8,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid symbol table: {self:?}")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ParseError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Symbol<'a> {
    pub start: u64,
    pub size: u64,
    pub name: &'a str,
}

impl Symbol<'_> {
    pub fn contains(&self, address: u64) -> bool {
        self.start <= address
            && self
                .start
                .checked_add(self.size)
                .is_some_and(|end| address < end)
    }
}

/// A validated, allocation-free view of an encoded symbol table.
#[derive(Clone, Copy, Debug)]
pub struct SymbolTable<'a> {
    bytes: &'a [u8],
    entry_count: usize,
    strings_offset: usize,
}

impl<'a> SymbolTable<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ParseError> {
        if bytes.len() < HEADER_SIZE {
            return Err(ParseError::Truncated);
        }
        if bytes[..MAGIC.len()] != MAGIC {
            return Err(ParseError::BadMagic);
        }
        if read_u16(bytes, VERSION_OFFSET)? != VERSION {
            return Err(ParseError::UnsupportedVersion);
        }
        if usize::from(read_u16(bytes, HEADER_SIZE_OFFSET)?) != HEADER_SIZE {
            return Err(ParseError::BadHeaderSize);
        }
        let total_size = usize::try_from(read_u32(bytes, TOTAL_SIZE_OFFSET)?)
            .map_err(|_| ParseError::BadTotalSize)?;
        if total_size != bytes.len() {
            return Err(ParseError::BadTotalSize);
        }
        if read_u64(bytes, RESERVED_OFFSET)? != 0 {
            return Err(ParseError::ReservedField);
        }

        let entry_count = usize::try_from(read_u32(bytes, ENTRY_COUNT_OFFSET)?)
            .map_err(|_| ParseError::Truncated)?;
        let entries_size = entry_count
            .checked_mul(ENTRY_SIZE)
            .ok_or(ParseError::Truncated)?;
        let expected_strings_offset = HEADER_SIZE
            .checked_add(entries_size)
            .ok_or(ParseError::Truncated)?;
        let strings_offset = usize::try_from(read_u32(bytes, STRINGS_OFFSET_OFFSET)?)
            .map_err(|_| ParseError::BadStringsOffset)?;
        if strings_offset != expected_strings_offset || strings_offset > bytes.len() {
            return Err(ParseError::BadStringsOffset);
        }

        let table = Self {
            bytes,
            entry_count,
            strings_offset,
        };
        let mut previous_start = None;
        for index in 0..entry_count {
            let symbol = table.symbol(index)?;
            if symbol.size == 0 {
                return Err(ParseError::ZeroSizedSymbol);
            }
            symbol
                .start
                .checked_add(symbol.size)
                .ok_or(ParseError::AddressOverflow)?;
            if previous_start.is_some_and(|start| start >= symbol.start) {
                return Err(ParseError::UnsortedSymbols);
            }
            previous_start = Some(symbol.start);
        }
        Ok(table)
    }

    pub fn len(&self) -> usize {
        self.entry_count
    }

    pub fn is_empty(&self) -> bool {
        self.entry_count == 0
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn iter(&self) -> Symbols<'a> {
        Symbols {
            table: *self,
            next: 0,
        }
    }

    pub fn lookup(&self, address: u64) -> Option<Symbol<'a>> {
        let mut low = 0;
        let mut high = self.entry_count;
        while low < high {
            let middle = low + (high - low) / 2;
            let symbol = self
                .symbol(middle)
                .expect("validated symbol table entry must remain readable");
            if symbol.start <= address {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        if low == 0 {
            return None;
        }
        let symbol = self
            .symbol(low - 1)
            .expect("validated symbol table entry must remain readable");
        symbol.contains(address).then_some(symbol)
    }

    fn symbol(&self, index: usize) -> Result<Symbol<'a>, ParseError> {
        let entry_offset = HEADER_SIZE
            .checked_add(index.checked_mul(ENTRY_SIZE).ok_or(ParseError::Truncated)?)
            .ok_or(ParseError::Truncated)?;
        let start = read_u64(self.bytes, entry_offset)?;
        let size = read_u64(self.bytes, entry_offset + 8)?;
        let name_offset = usize::try_from(read_u32(self.bytes, entry_offset + 16)?)
            .map_err(|_| ParseError::BadNameRange)?;
        let name_len = usize::try_from(read_u32(self.bytes, entry_offset + 20)?)
            .map_err(|_| ParseError::BadNameRange)?;
        if name_len == 0 {
            return Err(ParseError::EmptyName);
        }
        let name_start = self
            .strings_offset
            .checked_add(name_offset)
            .ok_or(ParseError::BadNameRange)?;
        let name_end = name_start
            .checked_add(name_len)
            .ok_or(ParseError::BadNameRange)?;
        let name_bytes = self
            .bytes
            .get(name_start..name_end)
            .ok_or(ParseError::BadNameRange)?;
        let name = str::from_utf8(name_bytes).map_err(|_| ParseError::InvalidUtf8)?;
        Ok(Symbol { start, size, name })
    }
}

impl<'a> IntoIterator for &'a SymbolTable<'a> {
    type Item = Symbol<'a>;
    type IntoIter = Symbols<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Symbols<'a> {
    table: SymbolTable<'a>,
    next: usize,
}

impl<'a> Iterator for Symbols<'a> {
    type Item = Symbol<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next == self.table.entry_count {
            return None;
        }
        let symbol = self
            .table
            .symbol(self.next)
            .expect("validated symbol table entry must remain readable");
        self.next += 1;
        Some(symbol)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.table.entry_count - self.next;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for Symbols<'_> {}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, ParseError> {
    let raw: [u8; 2] = bytes
        .get(offset..offset + 2)
        .ok_or(ParseError::Truncated)?
        .try_into()
        .map_err(|_| ParseError::Truncated)?;
    Ok(u16::from_le_bytes(raw))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, ParseError> {
    let raw: [u8; 4] = bytes
        .get(offset..offset + 4)
        .ok_or(ParseError::Truncated)?
        .try_into()
        .map_err(|_| ParseError::Truncated)?;
    Ok(u32::from_le_bytes(raw))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, ParseError> {
    let raw: [u8; 8] = bytes
        .get(offset..offset + 8)
        .ok_or(ParseError::Truncated)?
        .try_into()
        .map_err(|_| ParseError::Truncated)?;
    Ok(u64::from_le_bytes(raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::{SymbolInput, encode};

    fn fixture() -> Vec<u8> {
        encode(&[
            SymbolInput::new(0x1000, 0x10, "first"),
            SymbolInput::new(0x1020, 0x08, "second"),
        ])
        .unwrap()
    }

    #[test]
    fn exact_inside_end_and_gap_lookup_are_bounded() {
        let bytes = fixture();
        let table = SymbolTable::parse(&bytes).unwrap();
        assert_eq!(table.lookup(0x1000).unwrap().name, "first");
        assert_eq!(table.lookup(0x100f).unwrap().name, "first");
        assert!(table.lookup(0x0fff).is_none());
        assert!(table.lookup(0x1010).is_none());
        assert!(table.lookup(0x101f).is_none());
        assert_eq!(table.lookup(0x1020).unwrap().name, "second");
        assert!(table.lookup(0x1028).is_none());
    }

    #[test]
    fn empty_table_is_valid() {
        let bytes = encode(&[]).unwrap();
        let table = SymbolTable::parse(&bytes).unwrap();
        assert!(table.is_empty());
        assert!(table.lookup(0).is_none());
    }

    #[test]
    fn malformed_headers_entries_and_strings_are_rejected() {
        let valid = fixture();
        for length in 0..HEADER_SIZE {
            assert!(SymbolTable::parse(&valid[..length]).is_err());
        }
        for length in HEADER_SIZE..valid.len() {
            assert!(SymbolTable::parse(&valid[..length]).is_err());
        }

        let mut bad = valid.clone();
        bad[0] ^= 1;
        assert_eq!(SymbolTable::parse(&bad).unwrap_err(), ParseError::BadMagic);

        let mut bad = valid.clone();
        bad[VERSION_OFFSET..VERSION_OFFSET + 2].copy_from_slice(&2u16.to_le_bytes());
        assert_eq!(
            SymbolTable::parse(&bad).unwrap_err(),
            ParseError::UnsupportedVersion
        );

        let mut bad = valid.clone();
        bad[HEADER_SIZE_OFFSET..HEADER_SIZE_OFFSET + 2].copy_from_slice(&0u16.to_le_bytes());
        assert_eq!(
            SymbolTable::parse(&bad).unwrap_err(),
            ParseError::BadHeaderSize
        );

        let mut bad = valid.clone();
        bad[RESERVED_OFFSET] = 1;
        assert_eq!(
            SymbolTable::parse(&bad).unwrap_err(),
            ParseError::ReservedField
        );

        let mut bad = valid.clone();
        bad[TOTAL_SIZE_OFFSET..TOTAL_SIZE_OFFSET + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            SymbolTable::parse(&bad).unwrap_err(),
            ParseError::BadTotalSize
        );

        let mut bad = valid.clone();
        bad[STRINGS_OFFSET_OFFSET..STRINGS_OFFSET_OFFSET + 4]
            .copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            SymbolTable::parse(&bad).unwrap_err(),
            ParseError::BadStringsOffset
        );

        let mut bad = valid.clone();
        bad[ENTRY_COUNT_OFFSET..ENTRY_COUNT_OFFSET + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            SymbolTable::parse(&bad).unwrap_err(),
            ParseError::BadStringsOffset
        );

        let mut bad = valid.clone();
        let first_name_offset = HEADER_SIZE + 16;
        bad[first_name_offset..first_name_offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            SymbolTable::parse(&bad).unwrap_err(),
            ParseError::BadNameRange
        );

        let mut bad = valid.clone();
        let first_name_len = HEADER_SIZE + 20;
        bad[first_name_len..first_name_len + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            SymbolTable::parse(&bad).unwrap_err(),
            ParseError::BadNameRange
        );

        let mut bad = valid.clone();
        bad[HEADER_SIZE + 2 * ENTRY_SIZE] = 0xff;
        assert_eq!(
            SymbolTable::parse(&bad).unwrap_err(),
            ParseError::InvalidUtf8
        );

        let mut bad = valid.clone();
        let second_start = HEADER_SIZE + ENTRY_SIZE;
        bad[second_start..second_start + 8].copy_from_slice(&0x1000u64.to_le_bytes());
        assert_eq!(
            SymbolTable::parse(&bad).unwrap_err(),
            ParseError::UnsortedSymbols
        );

        let mut bad = valid.clone();
        bad[HEADER_SIZE + 8..HEADER_SIZE + 16].copy_from_slice(&0u64.to_le_bytes());
        assert_eq!(
            SymbolTable::parse(&bad).unwrap_err(),
            ParseError::ZeroSizedSymbol
        );

        let mut bad = valid.clone();
        bad[HEADER_SIZE + 20..HEADER_SIZE + 24].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(SymbolTable::parse(&bad).unwrap_err(), ParseError::EmptyName);

        let mut bad = valid.clone();
        bad[HEADER_SIZE + 8..HEADER_SIZE + 16].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(
            SymbolTable::parse(&bad).unwrap_err(),
            ParseError::AddressOverflow
        );
    }
}
