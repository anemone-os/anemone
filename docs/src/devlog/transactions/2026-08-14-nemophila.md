# 2026-08-14 - Nemophila

**Status:** Active / R4 / Stage 1 Closed / Stage 2 Closed / Stage 2 Feedback Interlude Closed /
Stage 3 Closed / Stage 4 Closed / Stage 4 Feedback Interlude Closed / Stage 5 Closed
**Owners:** doruche, Codex
**Canonical Target:** [RFC-20260814-nemophila R4](../../rfcs/nemophila/index.md)
**Implementation Route:** [Stage 1--6](../../rfcs/nemophila/implementation.md)
**Contract Delta:** None effective；`NEMOPHILA-R0-CUTOVER`仍为Future

## Scope

本transaction只为长期、多Stage RFC保存checkpoint execution、validation与resolution evidence。target、owner、ABI、
Contract Impact、acceptance和Stage路线仍只由canonical RFC及implementation拥有；本页不建立第二份计划、
interpreter profile/version或current contract。Stage 1、Stage 2与进入Stage 3前的feedback interlude均已按独立授权关闭；
interlude纠正Stage 2暴露的build/admission owner摩擦并承载R3 target revision。维护者已接受R4 integer-only target与
kernel/app compiler-target owner拆分；Stage 3与Stage 4各两个Checkpoint的授权均已消费并关闭；Stage 4后的单次Feedback
Interlude也已关闭，只重整内部owner/API/module/WIT维护边界。Stage 5单一Checkpoint 5A也已授权、消费并关闭；Stage 6仍未获
解析或执行授权。

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
point-specific result、直接发布已验证bytes、显式禁用constructor workaround关闭。在当时review scope内，复核认为无剩余
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

### 2026-08-14 - Stage 2 feedback interlude and R3 closure

**Target Revision:** 维护者接受R3：ordinary module build不再解析WIT、执行artifact envelope或运行canonical module；future
kernel runtime通过interpreter validation、pre-instantiation start rejection、narrow Linker与required typed entry lookup
enforce真实安全/兼容性义务，WIT metadata、精确imports/exports集合与custom-section allowlist不进入admission。Stage 2保持
Closed，Stage 3仍未获解析或执行授权。

**Change:** Rust SDK在同一owner内拆为bindings、lifecycle、call window、`services::logging`、
`weave::task::clone_observer`与export glue；`Module::Error`使通用load lifecycle不再依赖clone-specific registration error。
xtask module driver拆为`driver/mod.rs` contract与`driver/cargo.rs` implementation；manifest删除`package`，Cargo driver删除
`--package`，新增`conf/module.toml`参考模板。ordinary build只保留manifest/path、fixed toolchain/target/profile、fresh
ordinary candidate、single output与atomic export，删除`interface`、`envelope`、`harness`及xtask对`nemophila-wasm`、
`wasmparser`、`wit-component`、`wit-parser`、`wat`的依赖。

canonical clone observer的fake Host execution迁入module-local `host-fixture`。该fixture只在真实kernel Host wiring不存在时
证明SDK/WIT/interpreter seam，源码与implementation均要求Stage 5真实runtime/provider路径覆盖同一load、registration
success/failure、callback、logging与failure behavior后删除；它不是generic validator、production dependency或长期
admission facade。module toolchain继续使用`rust-toolchain.toml`固定的builtin`wasm32v1-none`，不在`conf/arch`复制target
spec JSON。

**Independent Review / Architecture Friction:** 独立subagent review发现两项Euclid并要求即时修正：SDK根级仍re-export
service/weave/point types，使canonical module继续依赖flat surface；driver common `BuildContext`与constant仍泄露
`cargo_manifest`/Cargo profile。实现删除领域类型root re-export并让canonical module使用`services::logging`与
`weave::task::clone_observer`，同时把context字段收敛为`manifest`并将profile移入`driver/cargo.rs`。review复核最新diff后
确认无残留Apollyon、Keter、Euclid或值得记录的Safe finding。Architecture Friction Scan确认build/runtime admission、SDK
infrastructure/service/point、driver contract/implementation与临时fixture/production replacement各有唯一owner，未留下第二份
状态/compatibility truth、owner穿透、public surface扁平化、无退出条件bridge或test-only production dependency。

**Validation:** `just test xtask`通过101项测试；`just test nemophila-module`从fresh candidate完成真实Cargo build并通过module-
local host fixture，最终artifact为19992-byte ordinary file，SHA-256为
`89288ff48e4162b20c3fd4b22cd9a45ccad625e775c57cab4673f2d62812f088`。`just fmt all --check`、`git diff --check`、
`mdbook build docs`、`just --list`、`just xtask module --help`与`just xtask module build --help`通过；CLI只描述build/export。
source/lockfile residual audit确认xtask没有WIT/API/envelope/harness入口，也不含`nemophila-wasm`、`wasmparser`、
`wit-component`、`wit-parser`或`wat`依赖。toolchain仍为`rustc 1.96.0-nightly (48cc71ee8 2026-03-31)`、LLVM 22.1.2。

kernel KUnit、QEMU、LTP、hardware、guest RV64/LA64 Nemophila runtime、kernel admission/lifecycle/management、SystemTarget
embedded selection与R0双架构acceptance均Not Run；module-local host fixture与ordinary build不得外推这些结果。

**Contract Cutover / Stop:** None。没有kernel runtime、SystemTarget、management ABI、visible semantics、current contract或
register baseline；host fixture不能外推kernel admission/lifecycle evidence。本interlude已Closed并停在Stage 3前，Stage 3
仍为Outline Only且未获解析或执行授权。

### 2026-08-14 - Stage 3 docs-only resolution

**Resolution:** 维护者授权解析Stage 3；该docs-only授权已消费。Stage 3现为Resolved / Ready / Not Started，并在同一完整
Implementation Boundary内使用两个execution checkpoint：Checkpoint 1闭合真实kernel embedding、logging Host lowering与
unpublished load transaction；Checkpoint 2消费该transaction，建立唯一runtime collection、kernel-private non-aliasing identity与
atomic publication。两个checkpoint都未获执行授权，Checkpoint 1 closure不自动授权Checkpoint 2，Stage 3 closure也不授权
Stage 4解析或执行。

**Owner / Handoff Decisions:** `anemone-kernel/src/nemophila/`是新的kernel顶层subsystem，拥有Nemophila admission、load
transaction、owning instance、runtime collection、identity publication与Host call window；`nemophila-wasm`继续唯一拥有通用
Core Wasm parse/validation/translation/execution/trap truth。内部load handoff只接收一次调用期间稳定的kernel-owned immutable
byte snapshot，不接收`Task`、credentials、file、user pointer或artifact-source enum；future management owner完成operation-local
`CAP_SYS_MODULE`检查、artifact source owner形成snapshot后，embedded/supplied都调用同一load-and-publish path；Checkpoint 1
的owner-private transaction只返回unpublished instance或typed internal failure，Checkpoint 2的最终入口才返回kernel-private
identity或typed internal failure。Stage 3不新增KernelConfig、SystemTarget、boot initcall、syscall、errno mapping或public
identity representation。

每次load新建并由该instance独占完整Engine/Module/Linker/Store/Instance entity。runtime按checked `Module::new`、pre-
instantiation `has_start()` rejection、Stage 3 narrow Linker、typed module-side `load` lookup的顺序admit；extra exports/custom
sections被忽略，WIT metadata、精确imports/exports集合与custom-section allowlist不进入policy。Checkpoint 1 success只产生
unpublished instance，error/trap直接释放完整transaction-local entity；不预建generic rollback journal/resource ledger，也不产生
Poisoned state。Checkpoint 2 commit才分配不与已销毁instance别名的kernel-private identity，并在唯一owner publication点使完整
instance可见。

**Host / Stage Boundary Decisions:** Stage 3只真实接入canonical WIT定义的value-only logging import。Host检查level、guest
pointer/length、range与UTF-8后向现有`debug::printk`提交borrowed value；无效lowering是contained module failure，不能panic
kernel。printk继续拥有filtering、record bound/truncation、ring与presentation；已提交日志不rollback，也不证明publication。Stage 3
不为canonical clone observer建立fake registration/provider；缺少Stage 4 weave capability时，该artifact只能由真实narrow linker
拒绝。registration/reservation、callback serial/in-flight、poison、try-unload/retirement仍完整留给Stage 4，clone point与canonical
runtime success留给Stage 5，management/artifact ingress与cutover留给Stage 6。

**Proof Route:** admission/load/publication cases走真实production transaction/runtime owner。小型Core Wasm fixtures只存在于
owner-local conditional KUnit，并按语义文件内联；跨模块composition位于`nemophila`最低共同owner的inline `kunits`，不建立
独立`kunit.rs`/`tests.rs`、第二product module或production validation facade。Checkpoint 2 KUnit使用isolated runtime owner并在
case结束时销毁整个test-local fixture，不能把该cleanup外推为production retirement。Checkpoint 1要求双架构KUnit-on/KUnit-off
build与至少RV64真实KUnit boot；Checkpoint 2要求RV64/LA64真实KUnit boot、两架构KUnit-off ordinary build、interpreter/module
regression与source/conditional-surface audit。KUnit proof只覆盖实际architecture/topology/path；LTP、hardware、management、
weave、concurrency、unload、clone与完整R0 acceptance继续Not Run。

**Live Evidence:** kernel root当前只有既有顶层subsystems且`anemone-kernel`尚未依赖`nemophila-wasm`；当前interpreter public
embedding surface提供checked `Module::new`、`Module::has_start`、`Linker`、`Store`、typed function lookup/call与Host `Caller`。
canonical WIT当前定义point-specific clone registration、四级logging、唯一`load`与固定`observe-clone`；Stage 2 host fixture仍手工
实现这条边界且明确带Stage 5删除gate。live printk已经拥有structured level policy、有界UTF-8 record、truncation、ring与console
presentation；KUnit contract要求shared-kernel case撤销publication、禁止test-driven production hook并限制proof外推。tracked
KernelConfig当前没有Nemophila feature，repository build/QEMU要求显式preset，现有RV64/LA64 wrappers提供真实KUnit boot route。

**Contract Cutover:** None。resolution没有修改父RFC R3 target、invariant semantics、Contract Impact或acceptance，不发布kernel code、
management ABI、visible semantics、current contract或register baseline，也不是`NEMOPHILA-R0-CUTOVER`。

**Validation / Next / Stop:** 本次只需`git diff --check`与`mdbook build docs`验证canonical docs。Stage 3 implementation、kernel
dependency、KUnit、QEMU、interpreter/module regression、RV64/LA64 runtime、LTP与hardware均Not Run。Stage 3保持Ready / Not
Started；只有维护者新的明确授权才能开始Checkpoint 1。

### 2026-08-14 - Stage 3 Checkpoint 1 activation and R4 target revision

**Activation / Target Revision:** 维护者授权Stage 3 Checkpoint 1执行，并在首次RV64 kernel runtime证据暴露target冲突后接受R4。
R4将`nemophila-wasm`受支持能力收敛为integer-only Core Wasm；`f32`/`f64`类型与相关指令由interpreter validator在checked
construction期间作为unsupported input拒绝。kernel与app compiler target不再共用一份JSON：kernel使用repository-owned
soft-float/no-native-FP target spec，Cargo app使用Rust builtin hard-float bare-metal target；Command app的
`ANEMONE_TARGET_TRIPLE`继续表示Anemone artifact identity。LA64 app继续施加`-C target-feature=-ual`以保持2K1000部署边界，
native userspace ABI与architecture FPU context能力不变。

**Trigger Evidence:** 首次RV64 wrapper已完成KUnit-on kernel build，但在
`anemone_kernel::nemophila::kunits::logging_lowering_contains_invalid_guest_values`执行纯整数guest路径时触发illegal instruction。
`build/nemophila-stage3-ckpt1-rv64.log`记录fault PC `0xffffffff804ebb9c`；对应disassembly位于
`nemophila_wasm::engine::executor::instrs::execute_instrs`，由LLVM生成`fsd fs0, 0x120(sp)`与`fsd fs1, 0x118(sp)`。因此只关闭
guest float不足以恢复kernel invariant；kernel compiler target也必须禁止普通Rust codegen使用native FP。

**Checkpoint Boundary:** Checkpoint 1继续只交付真实kernel embedding、logging Host lowering、unpublished load transaction、
integer-only validator与compiler-target owner拆分；不建立runtime collection、identity、publication、provider caller、management
ABI、KernelConfig/SystemTarget capability或current contract。Checkpoint 2仍为Not Started且未授权；本条activation不会自动授权
Checkpoint 2、Stage 4或任何后续cutover。

**Current State:** Closed。实现、validation、独立review与Architecture Friction Scan已按下条closure evidence闭合；R4与
Checkpoint 1均不构成runtime publication或current-contract cutover。

### 2026-08-14 - Stage 3 Checkpoint 1 implementation and review closure

**Implementation:** kernel新增owner-private顶层`nemophila`subsystem与对当前第一方`nemophila-wasm`的直接dependency；真实
transaction按checked `Module::new`、pre-instantiation start rejection、logging-only narrow Linker、typed `load` lookup/call的
顺序构造完整unpublished instance。success只返回不可观察的owning instance，module error、trap或Host lowering failure均由
transaction-local ownership直接释放；没有runtime collection、identity、publication、rollback ledger、provider、management
入口或production KUnit hook。六项inline KUnit覆盖valid/error/trap、logging lowering、admission rejection与额外export/custom
section边界。

interpreter configuration固定关闭float proposal并删除可重新启用float的配置API；checked construction的regression证明
`f32`/`f64`类型与相关指令在execution前被拒绝。kernel/app compiler target完成owner分离：RV64/LA64 kernel使用
repository-owned soft-float JSON，Cargo app分别使用`riscv64gc-unknown-none-elf`与`loongarch64-unknown-none`，后者继续
`-C target-feature=-ual`；Command app的`ANEMONE_TARGET_TRIPLE`只表示Anemone artifact identity。native userspace FPU ABI与
context能力未改变。

**Validation:** `just fmt all --check`、`git diff --check`、`mdbook build docs`通过。`just test xtask`通过103项；
`just test nemophila-wasm`通过71项unit、54项integration（含float rejection）、4项focused Miri及RV64/LA64 embedding
build；`just test nemophila-module`通过canonical clone-observer fixture。`just app build float-test --arch riscv64`与
`--arch loongarch64`通过。KUnit-on `qemu-virt-rv64-release` / `qemu-virt-la64-release`及KUnit-off
`competition-final-rv64-release` / `competition-final-la64-release`均build通过。

`./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/nemophila-stage3-ckpt1-rv64.log`正常关机：638/638
KUnit通过，其中6项为Nemophila owner-local case；wrapper当前socket LTP profile为6/6。该LTP结果只作为同次用户态回归，
不是Nemophila lifecycle或R0 acceptance proof。LA64 runtime与hardware均Not Run。

双架构ELF以启用对应decoder feature的`rust-objdump`审计。RV64浮点指令只位于显式`__load_next_frs` /
`__save_current_frs` architecture context symbols。LA64最终KUnit ELF中的FPU/LSX指令只位于
`__load_next_lsx`、`__save_current_lsx`、`__load_fpu_control`、`__load_next_frs`、`__save_current_frs`与
`__save_fpu_control`；普通Rust、Nemophila与lwext4 text均未命中。独立review最初发现LA64 lwext4仅使用`-mabi=lp64s`
仍会让GCC选择`-mfpu=FPU 64`，构成Keter；toolchain flags补充`-msoft-float`后重新完成LA64 KUnit-on/off build与ELF审计，
该finding已neutralize。review同时指出的Command app identity措辞已修正。

**Architecture Friction Scan / Proof Limits:** source/visibility/conditional-surface audit确认Nemophila API最高仅
`pub(super)`，没有initcall、syscall、config/runtime collection/identity、unchecked construction或test-driven production
surface；只有语义文件内inline conditional KUnit。没有第二份validator/WIT/log/lifecycle truth、owner穿透、私有表示泄漏、
无退出条件临时桥或隐含cleanup顺序；本checkpoint未留下Euclid/Keter/Apollyon。证据不外推LA64 runtime、management
authorization、artifact ingress、weave、publication/identity、concurrency、poison、unload、clone或完整R0 acceptance。

**Result / Next / Stop:** Stage 3 Checkpoint 1 **Closed**，Contract Cutover为None。Checkpoint 2现在是Ready / Not Started且未获
执行授权；本次执行严格停止，不进入runtime collection、identity、atomic publication、Stage 4或任何后续cutover。

### 2026-08-14 - Stage 3 Checkpoint 2 implementation and closure

**Execution Authorization:** 维护者授权完成Checkpoint 2；该授权已消费。Stage 4及后续gate仍未获解析或执行授权。

**Implementation:** 新增owner-private `runtime`模块与唯一production `Runtime` collection。kernel-private
`InstanceIdentity`从1开始单调分配，不因失败回退；同一个`BTreeMap`同时表达published membership并拥有完整
`RuntimeInstance`，插入map是唯一publication线性化点。解释器checked construction、narrow linking与module-side `load`继续
在publication lock外完成；成功后才在lock内检查identity、插入完整entity并推进cursor。load或commit前失败直接drop
transaction-local entity，不产生identity、半published entry或第二份lifecycle truth。重复load同一artifact仍各自新建
Engine/Module/Store/Instance并取得不同identity。

两个新增inline conditional KUnit composition case直接构造隔离runtime owner：一项以含extra export/custom section的同一bytes
完成两次真实load/commit并观察两个不同identity；另一项证明module拒绝时collection与identity cursor均不变。测试只通过
`#[cfg(feature = "kunit")]`的owner-local只读snapshot观察私有表示，没有向ordinary production surface增加pause、inspection、
reset或failure-injection API；fixture整体销毁只证明test-local cleanup，不外推try-unload/retirement。

**Validation:** `just fmt kernel --check`、`just test xtask`（103/103）、`just test nemophila-wasm`（71项unit、54项integration、
1项doctest、4项focused Miri以及RV64/LA64 embedding build）、`just test nemophila-module`、`git diff --check`与
`mdbook build docs`通过。KUnit-on
`qemu-virt-rv64-release`、`qemu-virt-la64-release`及KUnit-off `competition-final-rv64-release`、
`competition-final-la64-release`均build通过。wrapper使用`smp=1`、`memory=1G`与开发者预先存在的
`kernel_symbols=false` local default tuple；该config diff不是Nemophila变更，也不纳入本checkpoint write-back。

RV64日志`build/nemophila-stage3-ckpt2-rv64.log`记录640/640 KUnit通过，其中8项Nemophila case均通过；当前socket LTP
profile 6/6通过，system-power进入machine action后由SBI handler正常终止QEMU。LA64日志
`build/nemophila-stage3-ckpt2-la64.log`记录643/643 KUnit通过，其中相同8项Nemophila case均通过；同一LTP profile 6/6通过，
filesystem/network/device orderly shutdown也完成。LA64随后按`SYSTEM-POWER-MACHINE-001`报告
`no power off handler succeeded, halting the system`并永久halt，再由host `Ctrl-A x`退出QEMU。wrapper marker本身不是proof；
日志同时核对了全部guest evidence、orderly shutdown顺序与唯一末尾halt路径。

**Independent Review / Architecture Friction:** source/visibility/conditional-surface audit确认production surface只有一个
crate-internal load-and-publish capability；runtime map是唯一publication/lifetime owner，identity只作为其key，不存在mutable
`loaded`镜像、artifact-source状态、shared interpreter entity、test hook、Stage 4 placeholder或public ABI。publication lock只
覆盖identity检查、map insertion与cursor推进；可能调用printk的module load在lock外。独立review最初发现KUnit observation使
`RuntimeInner`与字段在ordinary build中无条件放宽为`pub(super)`，形成一项Euclid；实现恢复私有表示，只留下conditional
owner-local只读snapshot后neutralize。最终diff没有owner穿透、私有表示泄漏、无退出条件临时桥、隐含cleanup顺序或以降低oracle
换取通过的Nemophila内摩擦。review识别的`conf/kconfs/default.toml`差异经核对属于pre-existing user-owned dirty state，不纳入
Nemophila change；对应wrapper evidence按实际tuple诚实记录。

**Maintainer Validation Disposition / Result / Stop:** active register entry
`ANE-20260726-SYSTEM-POWER-ARCH-COVERAGE`与current `SYSTEM-POWER-MACHINE-001`均明确LA64没有ordinary
power-off/reboot handler，两个intent自然到达唯一末尾halt。维护者确认在全部Nemophila/KUnit evidence与orderly subsystem
shutdown完成后由host终止QEMU，是该平台合理且预期的Stage 3 harness终点；这不证明LA64 power-off capability，也不以wrapper
success code替代guest evidence。该disposition保持R4 target、owner、ABI、acceptance、validation claim与current contract，
不要求跨入LA64 platform/system-power owner。Checkpoint 2与Stage 3据此**Closed**，Contract Cutover保持None；Stage 4保持
未解析、未授权。hardware、management authorization、embedded/supplied ingress、weave、callback concurrency/poison、
try-unload、clone与完整R0 acceptance均Not Run / Not Proven。

### 2026-08-14 - Stage 4 docs-only resolution

**Resolution:** 维护者授权解析Stage 4；该docs-only授权已消费。Stage 4现为Resolved / Ready / Not Started，并在同一完整
Implementation Boundary内使用两个execution checkpoint：Checkpoint 1闭合typed provider catalog、canonical WIT registration、
transaction-local reservation与identity/instance/binding atomic publication；Checkpoint 2在同一owner上闭合cohort、explicit
in-flight、per-instance serial execution、trap poison/cancellation与try-unload/retirement。两个checkpoint均未获执行授权；
Checkpoint 1 closure不自动授权Checkpoint 2，Stage 4 closure也不授权Stage 5解析或执行。完整target/non-goals、owner/handoff、
failure/cleanup、验证与停止条件只由[implementation](../../rfcs/nemophila/implementation.md)
拥有，本记录不建立第二份实施计划。

**Resolution Evidence:** Stage 3 final source由一个`SpinLock<RuntimeInner>`保护单调identity cursor和
`BTreeMap<InstanceIdentity, RuntimeInstance>`；该map membership与完整interpreter island是唯一publication/lifetime truth，
module construction/load在publication lock外完成。`RuntimeInstance`当前直接拥有Engine/Module/Store/Instance，`HostContext`
只支持value-only logging，narrow Linker尚未注册`weave-clone`，因此canonical observer仍会因unknown import被拒绝。canonical WIT/
SDK已经固定point-specific registration result、load-only callback storage、`observe-clone(u32,u32)` trampoline与logging callback
window；Stage 2 module-local host fixture仍带Stage 5真实路径替换gate。

ordinary `SpinLock`在当前`spin_lock_irqsave`配置下关闭本地中断，不能跨Wasm execution、Host logging或sleepable serialization；
kernel现有`Mutex`提供ordinary task-context sleepable exclusion并拒绝IRQ/IRQ-off/preempt-disabled调用。KUnit在Late initcall之后运行，
允许在Nemophila并发协议本身被测时使用`KThreadBuilder`、Event/predicate phase和完整join，但禁止固定yield/sleep、timeout成功oracle
与production pause hook。现有perf registry证明双架构linker section/static descriptor可由owner扫描和校验；register没有Nemophila
current open issue/limitation，current contracts也没有Nemophila effective ID。

**Validation / Disposition:** `git diff --check`通过；`mdbook build docs`通过。resolution保持Accepted R4 target、owner、ABI envelope、
Contract Impact与acceptance，Contract Cutover为None；
不新增current contract/register、production point/task call site、management/artifact ingress、public ABI或runtime KernelConfig。
Stage 4 code、KUnit、kernel build、QEMU、SMP、interpreter/module regression与hardware均Not Run。Stage 4保持Ready / Not Started；
只有维护者新的明确授权才能开始Checkpoint 1。

### 2026-08-14 - Stage 4 Checkpoint 1 implementation and closure

**Execution Authorization:** 维护者授权完成Stage 4 Checkpoint 1；该授权已消费。Checkpoint 2、Stage 5及后续gate仍未获执行授权。

**Implementation:** 新增Nemophila-owned `weave`模块与双架构`.nemophila_providers` linker catalog。最窄crate-internal
declaration surface产生typed clone-observer point capability与16-byte immutable descriptor；固定kernel-internal point identity而非
link order、section offset或descriptor address决定catalog身份。descriptor只含point、`Fanout`/`Exclusive` policy与callback shape，
不保存instance、binding、reservation、in-flight或lifecycle state。ordinary build不贡献production descriptor；conditional KUnit
分别贡献clone-shaped `Fanout`与synthetic `Exclusive` descriptor。runtime启动时校验raw descriptor layout、合法组合与duplicate point；
invalid/duplicate是kernel invariant failure，不降格为module registration result。

kernel narrow Linker在logging之外接入canonical `anemone:nemophila/weave-clone@0.1.0.register-observer` import，并从当前Caller
执行fixed `observe-clone(i32, i32)` typed lookup。`HostContext`只持可撤销load-scoped `RegistrationWindow`；module-side `load`
正常返回或trap后先关闭该窗口，再解释load result。late registration因此形成contained Host trap，Store不保存第二份phase truth。

Stage 3 `Runtime`改为由`Arc<RuntimeState>`承载自然内部lifetime；同一个state lock继续保护唯一published collection，并新增
runtime-minted load transaction identity与transaction record。record是全部unpublished reservation唯一真相源；Host capability只
按identity访问，不镜像reservation或lifecycle。registration在lock内检查provider availability、same-transaction duplicate及
point-owned policy；`Exclusive`与其它transaction reservation或live binding冲突，`Fanout`允许并存。dynamic failure无副作用返回
canonical typed discriminant，module自行选择fatal或accepted。successful registration直接把typed callback存入transaction record，
不延迟到commit重新检查policy。

commit在一个runtime state guard下检查monotonic instance identity、移走transaction record、把bindings附着到持有同一Store的
`RuntimeInstance`并插入published map；identity、owning instance与bindings共用一个publication线性化点。rollback先从map撤销
transaction membership，再在临时spin guard释放后析构callback/interpreter entity。唯一production load caller把transaction、
RegistrationWindow、当前Caller callback与最终RuntimeInstance/Store局部成对使用；没有raw pointer、shared interpreter entity、
artifact-source状态、mutable `loaded`镜像或commit-time late conflict。

**Validation:** inline owner-local KUnit新增catalog empty/duplicate/invalid与order-independence、provider unavailable、module-decided
fatal/accepted failure、callback missing/wrong type、late registration trap、same-instance duplicate、`Fanout` reservation coexistence、
`Exclusive` pending/live conflict，以及identity/instance/bindings atomic publication。review指出初始fatal rollback fixture使用empty
catalog，未实际取得reservation；补充两个真实WIT registration成功后分别返回module error与执行guest `unreachable` trap的case，
完整`RuntimeSnapshot`前后相等，直接证明transaction/reservation/publication与identity cursor全部rollback。

`just fmt kernel --check`、`git diff --check`通过。本checkpoint实现完成后执行`just test xtask`（103/103）、
`just test nemophila-wasm`（71项unit、54项integration、1项doctest、4项focused Miri与RV64/LA64 embedding build）及
`just test nemophila-module`；canonical clone-observer build/export/host fixture继续通过。KUnit-on
`qemu-virt-rv64-release`与`qemu-virt-la64-release`、KUnit-off `competition-final-rv64-release`与
`competition-final-la64-release`均build通过。最终补充case后重新完成RV64与LA64 KUnit-on build。

ELF audit显示KUnit-on RV64 catalog为`0xffffffff8073c8c0..0xffffffff8073c8e0`、KUnit-on LA64 catalog为
`0xffffffff807e26e0..0xffffffff807e2700`，两者均恰含两个16-byte conditional descriptor。KUnit-off LA64 ELF中
`__snemophila_providers == __enemophila_providers == 0xffffffff806afd38`，确认ordinary build无production/conditional descriptor。

`./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/nemophila-stage4-ckpt1-rv64.log`使用
`qemu-virt-rv64-release`、`smp=1`、`memory=1G`与preliminary RV64 test image。最终guest完成647/647 KUnit，其中15项
Nemophila case全部进入并通过，新增successful-reservation rollback case有直接marker；当前socket LTP profile 6/6通过，随后完成
orderly shutdown并进入PowerOff machine action。LTP只作为同次用户态回归，不是Nemophila registration proof。LA64 runtime与
hardware Not Run；LA64 build/ELF证据不外推guest execution。

**Independent Review / Architecture Friction:** 独立review核对provider/catalog、WIT Host window、callback/Store lifetime、
commit/rollback Drop顺序、ordinary/conditional surface与Ckpt 2边界。初始唯一Euclid是上述成功reservation后rollback的直接验证
缺口；补case后同一reviewer复核fixture bytecode与snapshot oracle，最终分级Apollyon 0 / Keter 0 / Euclid 0。最终source scan确认
`RuntimeState`是reservation/publication唯一truth；provider不持callback collection，Host window不驱动phase，callback handle随
transaction record移动到owning instance，complex Drop不发生在runtime spin guard内。`LoadTransaction::commit`的owner-private类型
表面可接收一个`RuntimeInstance`，但唯一production caller只提交本transaction刚构造的instance，KUnit直接驱动也只服务已授权的
owner protocol proof；当前没有第二个consumer、public API或错配路径，因此未形成摩擦。没有owner穿透、私有表示泄漏、调用者/
架构特判、无退出条件临时桥、隐含failure/cleanup顺序或以降低oracle换取通过。

**Result / Next / Stop:** Stage 4 Checkpoint 1 **Closed**，Contract Cutover保持None；不更新current contract或register。
Checkpoint 2现在Ready / Not Started且未获执行授权。本次执行严格停止，不进入point invocation、cohort/in-flight、per-instance
execution serialization、trap poison/cancellation、try-unload/retirement、task clone seam、management/public ABI或Stage 5；这些
能力及LA64 runtime、hardware、真实clone placement与完整R0 acceptance均Not Run / Not Proven。

### 2026-08-15 - Stage 4 Checkpoint 2 implementation and Stage closure

**Execution Authorization:** 维护者授权完成Stage 4 Checkpoint 2并在review/validation后指示收口提交；该授权已消费。Stage 5及
后续gate仍未获解析或执行授权。

**Implementation:** Stage 3/Checkpoint 1的published map扩展为membership、live/poisoned lifecycle、bindings与显式in-flight的
唯一行为真相；每个published entry持独立sleepable execution mutex。runtime在一个state guard内选择完整live cohort并为全部
callbacks预建ownership，释放guard后才逐项进入guest。同instance共享execution domain，不同instances不共享global lock；唯一
锁序为execution mutex到短时state guard。

guest trap及logging/weave已知Host trap在释放execution slot前发布Poisoned，具体Host reason与instance/point/classification只保留
为immutable diagnostic。queued same-instance invocation取得slot后取消，fanout其它instance继续；poisoned binding/resource保持
失活但继续占用Exclusive。kernel-private typed try-unload以显式in-flight返回无副作用Busy，零in-flight时原子撤销membership与
bindings，再在guard外析构interpreter/resource。用户要求的模块形状同步完成：原`runtime.rs`成为
`runtime/{mod.rs,invocation.rs}`，属于同一Runtime owner内职责拆分，没有public surface或owner迁移。

owner-local KUnit新增cohort ownership/busy retirement、guest trap poison/queued cancellation/fanout continuation、poisoned
exclusive occupancy/replacement、真实typed callback TID与logging Host handoff，以及production execution capability上的SMP
independent progress。并发case使用Event/predicate phase、真实invocation和完整kthread join，没有sleep、固定yield/tick、timeout
成功oracle、production pause hook、KUnit Host service或test-driven lifecycle state。

**Review / Friction:** 独立change review先发现poisoned-exclusive fixture没有真实Host import trap，并指出Host diagnostic丢失具体
reason；改用late-registration fixture、保留`OutsideLoad`等Copy reason并收紧callback non-poison oracle后复核通过。最终分级
Apollyon 0 / Keter 0 / Euclid 0。Architecture Friction Scan确认published map是唯一state owner，Arc/mutex/diagnostic不决定
lifecycle，provider与Host不穿透runtime private state，cleanup顺序显式，KUnit未塑造production protocol，Stage 5/task/
management/ABI均未提前进入。

**Validation:** `just fmt kernel --check`、`just test xtask`（103/103）、`just test nemophila-wasm`（71 unit、54 integration、
1 doctest、4 focused Miri）、`just test nemophila-module`与`git diff --check`通过。实现矩阵完成KUnit-on RV64/LA64与KUnit-off
RV64/LA64 build；KUnit-on ELF catalog各含两个16-byte conditional descriptors，KUnit-off双架构catalog为空。最后的
Host-diagnostic refinement后重跑RV64 KUnit-on build；维护者接受LA64不因该owner-local Copy diagnostic变化重复build/runtime。

`build/nemophila-stage4-ckpt2-rv64-smp2.log`记录`qemu-virt-rv64-release`、2 HART、`memory=1G`、wrapper生成的pretest rootfs与
从preliminary master重新复制的worktree-local test disk。最终652/652 KUnit通过，其中20项Nemophila case全部通过；真实callback
logging marker可见，SMP case返回`ok`，当前socket LTP profile 6/6，随后进入PowerOff machine action。SMP ENTER/PASS使用`kinfo`
而受普通console policy过滤；维护者接受source lock/lifetime proof、2-HART case进入及完整suite/terminal outcome，不要求为marker
改变日志策略或追加运行，本证据也不声称直接观测mutex waiter挂队。

LA64本checkpoint最终source的focused guest与hardware **Not Run**；LA64 build/ELF证据不外推guest execution。维护者在该明确
proof limit下指示收口。

**Result / Next / Stop:** Stage 4 Checkpoint 2与Stage 4 **Closed**，Contract Cutover仍为None；不更新current contract或register。
Stage 5--6仍未解析或授权，不接入task clone seam、不删除Stage 2 temporary Host fixture、不引入management/artifact ingress或
public ABI，也不声称完整R0 acceptance。

### 2026-08-15 - Stage 4 Feedback Interlude implementation and closure

**Execution Authorization:** 维护者把Stage 4后的软件工程审查与重整授权为一个实施环节，不拆checkpoint；该授权已消费。
Stage 5及后续gate仍未获解析或执行授权。

**Implementation:** 具体clone point从Nemophila通用机制移到task clone owner：`CloneObserver`拥有kernel-internal identity、
`Fanout` policy、canonical WIT consumer names、typed `CloneObservation { Tid, Tid }`及Core Wasm lowering。Stage 4 ordinary build
仍没有production descriptor或call site；KUnit条件下由task owner实际消费通用declaration SPI，证明sibling subsystem不需要
Nemophila增加clone分支。

Nemophila weave收敛为generic `PointSpec`/`Point<P>`、immutable provider catalog、typed-erasure binding与generic registration
Host lowering。最小declarative macro只原子产生typed point capability和descriptor；当前没有proc-macro解析/生成义务，因而不
增加新crate。`CallbackBinding::new::<P>`在擦除前闭合identity、callback signature与context lowering，runtime的cohort/
invocation也改为`P: PointSpec`，不再独立传递clone identity和TID参数。Host API composition继续显式安装canonical clone
registration contract；它不由provider catalog驱动，因此empty catalog仍能link并返回`provider-unavailable`，不会退化为
unknown import。

内部文件按稳定职责目录化：`weave/{mod,catalog,binding,host}.rs`、`host/{mod,logging}.rs`、
`runtime/{mod,invocation,transaction}.rs`。published state/lifecycle、invocation/retirement、unpublished transaction/registration
仍属于同一个Runtime owner，拆分没有移动state truth或扩大crate public API；跨模块composition KUnit保留在最低共同owner的
inline `nemophila::kunits`。ordinary build发现的本轮conditional re-export warning已通过真实`cfg`边界修正；只对等待Stage 5
production consumer的descriptor/macro re-export保留局部`allow`和退出条件。

**WIT Maintenance Decision:** `nemophila/wit/nemophila.wit`继续是logical interface唯一真相；Rust SDK low-level bindings继续由
`wit_bindgen`直接生成。kernel Host adapter因当前interpreter提供Core Wasm Linker而保持集中、手写和窄小，并在WIT、logging
consumer与task point consumer旁记录“同一review change同步修改consumer及真实link/call test”的约束。本阶段不增加WIT hash、
generated kernel schema mirror、admission metadata/runtime parser或完整Host codegen；后续只有在接口规模/变化率形成真实漂移
问题时才独立评估自动生成。

**Review / Architecture Friction:** 最终source audit确认clone-specific identity、policy、native context与WIT point mapping只在
task clone owner；通用runtime/weave没有clone分支，Host composition root的唯一concrete type引用只负责WIT-visible API
composition。published map与transaction map仍分别是published lifecycle和unpublished reservation唯一真相；type erasure不允许
identity与typed callable独立构造，provider/Host无法取得runtime private state。module split没有产生`manager/state/utils`空壳、
第二个registry/schema truth或无consumer facade。普通catalog为空，Stage 5 seam不存在。最终未发现Apollyon、Keter、Euclid或
需要单独记录的Safe friction。

**Validation:** `just fmt kernel --check`、`just test xtask`（103/103）、`just test nemophila-wasm`（71 unit、54 integration、
1 doctest、4 focused Miri）、`just test nemophila-module`、`mdbook build docs`与`git diff --check`通过。KUnit-on
`qemu-virt-rv64-release`/`qemu-virt-la64-release`和KUnit-off `competition-final-rv64-release`/
`competition-final-la64-release`均build通过；KUnit-on catalog为一个16-byte task-owned conditional descriptor，KUnit-off
catalog start/end相等。ordinary build没有本轮Nemophila unused import warning。

`./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img build/nemophila-stage4-feedback-rv64.log`使用repository
wrapper生成rootfs、复制preliminary master disk并运行`smp=1`/`memory=1G` guest。652/652 KUnit通过，其中20项Nemophila
case全部进入并通过；typed callback logging与poison diagnostics可见，socket LTP profile 6/6，随后完成orderly shutdown并
进入PowerOff machine action。

**Result / Next / Stop:** Stage 4 Feedback Interlude **Closed**，Contract Cutover保持None；不更新current contract或register。
LA64 guest/hardware、真实task clone placement、production descriptor/call site、canonical module真实vertical slice、Stage 2
temporary fixture删除、management/artifact ingress/public ABI与完整R0 acceptance均Not Run / Not Proven。Stage 5--6仍未解析或
授权。

### 2026-08-15 - Stage 5 docs-only resolution

**Resolution Authorization:** 维护者明确授权解析Stage 5，并指定本Stage运行RV64即可；该docs-only resolution授权已消费。
Checkpoint 5A实现、Stage 6解析/执行及任何contract cutover仍未授权。

**Preflight:** 核对Accepted R4 target/invariants、Stage 4与Feedback Interlude最终source/evidence、task共同`kernel_clone()`成功路径、
task-owned `CloneObserver` declaration、global Nemophila runtime、canonical module/temporary Host fixture、KernelConfig/SystemTarget/
BuildPreset owner、KUnit execution contract、`user-test`与RV64 repository wrapper。live source确认publish/enqueue后、vfork wait/return前
已有唯一共同窗口；KUnit runner在initial userspace前执行且要求case撤销global publication，也明确不支持创建userspace task，因而
不能让canonical live instance从KUnit case遗留到真实clone oracle。

**Resolution:** Stage 5保持父RFC target、owner、ABI、failure/cleanup、Contract Impact与acceptance不变，不产生R5。实施路线把本Stage
解析为单一Checkpoint 5A：task owner激活唯一production `Fanout` descriptor并在共同成功窗口提交creator/child TID值快照；一个
默认关闭、RV64-only、带Stage 6删除条件的capability-named validation probe从同次repository module build取得fresh canonical
artifact，经现有global `load_and_publish`/try-unload/runtime lifecycle闭合normal fanout、trap/poison、unload/reload，再由显式
`user-test` focused mode和stage-owned wrapper证明真实`clone`/`clone3` seam。probe不进入default/final configuration，不提供syscall、
public identity、第二runtime或通用artifact ingress；Stage 2 fake Host fixture只在真实组合coverage达到替换gate后删除。

**Validation / Contract:** 本次只修改canonical RFC route、状态摘要与transaction；`git diff --check`、`mdbook build docs`是本次
docs gate的validation。kernel/module/app build、KUnit、QEMU、RV64 guest、LA64、hardware、LTP/final harness、management ingress与
Stage 2 fixture删除均Not Run。Contract Cutover为None，不更新current contracts或register，`NEMOPHILA-R0-CUTOVER`仍为Future。

**Next / Stop:** Stage 5现在是**Resolved / Ready / Not Started**，只有维护者新的明确授权才能激活Checkpoint 5A。Checkpoint 5A关闭
后也必须停止；Stage 6仍只保留outline，不能由本resolution或未来Stage 5 closure自动解析、激活或cut over。

### 2026-08-15 - Stage 5 Checkpoint 5A implementation and Stage closure

**Execution Authorization:** 维护者授权完成Stage 5；该授权已消费。Stage 6解析/执行与任何contract cutover仍未授权。

**Implementation:** task-owned `CloneObserver` declaration现在无条件产生唯一16-byte production descriptor；共同
`kernel_clone()`在child publish并enqueue之后、`CLONE_VFORK` wait或普通return之前同步提交creator/child TID值快照。
publication guard已经由`publish`消费，enqueue返回后没有scheduler-private guard跨入callback；task owner只调用typed point，
cohort、execution、trap、poison与retirement仍由唯一global Nemophila runtime拥有。

新增默认关闭、KUnit-off的`nemophila_clone_validation` RV64 feature/KernelConfig/SystemTarget/BuildPreset和private activation。
xtask只在该feature启用时拒绝missing、non-regular或相对module/WIT/SDK inputs陈旧的canonical export，不自动build或repair。
activation以compile-time immutable bytes走现有`load_and_publish`/`try_unload`，依次完成canonical initial load/unload、两个normal
instance reload、synthetic trap poison/unload/reload，并把两个normal instance与fresh trap instance留给真实init/user-test clone。
trap fixture只复用task-owned `PointSpec`的WIT consumer names，没有第二runtime、registry、schema或通用ingress；feature与全部专用
selection、wrapper、synthetic call均带Stage 6正式ingress/management consumer出现时删除的退出条件。

canonical module在真实kernel load中先成功registration，再以第二个registration取得`AlreadyRegistered`并证明被拒callback environment
释放；该路径替代Stage 2 fake Host的dynamic-failure cleanup proof。fake Host workspace已经删除，`just test nemophila-module`收敛为
fresh build/export。`user-test --nemophila-clone-validation`使用现有`fork`和private raw `clone3` adapter分别创建、回收child；stage-owned
wrapper只编排repository module/rootfs/build/QEMU actions、worktree-local disk copy与log oracle。

**Validation:** `just test xtask`（103/103）、`just test nemophila-module`、`just build --preset
nemophila-clone-validation-rv64-release`、`just build --preset competition-final-rv64-release`、`just fmt kernel --check`、
`just fmt modules --check`、`just fmt user-test --check`、`just test nemophila-wasm`（71 unit、54 integration、1 doctest、4 focused
Miri）、`bash -n scripts/run-nemophila-stage5-rv64.sh`与`git diff --check`通过。freshness fence由probe build真实消费；source audit确认
validation feature只有专用config启用，default/final selection关闭，tracked fake Host文件全部删除，WIT、SDK、task consumer names同步。

`build/nemophila-stage5-rv64.log`记录21482-byte fresh canonical artifact、identity 1 load/unload、identity 2/3两个normal live
instances、identity 4 synthetic trap poison并unload、identity 5 trap reload。fixed-string occurrence audit记录三次duplicate registration
cleanup、synthetic TID pair两次、init `1 -> 9`两次、focused clone `9 -> 10`两次、raw clone3 `9 -> 11`两次，以及恰好两次trap
warning；因此identity 5在init clone后poison，未进入后续focused cohorts。日志还记录clone3 child 退出、parent精确匹配并reap task 11、
thread group retirement与PowerOff machine action。

该guest实际完成并关机；初版wrapper随后以`grep -Fc`统计匹配行，因多个printk record相邻在同一serial line而错误返回1。修正后的
oracle改用`grep -Fo | wc -l`统计occurrence。关机开始前最后两条userspace completion/PASS输出也未排空到serial log；wrapper现在
优先接受显式completion marker，缺失时要求同一clone3 child的kernel reap与retirement，再要求PowerOff。现有日志逐项满足这一更直接
oracle，故没有只为取得wrapper零退出重复运行相同guest；这不把总PASS或脚本退出码替代为runtime evidence。

独立probe-off命令`./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img
build/nemophila-stage5-probe-off-rv64.log`通过：完整KUnit显示`All tests passed!`，socket profile 6/6，随后进入PowerOff。probe-on、
probe-off KUnit与competition-final RV64 ELF的`__enemophila_providers - __snemophila_providers`均为`0x10`，即只有一个production
descriptor；final与probe-off binary均没有`NEMOPHILA-STAGE5`、`clone creator=`或duplicate-registration artifact marker。

**Independent Review / Architecture Friction:** 独立review逐项核对notify placement/guards-out、descriptor唯一性、fresh artifact
provenance、probe-off object fence、global runtime单一真相、trap/fanout continuation、fixture replacement与Stage 6退出条件。唯一初始
Euclid是关机边界userspace completion marker可能未排空导致wrapper假阴性；改用同一clone3 child的kernel wait/reap、thread-group
retirement与PowerOff fallback后复核通过，且该oracle比丢失的串行字节更直接。最终Apollyon 0 / Keter 0 / Euclid 0 / Safe 0。
source scan确认published lifecycle仍只有global runtime一份真相，task point只传TID值，validation probe不拥有runtime私有状态；没有
owner穿透、private representation泄漏、public API扩张、默认配置/架构特判、第二registry/schema、无退出条件桥、隐含cleanup顺序或
通过降低oracle换取通过。

**Result / Stop:** Checkpoint 5A与Stage 5 **Closed**，Contract Cutover保持None；不更新current contracts或register。LA64 Stage 5
kernel build/guest/hardware、management authorization、正式embedded/supplied ingress、public identity/errno、final harness、hardware、
完整R0 acceptance与`NEMOPHILA-R0-CUTOVER`均Not Run / Not Proven / Not Cut Over。Stage 6保持Outline Only / Not Started / Not
Authorized，本次严格停止。focused real seam仅为RV64、SMP=1的init/fork/raw clone3；vfork窗口与`CLONE_PARENT` creator规则为
source proof，不是独立guest case。
