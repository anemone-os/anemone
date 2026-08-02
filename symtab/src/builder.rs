//! Host-side checked encoder for the shared symbol-table format.

use std::{error::Error, fmt, vec::Vec};

use crate::{ENTRY_SIZE, HEADER_SIZE, MAGIC, SymbolTable, VERSION};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SymbolInput<'a> {
    pub start: u64,
    pub size: u64,
    pub name: &'a str,
}

impl<'a> SymbolInput<'a> {
    pub const fn new(start: u64, size: u64, name: &'a str) -> Self {
        Self { start, size, name }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodeError {
    TooLarge,
    ZeroSizedSymbol,
    AddressOverflow,
    UnsortedSymbols,
    EmptyName,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot encode symbol table: {self:?}")
    }
}

impl Error for EncodeError {}

pub fn encode(symbols: &[SymbolInput<'_>]) -> Result<Vec<u8>, EncodeError> {
    let entry_count = u32::try_from(symbols.len()).map_err(|_| EncodeError::TooLarge)?;
    let entries_size = symbols
        .len()
        .checked_mul(ENTRY_SIZE)
        .ok_or(EncodeError::TooLarge)?;
    let strings_offset = HEADER_SIZE
        .checked_add(entries_size)
        .ok_or(EncodeError::TooLarge)?;
    let mut strings_size = 0usize;
    let mut previous_start = None;
    for symbol in symbols {
        if symbol.size == 0 {
            return Err(EncodeError::ZeroSizedSymbol);
        }
        symbol
            .start
            .checked_add(symbol.size)
            .ok_or(EncodeError::AddressOverflow)?;
        if previous_start.is_some_and(|start| start >= symbol.start) {
            return Err(EncodeError::UnsortedSymbols);
        }
        if symbol.name.is_empty() {
            return Err(EncodeError::EmptyName);
        }
        strings_size = strings_size
            .checked_add(symbol.name.len())
            .ok_or(EncodeError::TooLarge)?;
        previous_start = Some(symbol.start);
    }
    let total_size = strings_offset
        .checked_add(strings_size)
        .ok_or(EncodeError::TooLarge)?;
    let total_size_u32 = u32::try_from(total_size).map_err(|_| EncodeError::TooLarge)?;
    let strings_offset_u32 = u32::try_from(strings_offset).map_err(|_| EncodeError::TooLarge)?;

    let mut bytes = vec![0; total_size];
    bytes[..MAGIC.len()].copy_from_slice(&MAGIC);
    bytes[8..10].copy_from_slice(&VERSION.to_le_bytes());
    bytes[10..12].copy_from_slice(&(HEADER_SIZE as u16).to_le_bytes());
    bytes[12..16].copy_from_slice(&total_size_u32.to_le_bytes());
    bytes[16..20].copy_from_slice(&entry_count.to_le_bytes());
    bytes[20..24].copy_from_slice(&strings_offset_u32.to_le_bytes());

    let mut name_offset = 0usize;
    for (index, symbol) in symbols.iter().enumerate() {
        let entry = HEADER_SIZE + index * ENTRY_SIZE;
        let name_offset_u32 = u32::try_from(name_offset).map_err(|_| EncodeError::TooLarge)?;
        let name_len_u32 = u32::try_from(symbol.name.len()).map_err(|_| EncodeError::TooLarge)?;
        bytes[entry..entry + 8].copy_from_slice(&symbol.start.to_le_bytes());
        bytes[entry + 8..entry + 16].copy_from_slice(&symbol.size.to_le_bytes());
        bytes[entry + 16..entry + 20].copy_from_slice(&name_offset_u32.to_le_bytes());
        bytes[entry + 20..entry + 24].copy_from_slice(&name_len_u32.to_le_bytes());
        let name_start = strings_offset + name_offset;
        bytes[name_start..name_start + symbol.name.len()].copy_from_slice(symbol.name.as_bytes());
        name_offset += symbol.name.len();
    }

    // Keep producer and consumer checks coupled at the format owner boundary.
    SymbolTable::parse(&bytes).expect("encoder must produce a parseable symbol table");
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_order_ranges_and_names() {
        let inputs = [
            SymbolInput::new(0x8000, 0x20, "alpha"),
            SymbolInput::new(0x9000, 0x30, "beta::gamma"),
        ];
        let bytes = encode(&inputs).unwrap();
        let symbols: Vec<_> = SymbolTable::parse(&bytes).unwrap().iter().collect();
        assert_eq!(
            symbols,
            vec![
                crate::Symbol {
                    start: 0x8000,
                    size: 0x20,
                    name: "alpha",
                },
                crate::Symbol {
                    start: 0x9000,
                    size: 0x30,
                    name: "beta::gamma",
                },
            ]
        );
    }

    #[test]
    fn invalid_inputs_fail_before_bytes_are_published() {
        assert_eq!(
            encode(&[SymbolInput::new(1, 0, "zero")]),
            Err(EncodeError::ZeroSizedSymbol)
        );
        assert_eq!(
            encode(&[SymbolInput::new(u64::MAX, 2, "overflow")]),
            Err(EncodeError::AddressOverflow)
        );
        assert_eq!(
            encode(&[
                SymbolInput::new(2, 1, "second"),
                SymbolInput::new(1, 1, "first"),
            ]),
            Err(EncodeError::UnsortedSymbols)
        );
        assert_eq!(
            encode(&[SymbolInput::new(1, 1, "")]),
            Err(EncodeError::EmptyName)
        );
    }
}
