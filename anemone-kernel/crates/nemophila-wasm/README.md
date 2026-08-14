# nemophila-wasm

`nemophila-wasm` is Anemone's first-party, `no_std + alloc` Core WebAssembly
interpreter. Its embedding API, configuration, supported proposals, and limits
evolve with the interpreter and its real kernel consumers; they are not a
separately versioned Nemophila profile.

Normal module construction validates WebAssembly before translation and
execution. Unchecked construction remains a crate-private unsafe boundary and
cannot be used by a kernel load path.

The source started from Wasmi `v1.1.0`. See [PROVENANCE.md](./PROVENANCE.md)
for the exact import identity and retained source boundary. The upstream README
and changelog are preserved as historical source material only.
