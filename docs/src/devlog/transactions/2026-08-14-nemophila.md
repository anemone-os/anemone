# 2026-08-14 - Nemophila

**Status:** Active / R2 / Stage 1 Closed / Stage 2 Ready / Not Started
**Owners:** doruche, Codex
**Canonical Target:** [RFC-20260814-nemophila R2](../../rfcs/nemophila/index.md)
**Implementation Route:** [Stage 1--6](../../rfcs/nemophila/implementation.md)
**Contract Delta:** None effective；`NEMOPHILA-R0-CUTOVER`仍为Future

## Scope

本transaction只为长期、多Stage RFC保存checkpoint execution、validation与resolution evidence。target、owner、ABI、
Contract Impact、acceptance和Stage路线仍只由canonical RFC及implementation拥有；本页不建立第二份计划、
interpreter profile/version或current contract。Stage 1已关闭；维护者本轮授权Stage 2 docs-only resolution，未授权任何Stage 2
implementation。Stage 3--6仍未获解析或执行授权。

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

Stage 1 closure后的`nemophila-wasm`是仓库内持续演进的第一方source，Stage 2直接消费当前source，无需额外source
handoff或并列source authority。production feature selection为default `extra-checks`，或等价的
`--no-default-features --features extra-checks`；crate-level compile guard禁止关闭executor invariant checks。`host-test`
只为host regression追加`std`与WAT parser。后续Stage若修改interpreter source，直接由普通Git历史记录并重跑受影响proof。

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

**Contract Cutover:** None。Stage 1只交付crate source、测试与build/link evidence；没有发布kernel
runtime、public management ABI、visible semantics、current contract或register baseline，也不是
`NEMOPHILA-R0-CUTOVER`。

**Validation:** `just fmt kernel`与最终`just fmt kernel --check`通过；`just test nemophila-wasm`通过：production
`no_std + alloc + extra-checks` check通过，55项unit tests、53项integration tests和1项doctest通过，定向Miri的3项
Stage 1 embedding tests通过，`riscv64gc-unknown-none-elf`与`loongarch64-unknown-none` validation staticlib均完成build/link。
使用`rustc 1.96.0-nightly (48cc71ee8 2026-03-31)`、LLVM 22.1.2，两个target均由当前toolchain安装。`cargo tree`、source/
API/unsafe/hygiene audit、remote tag audit、`git diff --check`与`mdbook build docs`通过。

kernel KUnit、QEMU、LTP、Nemophila admission/lifecycle/runtime tests、真实kernel load path、完整fuzz campaign、hardware和
RV64/LA64 R0 runtime acceptance均Not Run；crate-level cross-target staticlib build/link不得外推这些结果。

**Next / Stop:** Stage 1 Closed。该次执行严格停止在Stage 1，current contracts、register和`NEMOPHILA-R0-CUTOVER`保持
不变。后续Stage 2 resolution另见下一个checkpoint；它不改变本条closure evidence。

### 2026-08-14 - Stage 2 docs-only resolution

**Resolution:** Stage 2已解析为Ready / Not Started，Execution Authorization为None。实施路线现已闭合WIT source、Rust SDK
callback lowering、module manifest、最小`ModuleBuildDriver` trait、唯一Cargo implementation、common artifact
validation/export、canonical clone observer和host interpreter harness的Implementation Boundary、Deliverables、Validation、
Cutover与Exit / Stop。其它语言SDK、其它语言构建链和额外driver明确留在本Stage non-goals；kernel runtime、SystemTarget、
management ABI和双架构guest execution也未提前进入。

**Owner Decisions:** WIT继续是logical interface唯一owner；Rust SDK只拥有generated consumer view与guest-side ergonomic
lowering。WIT没有first-class function value，因此typed Rust callback由SDK保存为guest-instance-local slot，registration import
保持point-specific且不携带raw callable，固定WIT-derived callback trampoline在成功registration后调用该slot。static language
initialization只能落在唯一module-side `load` wrapper的合法路径中，不能形成Core Wasm start或第二个lifecycle entry。

Rust module与SDK允许使用instance linear memory内的guest-local heap，不承诺allocation-free。最终artifact在真实使用`alloc`时
必须链接guest global allocator，但allocator选择、SDK是否提供默认allocator以及callback environment采用inline还是heap storage
仍属Stage 2 implementation preference。该选择不得增加Host allocation service/import、kernel resource handle、跨instance
allocator或guest cleanup phase；registration failure释放pending environment，成功registration保留environment直到instance
销毁，而instance销毁仍直接回收guest memory、不运行guest `Drop`。allocator trap继续落入已有load-trap rollback或callback-trap
poison边界，不新增WIT registration error。

module build由xtask独立拥有，不复用app manifest/driver/type/architecture。真实但最小的`ModuleBuildDriver` trait只执行toolchain
并返回本次candidate；Cargo是唯一implementation。common module path统一拥有freshness、ordinary-file、Core Wasm/WIT
envelope、interpreter validation、diagnostics与`build/` export。Cargo route固定使用repository-owned`wasm32v1-none`与
`core`/`alloc` settings，只生成一份无architecture selector的artifact。

**Evidence:** live xtask app path确认现有app driver是enum dispatch，只作为manifest/driver/common-export结构参考；live
`rust-toolchain.toml`尚未声明Wasm target，而当前repository rustc target list包含`wasm32v1-none`。kernel `Tid`是`u32`；当前
`nemophila-wasm` parser/validator保留start-section识别能力；本地`wit-bindgen 0.51.0` source显示可关闭默认`std`/async并生成
Core Wasm export glue，其constructor workaround从generated export call path触发，因此Stage 2必须证明显式load先完成初始化且
artifact没有start。上述证据只解析实施路线，未构建或执行candidate artifact。

**Contract Cutover:** None。Stage 2 resolution不发布code、artifact、public ABI、visible semantics、current contract或register
baseline，也不是`NEMOPHILA-R0-CUTOVER`。

**Validation:** `git diff --check`与`mdbook build docs`通过。Stage 2 implementation、WIT/SDK tests、module Cargo build、
artifact inspection、`nemophila-wasm` host harness、kernel KUnit、QEMU、LTP、hardware与RV64/LA64 R0 runtime acceptance均
Not Run。

**Next / Stop:** Stage 2为Ready / Not Started；本轮严格停止在resolution。只有维护者新的明确授权才能开始Stage 2
implementation；Stage 2 closure也不会自动授权Stage 3。
