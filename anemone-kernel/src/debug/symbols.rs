//! Immutable linker-owned kernel symbol-table input and checked view.

use symtab::SymbolTable;

use crate::arch::link_symbols::{__eanemone_symtab, __sanemone_symtab};

// The blob length is a build input only. Runtime lookup always derives the
// byte range from linker-owned start/end symbols, so table size cannot become
// a second behavioral truth or alter the lookup path's code shape.
#[used]
#[unsafe(link_section = ".anemone.symtab.input")]
static EMBEDDED_SYMBOL_TABLE: [u8; include_bytes!("../../../build/generated/kernel.symtab").len()] =
    *include_bytes!("../../../build/generated/kernel.symtab");

pub(super) fn embedded_table() -> Option<SymbolTable<'static>> {
    let start = __sanemone_symtab as *const () as usize;
    let end = __eanemone_symtab as *const () as usize;
    let len = end.checked_sub(start)?;
    // The linker script owns this immutable loadable range for the lifetime of
    // the kernel. Checked parsing contains any generated-format corruption and
    // lets panic diagnostics fail closed to raw PCs.
    let bytes = unsafe { core::slice::from_raw_parts(start as *const u8, len) };
    SymbolTable::parse(bytes).ok()
}
