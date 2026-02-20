//! This module provides functions to build a symbol table in memory.
//! std crate is required for dynamic memory allocation.

use std::{alloc::Layout, collections::BTreeSet, os::raw::c_char, ptr::NonNull};

use super::{SymbolEntry, SymbolTable, SymbolType};

#[derive(Debug, Clone)]
pub struct BuilderSymbol {
    address: u64,
    name: String,
    type_: SymbolType,
}

impl BuilderSymbol {
    pub fn new<S>(address: u64, name: S, type_: SymbolType) -> Self
    where
        S: AsRef<str>,
    {
        Self {
            address,
            name: name.as_ref().to_string(),
            type_,
        }
    }
}

impl PartialEq for BuilderSymbol {
    fn eq(&self, other: &Self) -> bool {
        self.address == other.address
    }
}

impl Eq for BuilderSymbol {}

impl PartialOrd for BuilderSymbol {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BuilderSymbol {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.address.cmp(&other.address)
    }
}

#[derive(Debug, Clone)]
pub struct Builder {
    symbols: BTreeSet<BuilderSymbol>,
}

/// This encapsulates the built symbol table and its layout,
/// We do not implement Copy or Clone, thus preventing accidental
/// use-after-free.
#[derive(Debug)]
pub struct BuiltSymbolTable {
    raw: NonNull<u8>,
    bytes: usize, // layout.size() maybe larger due to alignment padding
    layout: Layout,
}

impl Builder {
    pub fn new() -> Self {
        Self {
            symbols: BTreeSet::new(),
        }
    }

    /// Add a symbol to the builder. Returns true if the symbol was added,
    /// false if a symbol with the same address already exists.
    pub fn add_symbol(&mut self, symbol: BuilderSymbol) -> bool {
        self.symbols.insert(symbol)
    }

    /// Consume the builder and build the symbol table.
    pub fn build(self) -> BuiltSymbolTable {
        // Calculate sizes
        let num_symbols = self.symbols.len();
        let entries_size = num_symbols * size_of::<SymbolEntry>();
        let strtab_size: usize = self.symbols.iter().map(|s| s.name.len() + 1).sum();
        let total_size = size_of::<u64>() * 2 + entries_size + strtab_size;
        let padded_size = (total_size + 7) & !7; // align to 8 bytes
        unsafe {
            let layout = Layout::from_size_align(padded_size, 8).unwrap();
            let ptr = std::alloc::alloc(layout);
            if ptr.is_null() {
                std::alloc::handle_alloc_error(layout);
            }

            // header
            let magic_ptr = ptr as *mut u64;
            magic_ptr.write(SymbolTable::MAGIC);
            let num_symbols_ptr = magic_ptr.add(1);
            num_symbols_ptr.write(num_symbols as u64);
            // entries
            let mut entry_ptr = num_symbols_ptr.add(1) as *mut SymbolEntry;
            let mut strtab_ptr = entry_ptr.add(num_symbols) as *mut c_char;
            let mut accumulated_offset: u32 = 0;
            for symbol in self.symbols {
                entry_ptr.write(SymbolEntry {
                    address: symbol.address,
                    name_offset: accumulated_offset,
                    type_: symbol.type_ as u8,
                });
                let name_bytes = symbol.name.as_bytes();
                core::ptr::copy_nonoverlapping(
                    name_bytes.as_ptr(),
                    strtab_ptr as *mut u8,
                    name_bytes.len(),
                );
                strtab_ptr.add(name_bytes.len()).write(0); // null terminator
                strtab_ptr = strtab_ptr.add(name_bytes.len() + 1);
                accumulated_offset += (name_bytes.len() + 1) as u32;
                entry_ptr = entry_ptr.add(1);
            }

            BuiltSymbolTable {
                raw: NonNull::new_unchecked(ptr),
                bytes: total_size,
                layout,
            }
        }
    }
}

impl BuiltSymbolTable {
    pub fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.raw.as_ptr(), self.bytes) }
    }
}

impl Drop for BuiltSymbolTable {
    fn drop(&mut self) {
        unsafe {
            std::alloc::dealloc(self.raw.as_ptr() as *mut u8, self.layout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symtab() {
        let mut builder = Builder::new();
        let symbols = [
            BuilderSymbol {
                address: 0x80000000,
                name: "start".to_string(),
                type_: SymbolType::Text,
            },
            BuilderSymbol {
                address: 0x80001000,
                name: "main".to_string(),
                type_: SymbolType::Text,
            },
            BuilderSymbol {
                address: 0x40002000,
                name: "data_var".to_string(),
                type_: SymbolType::Data,
            },
            BuilderSymbol {
                address: 0x0,
                name: "zero".to_string(),
                type_: SymbolType::Unknown,
            },
        ];
        for symbol in symbols {
            assert!(builder.add_symbol(symbol));
        }
        let symtab = builder.build();
        let size_1 = symtab.as_bytes().len();
        let symtab = unsafe { SymbolTable::from_ptr(symtab.as_bytes().as_ptr()) }
            .expect("symtab format mismatch");
        assert_eq!(symtab.num_symbols(), 4);
        let size_2 = symtab.raw.len() + 16;
        assert_eq!(size_1, size_2);

        for symbol in symtab {
            println!(
                "Symbol: addr=0x{:x}, name={}, type={:?}",
                symbol.address,
                symbol.name.to_str().unwrap(),
                symbol.type_
            );
        }

        // lookup tests
        let sym = symtab.lookup(0x80000000).unwrap();
        assert_eq!(sym.name.to_str().unwrap(), "start");
        let sym = symtab.lookup(0x80000FFF).unwrap();
        assert_eq!(sym.name.to_str().unwrap(), "start");
        let sym = symtab.lookup(0x80001000).unwrap();
        assert_eq!(sym.name.to_str().unwrap(), "main");
        let sym = symtab.lookup(0x40002010).unwrap();
        assert_eq!(sym.name.to_str().unwrap(), "data_var");
        let sym = symtab.lookup(0x1).unwrap();
        assert_eq!(sym.name.to_str().unwrap(), "zero");
    }
}
