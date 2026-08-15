# Source provenance

- Upstream project: <https://github.com/wasmi-labs/wasmi>
- Upstream tag: `v1.1.0`
- Upstream commit: `8273dfb09d493971b7bb12fe614d740cdc857175`
- Imported on: 2026-08-14
- Licenses: MIT or Apache-2.0; the unmodified license texts are retained as
  `LICENSE-MIT` and `LICENSE-APACHE`.

The import retained the interpreter package plus its `core`, `ir`, and
`collections` implementation dependencies. It intentionally omitted the
upstream CLI, WASI adapter, C API, alternate `ir2`, fuzz-support package, WAST
runner, benchmark artifacts, and nested Git submodules. Regression tests kept
under this directory are validation inputs and are not production dependencies.

After import, this directory is maintained as Anemone source. The upstream tag
records provenance only; it does not define this crate's API or future source
revision.
