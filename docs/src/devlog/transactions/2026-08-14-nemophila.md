# 2026-08-14 - Nemophila

**Status:** Active / R2 / Stage 1 Closed
**Owners:** doruche, Codex
**Canonical Target:** [RFC-20260814-nemophila R2](../../rfcs/nemophila/index.md)
**Implementation Route:** [Stage 1--6](../../rfcs/nemophila/implementation.md)
**Contract Delta:** None effective；`NEMOPHILA-R0-CUTOVER`仍为Future

## Scope

本transaction只为长期、多Stage RFC保存checkpoint execution、validation与后续handoff。target、owner、ABI、
Contract Impact、acceptance和Stage路线仍只由canonical RFC及implementation拥有；本页不建立第二份计划、
interpreter profile/version或current contract。维护者本轮只授权并关闭Stage 1；Stage 2--6均未获解析或执行授权。

## Checkpoint Log

### 2026-08-14 - Stage 1 activation and closure

**Change:** 从Accepted R2导入Wasmi `v1.1.0` tag对应的固定upstream commit
`8273dfb09d493971b7bb12fe614d740cdc857175`，在`anemone-kernel/crates/nemophila-wasm`形成Anemone直接拥有的
`nemophila-wasm`、`nemophila-wasm-core`、`nemophila-wasm-ir`和`nemophila-wasm-collections`源码。导入保留双许可证、
provenance、通用Core Wasm parser/validator/translator/executor及适用回归，删除CLI、WASI、C API、`ir2`、fuzz-support、
WAST runner、benchmark和nested Git/submodule。crate的configuration、proposal、limits和ordinary embedding API继续随
interpreter及真实consumer自然演进，没有增加profile/version、固定feature matrix或observer专用wrapper。

workspace与`just test nemophila-wasm`接线同时加入host regression和validation-only bare-metal staticlib consumer。host
regression覆盖eager validated construction、memory-bearing module、Host `i32` round trip、malformed/type-invalid rejection
和integer division-by-zero trap；bare-metal consumer直接使用普通`Config`、`Engine`、`Module`、`Linker`、`Store` API，
其bump allocator只迫使RV64/LA64链接真实`alloc`依赖图，artifact不执行，也不是候选kernel allocator。

Stage 2 handoff精确pin Anemone commit
`85489765a4ff57aac2d6eedd3567e98fa60b4b4c`。production feature selection为default `extra-checks`，或等价的
`--no-default-features --features extra-checks`；crate-level compile guard禁止关闭executor invariant checks。`host-test`
只为host regression追加`std`与WAT parser。后续Stage若修改interpreter source，必须pin新commit并重跑受影响proof；本次
pin不冻结crate source或API。

**Source / Dependency Audit:** `cargo tree -p nemophila-wasm --no-default-features --features extra-checks`的production graph
只包含三个in-tree implementation crates、`wasmparser`、`spin`、`libm`和`bitflags`；没有upstream `wasmi`、WASI、WAT、
CLI/C API、host filesystem/thread/random dependency，也没有kernel object或Nemophila lifecycle owner。源码树没有`.git`、
`.gitmodules`、symlink、submodule或第二套interpreter。remote tag audit确认`v1.1.0^{}`解析到记录的upstream commit；该tag
只有fuzz harness/fuzz-support，没有checked-in crash corpus，因此未把host-only fuzz framework导入production tree。

**Safety Audit:** 普通buffered module construction只有`Module::new`，它先安装按当前`Config`生成的`Validator`，再进入
private parser/translation implementation；导入时存在的public unchecked constructor已删除，validation consumer与未来
kernel consumer均无法绕过validation。retained production unsafe按owner-local invariant分为：仅由`T` storage决定的arena
marker `Send`/`Sync`；`ByteBuffer`的`Vec`/exclusive static backing raw representation；reference/store的layout、bit pattern和
type-identity转换；append-only pinned code map；executor内由validated/translated instruction stream、frame/register bounds、
cache refresh和store lifetime共同约束的pointer/cache/stack操作。对应invariant保留在owner-local safety contract，
`extra-checks`在所有受支持build中常开，定向Miri覆盖Stage 1 validation/execution/trap consumer。validation-only allocator的
`GlobalAlloc`/`Sync` unsafe独立于production crate；其单artifact、never-executed假设已在实现处说明，不能外推为kernel allocator
或runtime safety proof。

**Contract Cutover:** None。Stage 1只交付crate source、测试、build/link evidence与Stage 2 pinned handoff；没有发布kernel
runtime、public management ABI、visible semantics、current contract或register baseline，也不是
`NEMOPHILA-R0-CUTOVER`。

**Validation:** `just fmt kernel`与最终`just fmt kernel --check`通过；`just test nemophila-wasm`通过：production
`no_std + alloc + extra-checks` check通过，55项unit tests、53项integration tests和1项doctest通过，定向Miri的3项
Stage 1 embedding tests通过，`riscv64gc-unknown-none-elf`与`loongarch64-unknown-none` validation staticlib均完成build/link。
使用`rustc 1.96.0-nightly (48cc71ee8 2026-03-31)`、LLVM 22.1.2，两个target均由当前toolchain安装。`cargo tree`、source/
API/unsafe/hygiene audit、remote tag audit、`git diff --check`与`mdbook build docs`通过。

kernel KUnit、QEMU、LTP、Nemophila admission/lifecycle/runtime tests、真实kernel load path、完整fuzz campaign、hardware和
RV64/LA64 R0 runtime acceptance均Not Run；crate-level cross-target staticlib build/link不得外推这些结果。

**Next / Stop:** Stage 1 Closed。执行严格停止在本Stage；Stage 2--6保持Outline Only且未获authorization，current contracts、
register和`NEMOPHILA-R0-CUTOVER`保持不变。下一步只能在维护者新的明确授权下解析并执行Stage 2。
