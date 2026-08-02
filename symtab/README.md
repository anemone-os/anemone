`symtab` owns Anemone's private, versioned kernel symbol-table byte format.
The host-side `builder` feature provides a checked encoder; the default
`no_std` surface provides an allocation-free checked parser and bounded
lookup. ELF selection and build-pass orchestration remain xtask concerns.
