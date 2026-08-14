# 2026-08-14 - Nemophila

**Status:** Active / R2 / Stage 1 Closed / Stage 2 Closed
**Owners:** doruche, Codex
**Canonical Target:** [RFC-20260814-nemophila R2](../../rfcs/nemophila/index.md)
**Implementation Route:** [Stage 1--6](../../rfcs/nemophila/implementation.md)
**Contract Delta:** None effective；`NEMOPHILA-R0-CUTOVER`仍为Future

## Scope

本transaction只为长期、多Stage RFC保存checkpoint execution、validation与resolution evidence。target、owner、ABI、
Contract Impact、acceptance和Stage路线仍只由canonical RFC及implementation拥有；本页不建立第二份计划、
interpreter profile/version或current contract。Stage 1与Stage 2均已按独立授权关闭；Stage 3--6仍未获解析或执行授权。

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

### 2026-08-14 - Stage 2 implementation and closure

**Execution Authorization:** 维护者授权完成Stage 2；该授权已消费。Stage 3的解析与执行均未授权。

**Change:** 新增canonical `anemone:nemophila@0.1.0` WIT package/world、独立`no_std + alloc` Rust SDK、
`clone-observer` module workspace/lockfile与`wasm32v1-none` Cargo artifact。SDK公开load-scoped
capability/provider/point hierarchy与typed callback，只在guest instance memory保存`Empty`/`Pending`/`Registered`
callback slot；registration failure先释放pending environment，success保留environment直到instance整体销毁。module load只以
独立unit result表达module是否接受本次load，point-specific registration result继续只表达Host binding请求；clone point为
`Fanout`，因此其result只保留`registered`、`provider-unavailable`与`already-registered`。固定callback export在load前或
registration failure后调用都会trap；`wit-bindgen`的per-export constructor workaround被显式禁用，避免callback entry在
module-side `load`之前执行静态初始化。

xtask新增独立module manifest/config owner、`module build` action、最小`ModuleBuildDriver` trait与唯一Cargo driver；没有复用
app manifest/driver、SystemTarget、Platform architecture或artifact path。Cargo route固定repository toolchain、
`wasm32v1-none`、`nemophila-module` profile、`-Z build-std=core,alloc`与`--locked`。每次invocation建立新的
`build/modules/.candidates/clone-observer-<pid>-<sequence>`，common path拒绝missing/non-regular/multiple/stale candidate，
区分Core/Component binary，使用canonical WIT检查metadata/import/export/helper/custom-section envelope，以当前
`nemophila-wasm`完成eager validation/translation并运行value-only fake Host harness，最后只原子发布同一份已验证byte
snapshot到`build/modules/clone-observer/nemophila_clone_observer.wasm`。standard clean、repository format all/module scope和
module-specific scope均覆盖新增source/output。

按维护者关于Wasmi crate shape的要求，原`nemophila-wasm-collections`与`nemophila-wasm-ir`两个独立implementation crates
删除并机械内收到`nemophila-wasm::{collections,ir}`私有模块；`nemophila-wasm-core`仍保持独立crate。interpreter新增通用
structural `Module::has_start()` metadata query，R0 start rejection仍由Nemophila artifact owner决定。该收缩没有改变通用
validator/executor truth、公开embedding语义或interpreter/Nemophila owner分工。

**Artifact / Dependency Audit:** 最终stable artifact为19842-byte ordinary Core Wasm file，SHA-256为
`383a504d961c8d609822274e057143da8a2d96e1fd947399700e4e5ffff85bf6`。两次fresh candidate build得到相同hash；actual imports
只有WIT-derived `anemone:nemophila/weave-clone@0.1.0::register-observer`与
`anemone:nemophila/logging@0.1.0::write`，exports只有`load`、`observe-clone`、linear memory、allocator helper和
toolchain globals，custom sections只含两份WIT component-type metadata、`name`、`producers`与`target_features`。artifact没有
Core Wasm start、Component binary、WASI/host-libc import、额外Host service/lifecycle entry或`run_ctors_once` symbol。module
lockfile包含`dlmalloc`对其它host target的conditional `libc`/`windows-sys`记录，但实际`wasm32v1-none` dependency graph与
artifact没有这些runtime依赖。source tree没有nested Git、submodule或symlink；interpreter production graph只保留main/core
implementation、`wasmparser`、`spin`、`libm`、`bitflags`及feature-selected collection dependencies。

**Host Harness:** callback-before-load trap且不注册/不记录日志；successful load恰好调用一次registration，随后callback把
`0`与`u32::MAX` TID原样记录为`clone creator=0 child=4294967295`；provider-unavailable时module收到typed failure，SDK先
释放pending callback environment，再产生精确debug diagnostic并返回generic module load error，callback继续不可调用。
fake Host只实现WIT value boundary，不形成production runtime/provider/lifecycle facade。

**Independent Review / Architecture Friction:** 独立review最初发现并要求修正三条真实摩擦：module load result复用
registration state且clone `Fanout` result预置exclusive-only outcome；stable export二次读取candidate而可能偏离已验证snapshot；
bindgen默认在每个export前运行constructor workaround而绕过唯一load lifecycle。实现分别以独立unit load result和收窄的
point-specific result、直接发布已验证bytes、显式禁用constructor workaround关闭。review复核最新diff后确认无剩余
Apollyon、Keter、Euclid或值得记录的Safe finding。最终Architecture Friction Scan确认WIT/runtime binding、interpreter/
artifact validation、driver/common path与candidate/export各自只有一个行为owner，没有第二份状态真相、owner穿透、public API
扩张、test-only production path或降低oracle的桥。

**Contract Cutover:** None。Stage 2只交付WIT/SDK/toolchain/canonical artifact和host evidence；没有kernel runtime、
SystemTarget embedded selection、public management ABI、visible semantics、current contract或register baseline，也不是
`NEMOPHILA-R0-CUTOVER`。

**Validation:** `just --list`、`just xtask module --help`与`just xtask module build --help`确认production CLI；
`just test xtask`通过105项测试；`just test nemophila-module`通过真实Cargo build、envelope与host harness，随后第二次
`just module build clone-observer`从另一fresh candidate重复得到相同hash。`just clean`确认stable module export被删除，clean后
重新build成功。`just test nemophila-wasm`通过production `no_std + alloc + extra-checks` check、71项unit tests、53项
integration tests、1项doctest、定向Miri的3项Stage 1 embedding tests，以及RV64/LA64 validation staticlib build/link。
`just build --preset qemu-virt-rv64-release`完成repository-owned RV64 release kernel双pass build；该命令只恢复/验证generated
kernel inputs与workspace build，不是Nemophila kernel integration或runtime evidence。最终`just fmt all --check`、
`just fmt modules --check`、`just fmt clone-observer --check`、`git diff --check`与`mdbook build docs`通过。toolchain为
`rustc 1.96.0-nightly (48cc71ee8 2026-03-31)`、LLVM 22.1.2。

kernel KUnit执行、QEMU、LTP、hardware、guest RV64/LA64 Nemophila runtime、kernel admission/lifecycle/management、
SystemTarget embedded selection与R0双架构acceptance均Not Run；host harness、bare-metal interpreter link和普通RV64 kernel
build不得外推这些结果。

**Next / Stop:** Stage 2 Closed。current contracts与register保持不变，`NEMOPHILA-R0-CUTOVER`仍为Future。本次严格停止在
Stage 2；Stage 3仍为Outline Only，未获解析或执行授权。
