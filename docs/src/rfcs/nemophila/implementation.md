# Nemophila R0 实施路线

**状态：** Active
**最后更新：** 2026-08-15
**父 RFC：** [RFC-20260814-nemophila](./index.md)
**当前修订：** R4
**Stage 状态：** Stage 1 / Resolved / Closed；Stage 2 / Resolved / Closed；Stage 2 Feedback Interlude / Resolved / Closed；
Stage 3 / Resolved / Closed；Stage 4 / Resolved / Closed；Stage 4 Feedback Interlude / Resolved / Closed；
Stage 5--6 / Outline Only / Not Started
**Execution Authorization：** Stage 4两个Checkpoint与Feedback Interlude单次实施授权均已消费；Stage 5--6解析/执行均未授权
**Contract Cutover：** None

本页组织父 RFC Accepted R4 Target 的实施顺序、依赖和受保护边界，不另行定义 target、owner、ABI、Contract Impact 或
acceptance。Stage 1 与 Stage 2 已关闭；进入Stage 3前的feedback interlude已经纠正build/admission owner；Stage 3现已解析为
两个共享同一Implementation Boundary的execution checkpoint，现均已关闭；Stage 4也已在不改变R4 target的前提下解析为
两个共享完整lifecycle边界的execution checkpoint，现均已关闭；其Feedback Interlude随后在Stage 5前完成owner surface、
module shape与WIT维护边界重整，但不改变Stage 4语义；Stage 5--6仍只有outline。当前没有Nemophila current contract或cutover。

Stage 1 的 Deliverable、Validation、Cutover 和 Stop / Exit 已按 R2 闭合，execution evidence见
[transaction](../../devlog/transactions/2026-08-14-nemophila.md)。
Stage 2 的 Purpose、Prerequisites、Implementation Boundary、Deliverables、Validation、Cutover 与 Exit / Stop 已闭合，
execution evidence同样见transaction；feedback interlude不重开Stage 2，而是在后续Stage消费其产物前关闭已发现的owner
摩擦与R3 target revision。Stage 3与Stage 4的可执行边界、各自两个checkpoint、validation与stop条件已在下文闭合；Stage 5--6
仍只保留Purpose、Prerequisites和Protected Boundary，只有维护者明确授权解析对应Stage后才补充其可执行边界。关闭一个checkpoint
不自动授权同Stage的下一个checkpoint，关闭一个Stage也不自动授权下一个Stage的解析或执行。

## 全局 Implementation Boundary

- **Target / non-goals：** 交付父 RFC 定义的 in-tree 第一方integer-only Core Wasm interpreter crate、WIT/SDK/artifact toolchain、
  kernel runtime、`weave`、值型日志 service、clone observer vertical slice 与管理面。trusted/good-faith
  module 边界、无 fuel/preemption/force-unload、无 guest-controlled concurrency/shared execution state、无 shared
  compiled artifact、无第二个 point 或其它 Host service、无 module-side cleanup 等 non-goals 保持不变。
- **Owner / handoff / failure / cleanup：** `anemone-kernel/crates/nemophila-wasm` crate 只拥有导入后 interpreter source、
  integer-only Core Wasm parse、validation、translation、execution 与 trap reporting；Nemophila API owner 拥有 WIT logical
  interface，普通module build只拥有manifest/toolchain/fresh candidate/export，canonical module conformance在真实kernel
  consumer出现前由module-local临时fixture证明；task credentials 拥有
  effective `CAP_SYS_MODULE` truth；Nemophila runtime 唯一拥有 admission、instance、registration/reservation、execution
  serialization、in-flight、poison 与 retirement；具体 subsystem 只拥有 point semantics/call site/binding policy，kernel
  logging owner 保留日志 truth。跨 owner 只使用父 RFC 定义的窄 typed handoff，failure 与 cleanup 继续服从 load rollback、
  callback poison quarantine 和无副作用 busy try-unload。
- **First-party source evolution：** 固定 Wasmi `v1.1.0` 只固定可审计的上游源码 provenance，不冻结导入到
  `anemone-kernel/crates/nemophila-wasm` 后的 crate source 或内部 API。该 crate 是贯穿 Stage 1--6 持续演进的第一方 source
  owner；后续 Stage 直接消费仓库中的当前 source，可以在同一 interpreter owner 内修改 crate，并用普通 Git 历史保存变化与
  execution evidence。crate source 修改后必须重跑受影响的既有 interpreter proof 与当前 Stage consumer proof。Stage closure
  关闭当时的 deliverable 和 evidence，不建立并列的 interpreter profile/version/source identity truth。
- **Protected ABI / contract / acceptance：** 保持 start-free artifact、唯一 module-side `load` entry、WIT 单一接口来源、
  load-scoped hierarchical SDK、per-instance interpreter ownership/serial execution、module-decided registration failure、
  runtime-owned reservation/cohort/poison lifecycle、clone observer 的 TID snapshot 与非决策语义，以及 embedded/supplied
  common path 和同一 artifact 的 RV64/LA64 acceptance。`NEMOPHILA-R0-CUTOVER` 前没有 Nemophila effective contract；
  Stage outline 和 RFC Accepted 状态都不发布部分 ABI 或 current semantics。
- **Validation claim：** interpreter crate只证明R4 integer-only Core Wasm correctness、float-bearing input rejection与kernel embedding feasibility；
  普通module build不证明当前kernel compatibility，canonical module fixture只提供阶段性SDK/WIT/interpreter conformance；
  Nemophila runtime分别证明authorization、start/link/typed-entry admission、transactional lifecycle、weave registration/dispatch、trap containment、
  logging handoff 与 clone semantics；最终 Stage 以同一 artifact 的双架构 vertical slice 闭合父 RFC acceptance。R0 不
  外推 execution progress、unload bounded completion、恶意 module DoS containment 或日志持久性。
- **Stop conditions：** 需要改变父 RFC 的 target/non-goals、interpreter/runtime/provider owner、WIT/SDK
  hierarchy、management authority、instance serial model、registration/cohort/poison/unload 语义、clone point、public ABI、
  Contract Impact、acceptance 或 validation claim；需要允许 Core Wasm start、第二个 lifecycle entry、额外 Host service/
  resource handle、guest-controlled concurrency/shared state、shared compiled artifact，或只能通过第二份行为真相、无退出
  条件的兼容桥、test-only production API 或降低 oracle 才能推进。出现这些情况时停止并回到 RFC review / Target
  Renegotiation。

预计项目、目录、模块、内部 API 和测试载体只在对应 Stage 解析时作为非穷举提示补充；本页不维护逐文件 write set、
resolved manifest 或并列实施计划。普通 commit 不形成新 Stage，Stage 数量也不表达实现提交数量。

## Stage 路线图

| Stage | 解析程度 | 目的 | 可见语义 / Cutover |
| --- | --- | --- | --- |
| Stage 1 | Resolved / Closed | 从固定 Wasmi provenance 形成可持续演进的 in-tree `nemophila-wasm` crate | None |
| Stage 2 | Resolved / Closed | 建立 WIT、Rust SDK、Cargo module build 与 canonical artifact，并与 interpreter integration 共同收敛 | None |
| Stage 2 Feedback Interlude | Resolved / Closed | 收拢SDK/config/build owner，移除xtask业务admission mirror，并以R3修正future kernel admission target | None |
| Stage 3 | Resolved / Closed | 以当前第一方 interpreter source 建立 kernel transactional runtime core | None |
| Stage 4 | Resolved / Closed | 闭合 weave、并发调用与完整 instance lifecycle | None |
| Stage 4 Feedback Interlude | Resolved / Closed | 收拢point/provider owner、typed SPI、Host/WIT consumer与Nemophila内部模块边界 | None |
| Stage 5 | Outline Only | 接入真实 clone observer vertical slice | None |
| Stage 6 | Outline Only | 激活 management、完成双架构 acceptance 并原子 cut over | `NEMOPHILA-R0-CUTOVER`（Future） |

## Stage 1 — `nemophila-wasm` 裁剪与适配

**Resolution：** Resolved / Closed

**Execution Authorization：** Consumed

**Purpose：** 先 clone 固定 Wasmi `v1.1.0` 源码，再将实际 interpreter source 导入 Anemone 仓库的
`anemone-kernel/crates/nemophila-wasm`，形成由第一方直接裁剪、适配、维护且会在后续 Stage 继续演进的 in-tree crate。
该 crate 不是 submodule、nested Git repository、外部独立 project、`anemone-kernel/crates/anemos` 下的 crate，也不是依赖
另一个 upstream `wasmi` crate 的 wrapper/adapter。Stage 1 保留 Wasmi 派生的通用 Core Wasm interpreter 能力，提供可由
后续 consumer 使用的 parse/validation/translation/execution/trap substrate，并关闭属于该 crate owner 的源码导入、基础
裁剪与 kernel embedding 可行性。它不冻结 configuration、feature matrix、limits、embedding API 或后续 crate source，
不提前证明 Stage 2 的 canonical artifact 或 Stage 3 的真实 kernel runtime integration。

**Prerequisites：** 父 RFC R2 保持 Accepted；当前没有已提交 Nemophila production code、current contract 或 register
baseline；固定 Wasmi
`v1.1.0` 上游基线及 provenance 可复现。进入本 Stage 时读取 live kernel/build owner 与真实 embedding/toolchain 约束，
并以 commit `8273dfb09d493971b7bb12fe614d740cdc857175` 作为唯一导入起点；维护者另行授权 Stage 1 执行。

**Protected Boundary：** `nemophila-wasm` 只拥有通用 Core Wasm correctness 和每次调用所需的 interpreter substrate；
不得吸收 WIT policy、Host capability、provider catalog、kernel synchronization、instance lifecycle 或 management authority。
裁剪服从真实 embedding、依赖和审计需求，不为追求表面体积而重写无关语义，也不承诺通用外部 compatibility surface。
若出现无法由 source/build 直接回答的具体高风险假设，只在本 Stage 内解析具有明确 hypothesis、failure signal、write-back
和退出条件的最小 probe；probe 失败时停止，不用兼容桥、第二套 validator 或较弱 oracle 绕过。

### Implementation Boundary

- **Target / non-goals：** 从固定 Wasmi clone 导入 interpreter source、license/notices 与可复现 provenance，在
  `anemone-kernel/crates/nemophila-wasm` 形成以 `no_std + alloc` 为 production substrate 的第一方通用 Core Wasm interpreter
  crate 和双架构 kernel-embedding build proof。Stage 1 不建立
  WIT、module SDK、canonical clone
  observer、Nemophila admission/runtime、Host import、provider、management ABI、kernel current contract 或通用外部 Wasmi
  compatibility；不保留 upstream Git metadata，不把导入源码放入 `anemos` 或外部 repository，也不保留 upstream `wasmi`
  作为 production Cargo dependency 再由 `nemophila-wasm` 包装。
- **Crate / source shape：** production dependency graph 只保留通用 interpreter 所需能力，不把 upstream CLI、WASI、C API、WAT
  parser、fuzz/spec runner 或 host-only tooling 带入 kernel consumer；这些组件中的 oracle、fixture 或回归语料可以作为
  crate-local validation input 保留。裁剪以 owner、依赖、审计和 embedding 边界为依据，不要求为了行数或制品体积重写
  已有正确语义。
- **API evolution / validation boundary：** `nemophila-wasm` 保持通用 Core Wasm interpreter 形状；configuration、feature
  support、limits、module metadata 与 embedding API 根据 interpreter 自身能力和后续真实 kernel consumer 共同演进，Stage 1
  不冻结稳定外部 surface。普通 module construction 必须在执行前完成 validation；malformed、invalid 或实现不支持的输入
  返回 error，不能作为已验证 module 进入 executor。production surface不暴露unchecked module construction；private parser
  implementation的unsafe只由已安装Validator的owner-local路径调用，validation-only consumer无法调用它。
- **Runtime boundary：** interpreter API 表达通用 Wasm values、module/instance/store/Host function 与 trap/error；interpreter
  不拥有 kernel call window，也不得持有 `Task`、`File`、
  kernel lock/raw pointer、provider、registration、in-flight、poison 或 retirement state。kernel allocator 下自然、适度的
  有界分配可以由 embedding consumer 提供，Stage 1 不为消除分配引入 pool、镜像状态或新的 resource owner。
- **Safety boundary：** supplied bytes 即使来自 R0 trusted/good-faith caller 也必须作为未验证输入进入 checked parse/
  validation path；production surface 不暴露 unchecked module construction。retained production unsafe code/unsafe impl 必须有
  owner-local invariant audit，release build 保留 Wasmi `extra-checks` 或等价的 executor invariant checks，不能把 translation
  bug 转化为 unchecked UB。该边界不扩大为恶意 module progress/DoS containment claim。
- **Source rule：** Stage 1 交付仓库内可供 Stage 2 直接消费的 `nemophila-wasm` 第一方 source。后续 Stage 可以在
  interpreter owner 内直接修改该 crate，并通过普通 Git 历史记录变化；crate source 改变时重跑受影响 proof。未来真实
  artifact/WIT/admission compatibility 需要多个格式并存时，由对应 artifact/API owner 另行建立版本边界。

### Deliverables

- Anemone 仓库内 `anemone-kernel/crates/nemophila-wasm` 第一方 crate，直接包含从固定 Wasmi provenance 导入并开始裁剪/
  适配的 interpreter source、许可材料和 crate identity；无 submodule/nested Git metadata、`anemos` 归属或 production
  upstream `wasmi` dependency；
- production `no_std + alloc` 通用 Core Wasm interpreter substrate，覆盖 binary parse、validation、translation、execution、
  module inspection、Host function 和 trap/error reporting；
- crate-local upstream/core regression、malformed/type/control-flow rejection 与一般执行正例；不按 R0 observer 的最低需求
  建立 feature allowlist、固定 limits 或专用 wrapper；
- crate-local host test/oracle 与 validation-only `no_std` embedding consumer；后者真实调用普通 production embedding API，
  但不进入 production dependency，也不成为未来 kernel runtime 的临时 facade；
- Stage 1 closure 记录实际 build feature selection、依赖审计结果及验证 evidence；Stage 2 直接从仓库当前第一方 source
  继续，不建立额外的 source handoff 或并列 source authority。

### Validation

- **Provenance / hygiene：** 从干净 Anemone checkout 证明 upstream tag/commit、源码导入关系、license/notices、crate identity 与
  lock/dependency inputs 可追溯；源码树和 dependency audit 证明没有 submodule/nested repository、production upstream
  `wasmi` dependency、无来源代码或未说明的第二份 interpreter implementation。
- **Interpreter correctness：** crate owner tests 覆盖通用 Core Wasm parse/validate/translate/execute 正例、malformed/type/
  control-flow failure、Host value round trip 与 trap reporting；保留适用的 upstream regression corpus，不把“能解析”冒充
  “允许执行”，也不以 canonical observer 覆盖替代一般解释器回归。
- **Owner surface：** source/API/dependency audit 证明production surface没有unchecked constructor，private parser unsafe仍在
  owner-local validation boundary内；production
  graph 不依赖 `std`、WASI、WAT、CLI/C API、host filesystem/thread/random source，
  也不出现 kernel object、同步或 lifecycle owner。
- **Safety：** source audit 枚举 retained production unsafe boundary 及其不变量，证明 untrusted bytes 不能绕过Validator进入
  translation/execution，release configuration 启用 `extra-checks` 或已证明等价的 invariant checks；结合适用的 Miri、fuzz regression
  corpus 与 malformed module tests 覆盖 parser/translator/executor 的安全敏感路径。此证据不能外推为对全部 interpreter bug
  或恶意 module DoS 的形式化证明。
- **Embedding：** validation-only consumer 真实调用普通 production embedding API，并覆盖至少一次 parse、validation、eager
  translation mode、instance execution、Host value round trip 与 trap reporting；同一仓库 source 以 repository Rust
  toolchain 对 `riscv64gc-unknown-none-elf` 和 `loongarch64-unknown-none` 完成 `no_std + alloc` build/link。该证据只证明
  crate-level embedding feasibility，不外推真实 kernel load path、QEMU runtime 或双架构 R0 acceptance。
- **Reproducibility：** execution evidence 记录所有实际命令、toolchain/target identity、Git state、
  result 与 Not Run；Stage 1 不运行 kernel KUnit、QEMU、LTP 或 Nemophila lifecycle tests。

### Cutover

None。Stage 1 不发布 production kernel code、public ABI、visible semantics、current contract 或 register baseline；仓库内
第一方 crate source 是 Stage 2 的普通输入，不是 `NEMOPHILA-R0-CUTOVER`。

### Exit / Stop

Stage 1 在上述 deliverables 与 interpreter/owner/embedding proof 全部闭合后关闭。当前只使用一个 Stage closure checkpoint；
普通 import/cut/test commit 不形成额外 gate。Stage 1 closure 不冻结 `nemophila-wasm` source，后续 Stage 修改 crate 时重跑
受影响 proof，不重新打开 Stage 1。

如果实现需要改变 Wasmi 上游源码基线、interpreter/Nemophila owner 分工或 validation claim，必须停止并
回到 RFC review / Target Renegotiation。如果 `no_std` 双架构 embedding 只能通过吸收 kernel synchronization/lifecycle、复制
validator truth、引入无退出条件兼容桥或降低拒绝 oracle 才能成立，也必须
停止。当前 resolution 不预设 probe；出现 source/build 无法回答的具体高风险假设时，先在本节补全 hypothesis、failure signal、
write-back、code disposition 与 exit 并取得对应授权，再执行最小 probe。

### Result

Stage 1 已关闭，execution evidence 与 Not Run 见
[transaction](../../devlog/transactions/2026-08-14-nemophila.md)。`nemophila-wasm` 已是仓库内第一方 source，Stage 2
直接从当前 source 继续。Stage 2 后续已独立授权并关闭，不改变本 Stage closure evidence。

## Stage 2 — WIT、SDK 与 artifact toolchain

**Resolution：** Resolved / Closed

**Execution Authorization：** Consumed；Stage 3未获解析或执行授权

**Purpose：** 建立由同一 versioned WIT package/world 驱动的 logical interface、Rust module SDK、xtask-owned Cargo module
build 与 canonical clone observer artifact。真实 Rust toolchain output 必须通过共同 artifact envelope 和当前
`nemophila-wasm` 解释执行 proof，形成可供后续 kernel wiring 直接消费的一份 start-free、架构无关 Core Wasm 制品。

**Prerequisites：** Stage 1 已关闭，`nemophila-wasm` 已是仓库内可持续演进的第一方 crate；父 RFC 保持 Accepted R2；live
xtask 仍拥有 repository build/export orchestration，module build 尚无既有 public surface；当前 repository Rust toolchain 支持
`wasm32v1-none`，但 tracked toolchain configuration 尚未声明该 target。开始执行仍需维护者另行授权。

**Protected Boundary：** WIT 只拥有逻辑接口，不驱动 provider availability、binding policy、runtime lifecycle 或 admission。
高层 SDK 保持 load-scoped、按 capability、subsystem/provider 与 point 分层，不暴露 raw import、generic point tag、table
slot、callback token 或其它 callable representation。module build 是与 app build 分离的 owner；它可以借鉴 app build 的
manifest/driver/common-export结构，但不得复用 app manifest、driver type、target/architecture model 或 artifact path。canonical
artifact 不引入 WASI、Component binary、Core Wasm start、额外 Host service、第二个 point 或架构专用 variant。本 Stage 不
接入 kernel runtime、SystemTarget embedded selection、management ABI、live lifecycle 或 current contract。

### Implementation Boundary

- **Target / non-goals：** 交付一份 canonical WIT package/world、一个只服务 Rust module author 的 typed SDK、一个由 Cargo
  构建的 clone observer module、xtask module build/export 路径，以及用当前 `nemophila-wasm` 执行真实产物的 host harness。
  本 Stage 不接其它语言 SDK、其它语言构建链、通用 command/source driver、自动工具链安装、module package marketplace、
  kernel admission/runtime、embedded catalog、SystemTarget 接线或 RV64/LA64 guest execution。
- **Interface owner：** Nemophila API owner 保存唯一 versioned WIT source。Rust SDK 私有消费由 repository-owned
  `wit-bindgen` dependency 从该 source 生成的 bindings，只公开 Rust ergonomic projection 与 guest-side lowering；generated
  binding module、module 和 validation harness 都不得形成第二套手写 imports/exports/value schema。artifact identity/version、
  Host imports、固定 module-side `load` entry、clone callback entry 与 registration result 必须能机械追溯到该 WIT source。
- **Rust callback lowering：** WIT 不表达 function value。point-specific Rust registration API 接收 typed callback，SDK 在当前
  guest instance memory 中保存 pending callback，经 WIT-derived point-specific registration import 请求 Host registration；
  failure 清除 pending slot、释放其中的 callback environment 并把 typed error 交还 module-side `load` 决策，success 将该 slot
  保留到 instance 销毁。Host/runtime
  只按 WIT 约定的固定 callback export/trampoline 调用它，不从 module 获得 raw function/table/token。module load 后不提供
  re-registration/deregistration；failed load 的 guest memory 随整个未发布 instance 销毁，不建立 SDK cleanup protocol。
- **Guest allocation / callback storage：** Rust module 与 SDK 可以使用当前 instance linear memory 内的 guest-local heap；R0
  不承诺 SDK 或 module allocation-free。Rust module build 必须在真实使用 `alloc` 时使最终 artifact 链接一个兼容的 guest
  global allocator；SDK 是否提供默认 allocator、具体 allocator、callback environment 的 inline/heap storage 与 type-erasure
  形状均由 Stage 2 implementation 根据真实 toolchain output 决定。allocation 不得成为新的 Host import/service、kernel object/
  resource handle、跨 instance shared allocator 或 module lifecycle phase。instance 销毁直接回收完整 guest memory，不调用
  callback environment 的 guest `Drop` 或新增 module-side cleanup；SDK 也不要求把 allocation failure 转换为 WIT
  registration error，load/callback 中的 allocator trap 分别服从父 RFC 已有的 load rollback 与 callback poison 边界。
- **Load / initialization：** SDK 提供唯一显式 module-side `load` wrapper，module author 从 load-scoped context 进入 weave 与
  logging capability。Rust/WIT toolchain 需要的静态初始化只能在该 wrapper 的合法调用路径内至多运行一次；不得生成或接受
  Core Wasm start section、`_start`、WASI reactor/command entry 或第二个 module lifecycle entry。固定 callback trampoline 只在
  successful load 后由后续 runtime 调用，普通 Rust/Wasm export 不自动获得 extension semantics。
- **Module build owner：** xtask 增加独立 module manifest、module action 与真实但最小的 `ModuleBuildDriver` trait。trait 只接收
  已验证的 module build context、执行所选工具链并返回本次 invocation 的单一 candidate 及必要诊断；Cargo 是 Stage 2 唯一
  implementation。它不复用 app driver，也不提前抽象其它语言的 target、package manager 或 command model。module manifest
  只声明 module identity、workdir、Cargo build input 与 candidate contract；不声明 Anemone architecture selector。每个 Rust
  module 继续拥有自己的 Cargo package/workspace 与 lockfile，SDK 保持独立构建单元。
- **Cargo route：** Cargo driver 由 xtask 统一选择 repository Rust toolchain、`wasm32v1-none`、固定 module profile 和
  repository-owned `core`/`alloc` build settings，形成一个 Core Wasm candidate；module manifest 或调用者不能把它改成 WASI、host 或
  RV64/LA64 target。缺少 toolchain component/target、Cargo failure 或找不到本次 candidate 都直接返回带 module/driver/context
  的错误，不自动安装、不回退到环境默认 target，也不接受旧 candidate 冒充成功。
- **Common validation / export：** driver 不决定 artifact 合法性或 public output。共同 module build path 要求当前 invocation
  产生恰好一个 fresh ordinary Core Wasm file，使用当前 `nemophila-wasm` 完成通用 parse/validation，并以 canonical WIT
  机械派生的 envelope 检查 interface identity/version、允许的 value-only Host imports、固定 load/callback entries 与生成器
  所需 helper/custom metadata；拒绝 Component binary、start section、WASI/unknown Host import、缺失/额外 lifecycle/callback
  entry 和不匹配的 interface metadata。只有全部检查成功后才导出到稳定的 module-specific `build/` 路径；candidate、export、
  toolchain 与校验结果由共同路径统一诊断。普通 repository clean 清除该 export，format `all` 与 module-specific scope 覆盖 SDK
  和 Rust modules。
- **Interpreter evolution：** Stage 2 直接使用仓库当前 `nemophila-wasm` source。若真实 canonical output 暴露通用 Core Wasm
  parser/validator/executor 缺口，可以在 interpreter owner 内直接修正并重跑受影响的 Stage 1 regression 与本 Stage proof；
  Nemophila envelope 不能复制通用 validator，interpreter 也不能吸收 WIT policy、module manifest、build/export 或 lifecycle
  owner。
- **Protected semantics / validation claim：** 保持父 RFC 的 WIT single source、start-free unique load、hierarchical SDK、
  point-specific typed registration、module-decided registration failure、value-only logging 与同一 artifact 跨架构目标。本 Stage
  只证明 WIT/SDK/build output 自洽、canonical artifact 通过当前 interpreter 并可在 value-only fake Host 下运行；不证明
  Stage 3 的 kernel admission、transaction publication/rollback、reservation、per-instance serialization、poison、unload、
  management authorization 或双架构 runtime acceptance。

预计目录、文件、manifest字段、Rust type 名称和精确命令是非穷举实现提示；实现可在上述 owner 与 protected surface 内自然
决定。`ModuleBuildDriver` trait、Cargo-only implementation、driver/common-validation 分工、单一无架构 module artifact 与 WIT
consumer关系属于本 Stage 已解析边界，不得退化为 app driver 复用、enum-only dispatch、module-local shell wrapper 或手写并列
schema。

### Deliverables

- canonical versioned WIT package/world，以及由 `wit-bindgen` 私有生成、由 SDK 包装的 Rust consumer view；R0 world 只包含 point-specific clone
  registration、value-only logging、唯一 module-side `load` entry 与固定 clone callback contract；
- 独立 Rust SDK 构建单元，提供 load-scoped capability/provider/point hierarchy、typed registration result、guest-local callback
  slot/trampoline 与显式 load wrapper；
- 独立 module manifest/config owner、xtask module build action、最小 `ModuleBuildDriver` trait 和唯一 Cargo driver；repository
  entrypoint 可按 module identity 构建并将校验后的 artifact 导出到 `build/`，format/clean 路径覆盖新增 source/output；
- repository-owned `wasm32v1-none` Rust/Cargo configuration 与 guest-local allocation support，使真实 `core`/`alloc` consumer
  artifact 可链接并执行；不依赖 WASI、host libc、架构 selector、Host allocation service 或 module-local build wrapper；
- 使用 SDK 的 canonical Rust clone observer module：load 时显式注册 clone point，callback 接收 creator/child `u32` TID values
  并通过 typed logging capability 记录它们；同一次 build 只产生一份跨 RV64/LA64 复用的 Core Wasm artifact；
- artifact envelope inspector/validator 与 `nemophila-wasm` host harness。fake Host 只实现 WIT 定义的 value boundary，不成为
  production runtime facade、provider catalog 或 lifecycle owner。

### Validation

- **WIT / generated consumer：** WIT parser/`wit-bindgen` tests 证明 package/world/version、imports/exports、registration result、
  TID/log value shape 能由同一 source 解析并生成 private consumer view；source audit 不存在 SDK/module/harness 手写的第二套 schema。Rust SDK
  以 `no_std + alloc` 形状对 `wasm32v1-none` 构建，module author surface 只暴露 load-scoped hierarchy 与 typed callback。
- **Guest allocation：** 真实 Rust module artifact 至少执行一次 guest-local allocation、使用与释放，并由
  `nemophila-wasm` host harness 观察正常结果；source/link/import audit 证明 allocator 与 callback storage 没有引入 Host
  allocation import、kernel/resource handle、跨 instance state 或额外 lifecycle entry。若 callback environment 使用 heap
  storage，registration failure test 还须证明 pending environment 被释放；instance teardown 不外推 guest `Drop`/cleanup proof。
- **Callback lowering：** focused SDK/module tests 分别让 fake Host 返回 registration success/failure，证明 success 后固定 callback
  trampoline 调用保存的 typed callback，failure 清除 slot并把结果交给 module load，普通 export 不注册 callback。callback
  invocation 传入两个边界 `u32` TID values并观察相同日志值；测试不得借助 raw callback token、table index或第二个 lifecycle
  entry。
- **Build owner：** manifest/xtask tests 覆盖 identity/workdir/Cargo input、唯一 driver selection、缺失 target/toolchain、Cargo
  failure、missing/stale/non-regular candidate、common diagnostics 与成功 export。source review 确认 trait 是 module-local small
  boundary，Cargo 是唯一 implementation，app config/driver/architecture model 未被复用或扩大，也没有未消费的其它语言 driver。
- **Artifact envelope：** 对真实 Cargo output 检查其为 Core module而非Component binary，且无start/WASI/unknown Host imports；
  WIT-derived identity/version、允许 imports、固定 load/callback entries、helper/custom metadata 与candidate freshness全部匹配。
  malformed/envelope-invalid fixtures须分别由interpreter validation或Nemophila artifact检查的正确owner拒绝，不能把两者合并为
  一套validator。
- **Interpreter execution：** common path把导出的canonical artifact重新交给当前`nemophila-wasm` parse/validate/instantiate；
  value-only fake Host运行module-side `load`、registration success/failure、clone callback与logging，覆盖normal return和module-
  visible registration error。该harness只证明真实toolchain output与interpreter/SDK ABI相容，不外推kernelcall window、
  reservation、rollback/publication、trap poison或unload lifecycle。
- **Repository integration：** repository-owned module build与format入口从干净candidate state成功，导出一份fresh ordinary
  artifact到`build/`；重复build不把旧candidate当作新结果，standard clean移除export。记录toolchain/target、WIT identity、
  candidate/export和全部实际结果及Not Run。

### Cutover

None。Stage 2 不发布 kernel runtime、SystemTarget embedded selection、public management ABI、visible semantics、current contract
或 register baseline；`build/` 下的 canonical artifact 和 Rust SDK 仍是后续 Stage 的开发输入，不是
`NEMOPHILA-R0-CUTOVER`。

### Exit / Stop

Stage 2 在上述 WIT/SDK、Cargo-only module build、canonical artifact envelope、真实 interpreter execution、format/clean 与 owner
audit 全部闭合后以一个 closure checkpoint 关闭；普通 implementation commits 不形成额外 gate。Stage 2 closure 不自动授权
Stage 3 的解析或执行。

如果实现需要改变父 RFC 的 interface/lifecycle target、暴露 raw callback representation、允许 start/WASI/Component artifact、
增加第二个 lifecycle entry/point/Host service、让 WIT 或 build metadata 驱动 runtime policy、把 app driver/architecture model
变成 module truth、以第二套 validator 或 stale candidate 降低 oracle，必须停止并回到 RFC review / Target Renegotiation。若
`wasm32v1-none` 与真实 Rust SDK 只能通过上述退化才可用，也必须停止；仅在既有 owner 内调整 interpreter API、Cargo flags、
generated binding glue、manifest/file layout 或内部 type 不触发停止条件。

### Result

Stage 2 已关闭。canonical `anemone:nemophila@0.1.0` WIT、`no_std + alloc` Rust SDK、Cargo-built clone observer、独立
module manifest/driver/common validation path 与 stable `build/modules/clone-observer/` artifact 已交付。WIT-derived load
success/failure 与 point-specific registration result 保持分离；clone `Fanout` registration surface 不预置 exclusive-only
outcome；SDK 的 callback environment 在 registration failure 先释放，成功后保留到 instance memory 被整体销毁。bindgen 的
per-export constructor workaround 已禁用，artifact 无 Core Wasm start、WASI、Component binary、额外 Host import 或第二个
lifecycle entry。

共同 build path 每次使用新的 candidate 目录，在同一 byte snapshot 上完成 WIT envelope、当前 `nemophila-wasm` eager
validation/translation、fake-Host execution 与原子 export。host harness 覆盖 load 前 callback trap、registration success、
边界 `u32` TID logging、provider unavailable、pending environment release 与 failure 后 callback rejection。interpreter
owner 同时把原独立 collections/IR implementation crates 内收到 `nemophila-wasm` 私有模块，保留 `nemophila-wasm-core`
独立 crate；这不改变 interpreter/Nemophila owner 分工。

完整命令、artifact identity、independent review、Architecture Friction Scan 与 Not Run 见
[transaction](../../devlog/transactions/2026-08-14-nemophila.md)。本 Stage 没有 kernel runtime、SystemTarget、management ABI、
visible semantics、current contract 或 `NEMOPHILA-R0-CUTOVER`；Stage 3 仍为 Outline Only，未获解析或执行授权。

## Stage 2 Feedback Interlude — Build / admission owner correction

**Resolution：** Resolved / Closed

**Execution Authorization：** 维护者已授权本次纠偏与最终独立审阅；Stage 3仍未获解析或执行授权

Stage 2关闭后的software-engineering review发现，通用`module build`无条件解析canonical WIT、执行精确artifact envelope并
运行clone-observer fake Host harness，使xtask同时决定具体Nemophila API、当前kernel compatibility和一个业务module的
acceptance。这是Keter owner穿透：build system拥有了runtime admission的镜像，unknown API/额外export/custom section即使
会由future kernel自然拒绝或忽略，也不能产生普通build output。SDK同时把bindings、lifecycle、logging、task/clone point、
callback slot与export glue放在单一`lib.rs`，且通用`Module::load`直接依赖clone-specific registration error；manifest又增加
当前没有consumer的Cargo package selector，并缺少repository参考模板。

这些finding改变父RFC原先的artifact/admission target，因此不是保持R2的内部route correction。维护者接受R3：ordinary
build只交付candidate，kernel runtime才是安全与兼容性enforcement owner；本interlude不重开Stage 2，也不授权Stage 3。

### Implementation Boundary

- **SDK owner shape：** 在同一Rust SDK owner内按`bindings`、`lifecycle`、call window、`services::logging`、
  `weave::task::clone_observer`与export glue拆文件/namespace；保留既有module-author调用层次。通用module lifecycle只把
  module-local error降为wire-level unit error，不依赖任何具体point的registration error；clone callback slot/trampoline只
  留在clone point owner。
- **Manifest / build owner：** 新增带注释的`conf/module.toml`作为manifest参考；删除`build.package`与Cargo`--package`，由
  选定Cargo manifest自身拥有package/workspace default。xtask继续验证identity、相对path、fresh/missing/non-regular/multiple
  candidate，固定repository toolchain/target/profile，读取同一byte snapshot并原子export；它不解析Wasm/WIT、不实例化或
  执行业务module，也不判断当前kernel能否load。
- **Temporary Host fixture：** canonical clone observer在真实kernel Host wiring出现前保留一个module-local fake Host fixture，
  只证明当前SDK lowering、WIT ABI、guest allocation/callback slot与interpreter execution自洽。它是Stage 2到Stage 5的临时
  seam，不是generic validator、production dependency或admission facade；Stage 5真实runtime/provider路径覆盖同一load、
  registration success/failure、callback、logging与failure behavior后必须删除。若届时仍有无法经production path证明的纯SDK
  obligation，应重新审查并留下更窄test，不能默认保留整套fake Host。
- **R3 kernel admission：** 两种artifact ingress仍共同调用`Module::new`完成interpreter-owned validation/eager translation；
  runtime必须在实例化前通过`has_start()`拒绝start section，只向Linker注册R0 capability，并对module-side`load`及实际callback
  entry做typed lookup。malformed/unsupported Core Wasm、start、unknown/signature-mismatched import、missing/wrong-typed
  required entry都在publication前失败；WIT metadata、精确imports/exports集合与custom-section allowlist没有runtime consumer，
  不进入admission。额外export/custom section被忽略。
- **Failure classification：** module-side`load`返回error或trap时完整rollback且不发布instance，不进入`Poisoned`；只有live
  callback的module-caused trap进入poison quarantine。无限循环/长期不返回不由build、WIT metadata或fixture证明安全，继续
  位于R0 fuel/preemption non-goal。
- **Toolchain target：** repository继续通过`rust-toolchain.toml`固定toolchain与builtin`wasm32v1-none` target。该组合是module
  toolchain的单一target-spec truth；不在`conf/arch`复制一份JSON，也不把architecture-neutral module引入Platform arch模型。
  只有future consumer需要偏离builtin语义时，才由module toolchain owner提出替代spec与相应target review。

### Validation / Cutover / Stop

定向validation覆盖xtask manifest/driver/fresh export、真实canonical Cargo build、module-local fixture的load前trap、
registration success/failure、边界TID logging与failure后callback rejection，以及SDK/module/fixture format。source/dependency
audit确认xtask不再依赖`nemophila-wasm`、`wasmparser`、`wit-component`、`wit-parser`或`wat`，普通build也没有WIT、envelope或
harness入口。最终独立review先后发现并关闭SDK根级flat re-export与driver common contract泄露Cargo profile/field两项Euclid；
复核最新diff无残留Apollyon、Keter、Euclid或值得记录的Safe finding。Architecture Friction Scan确认build/runtime admission、
SDK infrastructure/service/point、driver contract/implementation与临时fixture/production replacement各有唯一owner和显式退出
条件。本interlude已Closed并停在Stage 3前；完整命令与Not Run见transaction。

Contract Cutover为None：没有kernel runtime、management ABI、visible semantics、current contract或register baseline。
本interlude只纠正Stage 2产物owner及future Stage 3 target，不能把host fixture外推为kernel admission/lifecycle evidence。

## Stage 3 — Kernel transactional runtime core

**Resolution：** Resolved / Closed

**Execution Authorization：** Checkpoint 1与Checkpoint 2均已消费

**Purpose：** 在 kernel 内接入当前 `nemophila-wasm` 与 WIT Host consumer，建立共同 artifact
admission、per-instance interpreter ownership、恰好一次 module-side `load` entry、值型日志 call window，以及
unpublished rollback / atomic live publication 的 runtime core。

**Prerequisites：** Stage 1、Stage 2与Stage 2 Feedback Interlude关闭，当前interpreter source与WIT interface可由kernel
integration直接消费，canonical artifact可作为相同接口边界的回归与后续vertical-slice evidence；它在Stage 4 weave capability
接入前不要求成功kernel load。进入本Stage时从父RFC management envelope与live task/ABI owner解析management-to-runtime
内部handoff和proof route，public management ABI仍留待Stage 6激活。R4已把interpreter收敛为integer-only profile，并授权
Checkpoint 1同时闭合kernel/app compiler-target owner分离：kernel使用repository-owned soft-float target spec，Cargo app继续
使用Rust builtin hard-float target，native userspace ABI与FPU context能力不变。

**Protected Boundary：** interpreter validation 与 Nemophila admission 不能互相替代或形成第二份 feature truth；每次 load
独占完整 interpreter entity，不共享 Engine/code/cache。runtime在实例化前拒绝start，narrow Linker与required typed entry
lookup拥有实际compatibility判定；不得恢复WIT metadata、精确imports/exports集合或custom-section allowlist。module load error/trap 只能完整 rollback，日志诊断不能伪装 live
publication。真实 kernel embedding 暴露的 interpreter 修改仍回到 `nemophila-wasm` owner，并在本 Stage 关闭前重跑受影响
interpreter/kernel proof；不得把 kernel object、同步或 lifecycle state 下沉进 interpreter。本 Stage 不公开 management ABI、
不接入真实 subsystem point，也不提前建立 current contract。

### Implementation Boundary

- **Target / non-goals：** 在 `anemone-kernel/src/nemophila/` 建立与 `task`、`fs`、`net` 等并列的kernel顶层Nemophila
  subsystem，并完成source-neutral artifact snapshot到published instance的真实kernel transaction。Stage 3只接入
  module-side `load`与值型logging Host service；不实现weave provider/catalog、registration/reservation、callback invocation、
  execution serialization、in-flight、poison、try-unload/retirement、clone call site、embedded catalog、SystemTarget、public
  management ABI或current contract，也不新增runtime KernelConfig feature。Stage 4--6的对象与namespace不能以placeholder、
  fake provider或无consumer facade提前进入。
- **Top-level owner shape：** kernel root把`nemophila`注册为独立顶层subsystem；其内部admission、load transaction、instance、
  runtime publication与Host lowering默认保持owner-private，只向未来management/provider consumer暴露所需的最窄
  crate-internal capability。`nemophila-wasm`继续是独立第一方interpreter crate，kernel通过普通Cargo dependency直接消费，
  不把interpreter源码、通用Wasm type或validator复制进`nemophila`，也不把Nemophila lifecycle、kernel lock或Host resource
  下沉到interpreter。
- **Compiler-target owner：** `conf/arch`中的kernel-only target spec拥有kernel code model、ISA与soft-float ABI，普通kernel Rust
  code不得由LLVM生成FPU register use；architecture FPU owner保留显式、受控的user-context save/restore assembly。Cargo app
  driver使用toolchain已安装的builtin bare-metal hard-float target，LA64继续显式关闭`ual`以保持2K1000部署边界；
  `ANEMONE_TARGET_TRIPLE`仍是Command app的artifact identity，不冒充Cargo compiler target。该拆分不改变native app ABI、
  Platform architecture、SystemTarget或kernel public surface。
- **Management-to-runtime handoff：** Stage 3面向未来caller的最终load-and-publish入口只接收一次调用期间稳定、kernel-owned的
  immutable artifact byte snapshot，并在Checkpoint 2返回kernel-private identity或typed internal failure；Checkpoint 1的
  owner-private transaction边界只返回unpublished instance或typed internal failure。两者均不接收`Task`、credentials、user
  pointer、file、artifact source enum或loader process lifetime。未来embedded与supplied入口必须先由各自owner取得同样的
  immutable snapshot，management boundary完成operation-local `CAP_SYS_MODULE`检查后再调用共同load-and-publish路径。Stage 3
  不实现capability check、用户拷贝、artifact catalog或errno mapping，也不缓存`trusted`状态。
- **Per-load interpreter ownership：** 每次load从当前interpreter configuration新建完整Engine/Module/Linker/Store/Instance
  execution entity，调用`Module::new`完成checked parse、validation与eager translation；任何Engine、translated code、Store、
  execution stack或compiled artifact都不在instances或reload之间共享。具体interpreter内部type组合与API可以随真实kernel
  consumer在`nemophila-wasm` owner内自然演进，但kernel-side owning instance必须清楚持有其完整lifetime。
- **Admission order：** runtime先调用checked `Module::new`，随后在任何实例化前通过`has_start()`拒绝Core Wasm start，再用只
  注册Stage 3真实支持能力的narrow Linker完成import resolution/typed signature检查，实例化start-free module，最后对唯一
  module-side `load` entry执行typed lookup。malformed/unsupported Core Wasm、start、unknown/signature-mismatched import、
  missing/wrong-typed load entry都在进入module lifecycle或publication前失败。额外export与custom section被忽略；runtime不解析
  WIT metadata，不比较精确import/export集合，也不建立custom-section allowlist或第二套validator。
- **Integer-only profile：** 当前interpreter configuration固定拒绝`f32`/`f64`类型及相关指令，kernel admission不复制
  float opcode扫描或第二套feature truth。float-bearing input与其它interpreter-unsupported module一样在`Module::new`失败；
  native userspace float、architecture FPU context与module artifact portability不由该profile改变。
- **WIT / logging Host boundary：** kernel Host wiring按canonical WIT的logging import identity、四级level与Core Wasm canonical
  ABI完成私有lowering，并由WIT-derived fixture/conformance evidence防止手写schema漂移；WIT source不在runtime被解析为policy。
  Host先检查guest pointer/length range、整数转换、UTF-8与level，再把borrowed value提交给现有`debug::printk` owner；无效lowering
  形成module-caused Host trap/load failure，不能panic kernel。Nemophila不复制log ring、record、policy或presentation，不为消息
  建立第二份持久buffer；printk继续拥有compile/runtime filtering、record bound、UTF-8 truncation、ring overwrite与console
  presentation。已提交或被过滤的日志均不改变load结果，已提交记录在后续rollback时不撤销，也不能作为publication oracle。
- **Unpublished transaction / cleanup：** load transaction直接拥有全部unpublished interpreter entity与Stage 3 Host context；在
  instance构造完成后恰好调用一次typed module-side `load`。normal success只产生可被commit消费的unpublished instance；module
  返回error、module/Host trap或其它pre-commit failure都直接销毁transaction-local entity且不产生identity/live entry。
  Stage 3没有rollback action stack、generic resource ledger、guest cleanup entry或补偿callback；当前唯一可回滚资源就是自然
  所有权下的完整unpublished interpreter entity，logging不是rollback resource。load error/trap不产生Poisoned instance。
- **Atomic publication / identity：** runtime owner持有唯一published-instance collection。Checkpoint 2的commit消费完整
  unpublished instance，分配对已销毁instance不可别名的kernel-private identity，并在一个owner-controlled publication点把
  identity与instance一起变为可见；identity representation与未来ABI encoding仍属Stage 6。identity reservation、collection
  insertion或其它commit前失败不留下可查identity或半published instance。相同artifact的重复load每次重新validate/translate并
  产生独立identity/entity；runtime不保存artifact来源或mutable `loaded`镜像。
- **Synchronization boundary：** Stage 3只有单次load entry，没有真实callback、并发management consumer或unload，因此不提前
  冻结per-instance execution lock、cohort/in-flight protocol或retirement ordering。published collection的最小同步只保护本Stage
  的atomic commit与owner-local observation；Stage 4必须在不制造第二份live truth的前提下扩展同一个runtime/instance owner，
  不能用Stage 3缺少外部并发为由改变父RFC的instance-serial target。
- **Validation shape：** deterministic admission/load/publication tests使用真实production transaction与runtime owner。小型Core
  Wasm validation artifacts只保留在owner-local conditional KUnit fixture中，覆盖Stage 3的load/logging ABI与failure shape，
  不登记为第二个Nemophila product module、不进入ordinary module build/export，也不形成production validation facade。
  tests按被测owner内联；跨admission/load/publication的composition cases位于`nemophila`最低共同owner的inline
  `#[cfg(feature = "kunit")] mod kunits`，不新增独立`kunit.rs`/`tests.rs`。KUnit可以构造隔离runtime owner并在case结束时销毁
  整个test-local fixture；该cleanup不形成production retirement或try-unload proof。
- **Build / activation boundary：** Stage 3通过现有Justfile/xtask/preset与architecture wrapper验证kernel dependency、KUnit-on与
  KUnit-off ordinary build及真实guest execution；不增加平行Cargo/QEMU wrapper。subsystem代码进入ordinary kernel source，但
  没有boot initcall、SystemTarget、syscall或其它production caller，因而Stage 3 closure仍不激活用户/boot可见runtime capability。

预计`anemone-kernel/src/nemophila/`及其owner-local admission/load/instance/runtime/Host模块、kernel Cargo dependency、定向fixture
与测试是非穷举实现提示，不是严格逐文件write set。实现可以在上述owner内自然拆分，但不得扩大kernel root public surface、
引入`manager/state/utils`式无职责命名空间，或为后续Stage预建public trait、generic registry与resource abstraction。

### Execution Checkpoints

#### Checkpoint 1 — Kernel embedding 与 unpublished load transaction

- **Purpose / deliverable：** 建立顶层`nemophila`subsystem、kernel到当前`nemophila-wasm`的真实dependency、Stage 3 logging Host
  lowering，以及从immutable bytes经过checked admission、start rejection、narrow linking、typed load lookup到恰好一次module
  load的完整unpublished transaction。success停在可消费但不可观察的unpublished instance；所有failure直接rollback。
- **Independent safety：** 没有runtime collection、published identity、boot/management/provider caller或current contract；普通
  kernel行为保持中性。checkpoint不能退化为只有目录/Cargo接线的scaffold，也不能通过fake weave import让canonical observer
  假成功。canonical artifact若作为negative integration case进入kernel admission，必须按真实Stage 3 capability因尚未实现的
  weave import被拒绝，不能被特判绕过。
- **Validation / stop：** owner-local KUnit覆盖valid load、module error、trap、logging、malformed/unsupported/start、unknown或错型
  import、missing/wrong-typed load、float-bearing input、额外export/custom section及failure后无identity/publication；interpreter
  source/API若变化则重跑全部受影响crate proof。kernel ELF审计必须证明普通Rust text没有native FPU instruction，显式arch FPU
  save/restore symbol单独allowlist；双架构native `float-test`至少完成build，RV64完成guest runtime。至少完成双架构KUnit-on/
  KUnit-off build与一条RV64真实KUnit boot；LA64 runtime若未运行必须明确Not Run。
  Checkpoint 1独立review/写回后停止，不能自动进入Checkpoint 2。

#### Checkpoint 2 — Runtime owner 与 atomic publication

- **Purpose / deliverable：** 在Checkpoint 1 transaction之上建立唯一runtime collection与kernel-private non-aliasing identity，
  让内部load入口在module success后消费unpublished instance并原子publish；failure保持collection/identity不可见。两次load同一
  bytes必须形成两个独立interpreter entity与identity，额外export/custom section仍不影响publication。
- **Independent proof：** KUnit composition case直接调用真实runtime load/commit路径，以test-local runtime owner观察commit前后
  collection，并在case返回前销毁整个isolated fixture；测试不得给production transaction增加pause/hook/reset/inspection API，
  不把fixture teardown表述为try-unload/retirement。source audit闭合唯一owner、publication线性化点、identity不别名、failure
  cleanup和per-load interpreter ownership。
- **Validation / stop：** 完成RV64与LA64 KUnit真实boot、两架构ordinary KUnit-off build、interpreter/module regression、format、
  docs、dependency/source/conditional-surface audit与Architecture Friction Scan。两个wrapper必须核对focused marker、完整KUnit与
  architecture-appropriate terminal outcome：RV64由ordinary machine handler退出；LA64在current contract仍无ordinary handler
  时，完成orderly subsystem shutdown并进入唯一末尾halt后由host终止QEMU。LA64 disposition不能外推为power-off capability，
  wrapper success marker也不能替代对末尾halt路径的核对。Checkpoint 2关闭整个Stage 3，但不授权Stage 4，不形成public
  management ABI、visible semantics、current contract或`NEMOPHILA-R0-CUTOVER`。

#### Checkpoint 1 关闭结果

Checkpoint 1已关闭。kernel顶层`nemophila`subsystem通过owner-private接口完成checked construction、start rejection、narrow
linking、typed `load` lookup/call、value-only logging Host lowering与直接unpublished rollback；interpreter validator固定拒绝
float-bearing input。kernel/app compiler target已分离，双架构kernel使用repository-owned soft-float JSON，Cargo app使用builtin
bare-metal target，Command app artifact identity与native userspace FPU ABI保持不变。最终ELF审计仅在architecture FPU/LSX
context owner的显式load/save/control symbols中发现相关指令，普通Rust、Nemophila与lwext4 text均未使用native FP。

双架构KUnit-on/KUnit-off build、interpreter/module/xtask regression、双架构native `float-test` build、RV64 repository wrapper、
source/visibility/conditional-surface audit、format/docs/whitespace与独立review均通过；RV64运行中638项KUnit（含6项Nemophila
case）及当前socket LTP profile 6/6通过并正常关机。LA64 runtime、hardware、management、weave、publication、concurrency、
unload、clone与完整R0 acceptance均Not Run / Not Proven。Contract Cutover保持None；该次关闭严格停止在Checkpoint 1，
Checkpoint 2后续另行授权后的当前状态见下节。

#### Checkpoint 2 关闭结果

Checkpoint 2授权已消费。实现新增唯一production `Runtime` collection与单调kernel-private identity；同一个
`BTreeMap<InstanceIdentity, RuntimeInstance>`同时拥有publication membership与完整per-load interpreter entity，map insertion
是唯一publication线性化点。checked construction和module-side `load`在runtime lock外完成；commit前failure不改变collection或
identity cursor。重复load同一bytes每次重新构造解释器entity并取得不同identity。两个owner-local KUnit composition case直接
使用隔离runtime owner，覆盖重复load的独立publication以及失败后collection/cursor保持；没有production pause、reset、snapshot
hook或Stage 4 lifecycle placeholder。独立review最初发现KUnit observation无条件放宽`RuntimeInner`可见性的一项Euclid；最终
实现恢复ordinary-build私有表示，只保留conditional owner-local只读snapshot，该finding已neutralize且未留下其它架构摩擦。

`just fmt kernel --check`、`just test xtask`（103项）、`just test nemophila-wasm`（71项unit、54项integration、1项doctest、
4项focused Miri与双架构embedding build）、`just test nemophila-module`、双架构KUnit-on build、双架构tracked-final KUnit-off
build、`git diff --check`与`mdbook build docs`均通过。RV64 wrapper在`smp=1`、`memory=1G`下完成640/640 KUnit（含8项
Nemophila）与当前socket LTP profile 6/6，随后
通过machine handler正常退出。LA64 wrapper在相同topology下完成643/643 KUnit（含8项Nemophila）与同一LTP profile 6/6，
随后完成orderly filesystem/network/device shutdown并按`SYSTEM-POWER-MACHINE-001`到达
`no power off handler succeeded, halting the system`，再由host `Ctrl-A x`终止QEMU。wrapper success marker本身不是proof；
日志同时证明全部guest evidence、orderly shutdown顺序与唯一末尾halt路径。

register的`ANE-20260726-SYSTEM-POWER-ARCH-COVERAGE`与current contract都明确LA64没有ordinary power-off/reboot machine
handler。维护者确认上述host termination是该平台合理且预期的Stage 3 harness终点；该target-preserving validation disposition
不修改Nemophila target、owner、ABI、acceptance或current contract，也不声称LA64具备power-off capability。Checkpoint 2与
Stage 3据此关闭，Contract Cutover仍为None，Stage 4仍未获解析或执行授权。wrapper使用当前开发者预先存在的
`kernel_symbols=false` local default tuple；该配置改动不属于Nemophila write-back，也不纳入本checkpoint变更。hardware、
management authorization、两种artifact ingress、weave、callback并发/poison、try-unload、clone与完整R0 acceptance仍
Not Run / Not Proven。

### Deliverables

- kernel顶层`nemophila`subsystem及其owner-private admission、load transaction、instance、runtime publication与Host lowering；
- kernel对当前第一方`nemophila-wasm` production configuration的直接dependency，每次load独占完整interpreter entity；
- source-neutral immutable artifact snapshot handoff、checked admission顺序、唯一module-side `load` call与直接unpublished rollback；
- WIT-conformant、value-only logging Host import到现有printk owner的窄handoff，无kernel panic、第二日志truth或rollback resource；
- kernel-private non-aliasing instance identity、唯一runtime collection与atomic publication；
- owner-local conditional Core Wasm fixtures与inline KUnit coverage，不形成第二product module、test-only production API或未来Stage
  placeholder；
- Stage 3 execution evidence、Not Run矩阵、两个checkpoint各自的review/Architecture Friction结论与最终Stage状态写回。

### Validation

- **Admission owner：** valid、malformed、type/control-flow invalid、unsupported与start-bearing fixtures分别由interpreter validation
  或Nemophila start policy的正确owner拒绝；unsupported必须包含float-bearing module；source audit确认kernel不调用unchecked constructor、不复制validator、不解析WIT
  metadata/精确envelope，额外export/custom section不阻止load。
- **Link / WIT / Host：** exact logging import/signature与typed load entry通过，unknown/signature-mismatched import、missing/wrong
  load entry失败；WIT-derived fixture与source audit覆盖level/result/string ABI。invalid pointer/range/UTF-8/level形成contained module
  failure而非kernel panic；允许与filtered logging都不改变module result，失败load留下的已提交诊断不被当作publication。
- **Transaction / cleanup：** success entry恰好调用一次，module error、guest trap、Host trap及commit前failure都不留下identity、
  instance或Host resource；unpublished cleanup不进入guest、不运行第二lifecycle entry、不产生Poisoned state。failure injection不得
  依赖production KUnit hook，能由输入和真实owner API触发的failure才形成proof。
- **Publication / ownership：** commit前collection不可观察该instance，commit后identity与完整instance同时可见；repeated load
  分配non-aliasing identity并持有独立Engine/code/Store/stack，failed load不产生可见identity。KUnit isolated-owner teardown只
  证明fixture cleanup，atomic publication仍结合production source/lock/lifetime review，不外推Stage 4 retirement。
- **Interpreter regression：** 使用repository-owned `just test nemophila-wasm`重跑production `no_std + alloc + extra-checks`、
  integer-only interpreter、unsupported-float、trap与双架构embedding proof；任何Stage 3触发的interpreter source/API修正都由crate owner提交并记录影响，
  不建立Nemophila专用fork/profile。
- **Module/API regression：** 使用`just test nemophila-module`证明canonical WIT/SDK/artifact与Stage 2临时fixture继续自洽；该host
  fixture仍不是kernel runtime evidence，且直到Stage 5真实registration/callback路径达到替代gate前不得删除。Stage 3不要求
  canonical clone observer在缺少weave provider时成功kernel load。
- **Repository integration：** 只使用repository-owned format/build/QEMU/end-to-end入口。KUnit-on的
  `qemu-virt-rv64-release`与`qemu-virt-la64-release`完成双架构build；KUnit-off tracked final presets证明conditional fixture
  不进入ordinary dependency；Checkpoint 1至少运行RV64 repository wrapper，Checkpoint 2运行RV64/LA64 wrappers并核对focused
  Nemophila marker、全套KUnit marker与architecture-appropriate terminal outcome。RV64必须由ordinary machine handler退出；
  LA64当前完成orderly subsystem shutdown并进入current-contract末尾halt后由host终止。wrapper同时产生的用户态/LTP结果只按
  实际profile报告回归，不自动成为Nemophila proof或完整R0 acceptance。
- **Proof limits：** 每个checkpoint记录实际preset、architecture、CPU topology、feature tuple、artifact/fixture identity、命令、
  result与Not Run。Stage 3不声称management authorization、两种artifact ingress、weave registration/dispatch、instance
  concurrency、poison、unload、clone semantics、hardware、恶意module progress/DoS或最终RV64/LA64 vertical slice。

### Cutover

None。Stage 3只交付dormant kernel-internal runtime core与proof；没有boot/management/provider入口、public ABI、visible semantics、
current contract、register baseline或`NEMOPHILA-R0-CUTOVER`。Checkpoint 1无publication，Checkpoint 2只完成Stage 3内部atomic
publication，不是R0 semantic/contract cutover。

### Exit / Stop

Stage 3使用两个execution checkpoint；两者共享本节完整target、owner/handoff、failure/cleanup、protected surface、validation
claim与Cutover。Checkpoint 1必须独立安全且保持现有visible semantics/current contract中性；Checkpoint 2关闭整个Stage 3。
每个checkpoint都需要独立授权、review、execution evidence与Architecture Friction Scan，普通implementation commits不形成第三
checkpoint。维护者只授权Checkpoint 1时，关闭后必须停止。

如果实现需要引入fake/临时weave provider、generic resource ledger、第二份WIT/envelope/validator truth、shared interpreter
entity/cache、`Task`/credentials/file/user pointer穿透、boot或public management入口、新KernelConfig/SystemTarget capability、
额外Host service/resource、test-driven production hook，或者需要在两个checkpoint之间重新解析identity、publication、failure、
cleanup、owner、ABI、acceptance或validation claim，必须停止并回到RFC review / Target Renegotiation。真实kernel embedding要求的
interpreter内部API/source调整可以留在既有crate owner并重跑受影响proof；若只有扩大interpreter owner或降低admission oracle才能
继续，同样停止。Stage 3关闭后仍须等待维护者另行授权解析Stage 4。

## Stage 4 — Weave、并发调用与完整 lifecycle

**Resolution：** Closed / Checkpoint 1 Closed / Checkpoint 2 Closed

**Execution Authorization：** Checkpoint 1 Consumed；Checkpoint 2 Consumed

**Purpose：** 在 Stage 3 transactional core 上闭合 typed point/provider handoff、registration/reservation、fanout cohort、
per-instance serial execution、in-flight ownership、无副作用 busy try-unload、callback trap poison/cancellation 与 live/
poisoned retirement，使 runtime correctness 在接入 task owner 前独立成立。

**Prerequisites：** Stage 3 关闭且 load/rollback/publication owner 已稳定；Stage 2 的 point-specific SDK contract 可由 kernel
consumer 真实接线；维护者另行授权对应execution checkpoint。

**Protected Boundary：** provider 不持 instance、binding collection、in-flight 或 lifecycle truth；runtime 不发明 point
cardinality；poison 不是自动 unbind/unload，busy failure 不做部分 cleanup，同一 instance 不并发解释、不同 instances 不被
全局串行。不得为验证建立第二个 production point、无真实 consumer 的 public facade 或 KUnit-aware production protocol。

### Implementation Boundary

- **Target / non-goals：** 扩展Stage 3同一个kernel-internal `Runtime` owner，使module-side `load`能够通过canonical
  `weave-clone` WIT import取得transaction-local binding reservation，并使published instance能够经typed point capability完成
  cohort dispatch、per-instance serial execution、trap containment、poison/cancellation和显式try-unload/retirement。Stage 4
  不修改task clone路径，不加入ordinary production provider descriptor、embedded/SystemTarget或supplied artifact ingress、
  `CAP_SYS_MODULE`检查、syscall/errno/public identity encoding、runtime KernelConfig feature或current contract；不删除Stage 2
  temporary Host fixture，也不声称canonical clone observer vertical slice。Stage 5拥有真实task point与fixture替换，Stage 6
  拥有management activation、artifact ingress和最终cutover。
- **Provider declaration / catalog：** 具体subsystem通过最窄crate-internal provider declaration静态贡献point identity、typed
  call-window shape与immutable `Exclusive` / `Fanout` policy；贡献由Nemophila拥有的专用linker catalog汇聚，普通初始化顺序、
  link order和descriptor地址都不形成point identity、availability或dispatch order。descriptor不包含instance、callback、binding
  count、reservation、in-flight或lifecycle state，也不执行注册副作用。runtime唯一负责catalog校验与解析；duplicate/invalid
  descriptor是kernel build/invariant failure，不能伪装为module registration result。具体attribute/macro拼写和catalog内部索引
  属于implementation preference，但不得退化为字符串tag、开放`WasmValue`数组、动态initcall注册或subsystem-owned callback
  collection。Stage 4 ordinary build允许catalog为空；Stage 5才由task owner贡献唯一R0 production point。
- **Provider / runtime handoff：** provider consumer只持由其typed declaration导出的point invocation capability并提交一次
  point-specific value context；它不接收`Runtime`、instance identity、binding snapshot、interpreter object或runtime lock。
  runtime解析point-owned immutable policy、选择cohort并完成callback lowering/dispatch；provider不观察module trap或以其修改
  subsystem业务结果。R0 typed point只允许从可睡眠、interrupt-enabled且没有owner-private guard的普通task context同步调用；
  IRQ/IRQ-off/NMI point、decision-returning callback与异步fanout均不属于本Stage，Stage 5必须在真实clone seam复核该调用上下文。
- **WIT / registration Host boundary：** kernel narrow Linker在Stage 3 logging基础上只增加canonical WIT的point-specific
  `weave-clone.register-observer` import，并对fixed clone callback entry执行typed lookup；WIT source仍只拥有logical identity、
  version、values和registration result，不在runtime被解析为catalog/policy。registration只在当前module-side `load` call window
  合法；callback或其它entry中直接调用同一import属于module-caused Host trap，不能创建late binding。Host call window只向
  registration lowering暴露当前load transaction的窄reservation capability，不把完整runtime、provider、task或kernel lock
  存入interpreter；窗口在guest entry返回或trap后失效，不成为instance lifecycle的第二份phase truth。
- **Registration / reservation：** runtime以point identity和当前unpublished instance为key检查provider availability、同一
  instance重复registration及point-owned policy。`Exclusive`与已有live/poisoned binding或其它transaction reservation冲突；
  `Fanout`仍为每次successful registration建立属于该transaction、publication前不可dispatch的reservation。同一instance对同一
  point至多一个reservation/binding。provider unavailable、duplicate或exclusive occupied返回WIT typed dynamic failure且不改变
  runtime state；module可以把它视为fatal并让load rollback，也可以接受后返回success，发布零binding instance。callback export
  缺失/错型、非法call window或Host lowering错误是contained module failure，不降格为普通registration result。
- **Transaction / publication / rollback：** reservation与Stage 3 unpublished interpreter entity属于同一个runtime-owned load
  transaction。successful registration返回前已经排除commit-time occupancy conflict；module error/trap或其它pre-commit failure
  释放本transaction全部reservations并销毁完整unpublished entity，不留下identity、binding或Poisoned state。module success后，
  runtime在同一短临界区使identity、owning instance与全部bindings一起从unpublished变为live；不存在先发布instance再补binding、
  先暴露binding再补instance或commit时重新拒绝已成功exclusive registration的窗口。Stage 3 monotonic identity与per-load
  interpreter ownership保持不变，runtime不保存artifact source或mutable `loaded`镜像。
- **Authoritative lifecycle state：** Stage 3唯一published collection继续是instance membership/lifetime truth，并由同一runtime
  owner扩展出live/poisoned admission、bindings/reservations与显式in-flight accounting。实现可以用runtime-minted stable
  instance/invocation capability在短临界区外保护内存lifetime，但引用计数、mutex occupancy、diagnostic reason和provider catalog
  都不得反向决定lifecycle、busy或retirement。identity不复用；retiring可以由已经撤销collection/binding visibility的owning
  retirement capability表达，不要求为了叙事增加可被第二处修改的镜像enum。
- **Cohort / invocation ownership：** 一次point call在runtime state的单一cohort selection线性化点筛选该point当前全部live
  bindings，并在执行任一callback前为每个binding建立显式invocation ownership和in-flight计数。与publication/retirement竞争的
  binding要么不进入cohort，要么已计入in-flight；unpublished、poisoned与retiring instance不可准入。runtime随后释放catalog/
  collection state guard，再按无contract的内部顺序逐项dispatch；正常返回只释放对应ownership，一个instance trap仍继续cohort
  中其它instances。cohort不得同时占有多个instance execution slots，也不得逐callback重新查询而使同一次binding set漂移。
- **Per-instance execution / lock ordering：** 每个published instance拥有独立、可睡眠的execution serialization capability，
  保护其Store/Instance/stack等mutable interpreter island；同一instance已经admit但尚未执行的callbacks等待该串行域，不同
  instances不共享execution lock。任何runtime catalog/collection/binding spin guard都必须在等待execution slot和进入guest前
  释放，guest execution、Host logging与可能的诊断格式化都不能发生在`spin_lock_irqsave`临界区。trap路径允许在继续持有当前
  instance execution slot时短暂取得runtime state guard发布poison；反向路径不得持runtime state guard等待execution slot，
  由此固定唯一lock order。module-side `load`在unpublished transaction内完成且尚不可并发dispatch，不需要用global execution
  lock把不同loads串行。
- **Trap / poison / cancellation：** typed callback normal return继续live；只有interpreter明确分类的同步guest trap或由非法
  guest value/call window造成的module-side Host trap进入module-caused callback failure。unexpected linker/type/runtime invariant
  error、assertion或kernel panic不能按“任何`Err`”统一降格为Poisoned；若当前`nemophila-wasm`分类surface不足，可以在同一
  interpreter owner内做最小API演进并重跑受影响proof。module-caused trap必须在释放execution slot前原子发布`live -> poisoned`
  并保留triggering invocation直到containment/diagnostic cleanup完成；其它cohort已经admit但尚未进入guest的同instance callbacks
  在取得串行域后重新检查authoritative lifecycle、取消并释放ownership，不得进入guest。poison不撤销binding/resource、不释放
  `Exclusive`占位、不自动unload或恢复live；reason、instance/point identity与trap classification只能作为immutable diagnostic
  snapshot，行为继续只由lifecycle state驱动。
- **Try-unload / retirement：** Stage 4只提供kernel-internal typed try-unload operation，不检查credentials、不映射errno也不
  暴露identity ABI。runtime在同一state临界区解析identity并检查authoritative lifecycle/in-flight：任一admitted/waiting/
  executing invocation或poison cancellation/containment未完成时返回busy-class failure，且不改变lifecycle、bindings、resources、
  exclusive occupancy或admission；零in-flight的live/poisoned instance则在同一线性化结果中关闭新admission、撤销全部bindings、
  从published collection取出并形成owning retirement capability。可能运行复杂`Drop`的interpreter/resources在可见性撤销后、
  runtime state guard外销毁；cleanup不进入guest、不调用`exit`/`fini`、不允许module veto，也不靠偶然最后一个引用承担状态转换。
  not-found与busy保持不同kernel-internal结果，最终public encoding留给Stage 6。
- **Validation shape：** deterministic registration/lifecycle cases直接消费production transaction、catalog、cohort、invocation和
  retirement owner；owner-local conditional provider descriptors可以分别表达clone-shaped `Fanout`与synthetic `Exclusive`
  policy，但不进入ordinary build、不建立第二个production point或另一套registry。小型Core Wasm fixtures覆盖真实registration
  import、typed callback、normal return、guest/Host trap和logging；pure owner protocol tests可以直接驱动production内部的
  reservation/invocation capabilities，但不得增加pause/hook/reset、fake lifecycle field或generic native-callback abstraction。
  只有same-instance serialization、cross-instance progress和queued cancellation等被测并发语义允许使用KUnit kthreads；双方必须
  以具名phase/predicate/Event和真实invocation/retirement capability握手，所有worker在case返回前stop/join，不以sleep、固定yield/
  tick或host timeout作为成功oracle。actual concurrency correctness仍由state/lock/lifetime source proof闭合，SMP stress只作补充。
- **Build / activation boundary：** Stage 4继续通过repository Justfile/xtask/preset与现有QEMU入口验证，不增加parallel wrapper。
  ordinary kernel source包含完整internal protocol，但没有production provider descriptor、task call site、boot/management caller、
  public ABI或current contract；KUnit-off build必须证明conditional descriptors/observations/concurrency fixtures不进入ordinary surface。
  Stage 4 closure不删除module-local temporary Host fixture，因为它的Stage 5 replacement gate要求canonical module走真实task point。

预计`anemone-kernel/src/nemophila/`内按weave declaration/catalog、registration transaction、published instance、invocation/lifecycle
与Host lowering等稳定角色自然拆分，kernel linker catalog边界、必要的owner-local interpreter API演进、inline KUnit fixtures和
双架构验证配置是非穷举提示，不是严格逐文件write set。若继续把catalog、load、dispatch、lifecycle和tests全部堆进现有
`mod.rs`/`runtime.rs`会混合多套职责，可以在同一Nemophila owner内做行为保持的目录化拆分；不得借拆分扩大crate public API、
建立通用plugin framework或提前移动task/management owner surface。

### Execution Checkpoints

#### Checkpoint 1 — Typed weave registration 与 transactional binding

- **Purpose / deliverable：** 建立immutable provider catalog与crate-internal typed provider/runtime handoff，把canonical WIT
  `weave-clone` registration/callback entry接入Stage 3 narrow Linker；successful registration取得transaction-local reservation，
  module success把identity、instance与bindings原子publish，module error/trap与dynamic failure的fatal分支完整rollback。ordinary
  build仍无production provider descriptor；provider unavailable可被module接受并形成零binding live instance。
- **Independent safety：** 没有point invocation、callback execution/in-flight、poison、try-unload、task call site或management caller；
  ordinary kernel不存在可触发binding的production descriptor。conditional provider fixtures只服务owner-local KUnit并在isolated
  runtime owner内证明真实transaction，case teardown不能表述为retirement。Checkpoint 1不得为让canonical module假成功而
  硬编码provider availability、跳过callback typed lookup或把reservation延迟到commit。
- **Validation / stop：** owner-local proof覆盖catalog empty/duplicate boundary、provider unavailable、same-instance duplicate、
  `Exclusive` live/reservation conflict、`Fanout`多reservation、typed callback缺失/错型、load-window外registration trap、
  successful commit、fatal rollback与accepted failure后的零binding publication；source audit确认unpublished binding不可dispatch、
  publication只有一个owner/线性化点且ordinary build没有conditional descriptor。重跑interpreter/module regression、双架构
  KUnit-on/KUnit-off build与至少RV64真实KUnit boot；LA64 runtime未运行时明确Not Run。Checkpoint 1独立review/写回后停止，
  不能自动进入Checkpoint 2。

#### Checkpoint 2 — Cohort invocation、poison 与 retirement

- **Purpose / deliverable：** 在Checkpoint 1 bindings上建立single-selection cohort和全部callback-before-execution的invocation
  ownership，以独立sleepable instance serial domain执行真实typed Wasm callback；闭合normal return、trap-before-slot-release
  poison、queued cancellation、fanout continuation、busy try-unload以及live/poisoned zero-in-flight retirement与guest-free cleanup。
- **Independent proof：** deterministic owner-local cases覆盖publication/unload race的二选一结果、cohort稳定性、全部ownership
  预建立、normal cleanup、trap classification、poison retained occupancy、queued cancellation和busy无副作用；真实Wasm fixtures
  覆盖normal/trap/logging call window。live concurrency cases只通过production execution/invocation capabilities与Event/predicate
  phase驱动，证明same-instance serial和different-instance independent progress；至少一条真实`smp=2` guest run进入这些focused
  cases，`smp=1`或未进入case的总PASS不能冒充SMP proof。source review必须核对没有global lock跨guest、没有runtime-state-to-
  execution反向等待、explicit in-flight而非引用计数决定busy，以及withdraw-publication-before-drop cleanup。
- **Validation / stop：** 完成RV64与LA64 focused KUnit真实boot、两架构ordinary KUnit-off build、至少RV64 `smp=2` concurrency
  boot、interpreter/module/xtask regression、format/docs、ELF/linker catalog、dependency/visibility/conditional-surface audit与
  Architecture Friction Scan。wrapper或QEMU success marker必须结合focused markers、完整KUnit结果与architecture-appropriate
  terminal outcome核对。Checkpoint 2关闭整个Stage 4，但不授权Stage 5、不接入task clone seam、不删除temporary Host fixture，
  也不形成public management ABI、current contract或`NEMOPHILA-R0-CUTOVER`。

#### Checkpoint 1 关闭结果

Checkpoint 1授权已消费。实现新增typed clone-observer provider declaration与Nemophila-owned immutable linker catalog；point
identity来自固定kernel-internal数值而非link order或descriptor地址，descriptor只保存immutable point policy与callback shape。
ordinary build不贡献production descriptor；conditional KUnit分别贡献clone-shaped `Fanout`与synthetic `Exclusive` provider。

kernel narrow Linker现消费canonical `weave-clone.register-observer` identity并执行fixed `observe-clone(i32, i32)` typed lookup。
`HostContext`只持load-scoped `RegistrationWindow` capability，module-side `load`返回或trap后立即关闭，不能成为published instance
的第二份phase truth。runtime-owned transaction record是reservation唯一真相源；dynamic unavailable/duplicate/exclusive conflict
无副作用返回typed result，module决定其是否fatal。commit在同一runtime state guard内移走transaction bindings并插入完整owning
instance，使identity、instance与全部bindings共用一个publication线性化点且不做late policy recheck；rollback先撤销transaction
membership，再在spin guard外销毁callback/store/interpreter entity。

owner-local KUnit覆盖catalog empty/duplicate/invalid、provider unavailable、duplicate registration、`Fanout`并存、`Exclusive`
pending/live冲突、callback缺失/错型、load-window外trap、successful commit、accepted failure零binding publication，以及成功取得
reservation后的module error与guest trap完整rollback。独立review最初发现fatal rollback fixture使用empty catalog、没有实际取得
reservation的一项Euclid；补充真实registration后error/trap case并核对完整runtime snapshot后已neutralize。最终分级为
Apollyon 0 / Keter 0 / Euclid 0。

`just fmt kernel --check`、`just test xtask`（103/103）、`just test nemophila-wasm`（71项unit、54项integration、1项doctest、
4项focused Miri与双架构embedding build）、`just test nemophila-module`及`git diff --check`通过。KUnit-on
`qemu-virt-rv64-release`与`qemu-virt-la64-release`、KUnit-off `competition-final-rv64-release`与
`competition-final-la64-release`均build通过。KUnit-on双架构ELF的provider catalog均恰含两个16-byte conditional descriptor；
KUnit-off LA64 ELF中catalog start/end相等，确认ordinary build为空。

RV64 wrapper在`smp=1`、`memory=1G`下完成647/647 KUnit，其中15项Nemophila case全部进入并通过；当前socket LTP profile
6/6通过，随后经ordinary machine handler进入PowerOff machine action。LA64 runtime与hardware Not Run；双架构build/ELF证据
不外推LA64 guest execution。Checkpoint 1据此**Closed**，Contract Cutover保持None。Checkpoint 2现在Ready / Not Started且未获
执行授权；本次严格停止，不进入cohort/in-flight、execution serialization、poison/cancellation、try-unload/retirement、Stage 5
clone seam、management/public ABI或current contract。

#### Checkpoint 2 与 Stage 4 关闭结果

Checkpoint 2授权已消费。published map继续是membership、live/poisoned lifecycle、bindings与显式in-flight accounting的唯一
行为真相；每个entry另持独立sleepable execution mutex，Arc与mutex occupancy只保护内存lifetime和解释器串行，不决定busy或
retirement。一次point call在同一state guard内筛选全部live bindings并为整个cohort预建ownership，随后释放guard逐项执行；
同instance等待同一execution domain，不同instances没有global execution lock。锁序固定为execution mutex到短时runtime state
guard，guest、Host logging与诊断格式化均不发生在state spin guard内。

guest trap与已知module-caused Host trap在仍持execution slot时发布不可逆Poisoned；Host diagnostic保留具体reason以及
instance/point identity和Guest/Host classification，但这些字段不参与admission、replacement或retirement。已经admit但排队的
同instance invocation取得slot后重新检查lifecycle并取消，fanout中其它instances继续。`try_unload`用显式in-flight判断
`Busy`，busy路径无副作用；零in-flight时在一个state临界区撤销membership与全部bindings，Store/interpreter/resource随后在
guard外析构。runtime owner按稳定职责从`runtime.rs`目录化为`runtime/mod.rs`与`runtime/invocation.rs`，没有移动owner、扩大
public ABI或建立平行runtime。

新增owner-local cases覆盖cohort全部ownership、busy/not-found与retirement、guest trap poison/queued cancellation/fanout
continuation、poisoned exclusive occupancy及replacement、真实typed callback的TID value与logging Host window，以及真实
per-instance execution capability上的SMP independent progress。并发case只使用具名phase、Event/predicate、production
invocation/execution capability和完整kthread join；没有sleep、固定yield/tick、timeout成功oracle、production pause hook、KUnit
Host service或测试驱动的lifecycle字段。

独立change review最初发现poisoned-exclusive fixture误调用本地`load`而不能形成Host trap，并指出Host diagnostic丢失具体reason；
fixture改为真实late-registration import，diagnostic改为保留`OutsideLoad`等owner reason，logging case也直接断言callback返回后
没有in-flight或poison。复核后分级为Apollyon 0 / Keter 0 / Euclid 0。Architecture Friction Scan确认没有第二份状态真相、
provider/Host owner穿透、私有表示泄漏、为测试扩大production API、无退出条件临时桥、隐含failure/cleanup顺序或通过降低ABI
诚实性换取通过。

`just fmt kernel --check`、`just test xtask`（103/103）、`just test nemophila-wasm`（71项unit、54项integration、1项doctest与
4项focused Miri）、`just test nemophila-module`和`git diff --check`通过。实现矩阵完成双架构KUnit-on build与双架构KUnit-off
build；ELF audit中KUnit-on catalog均为两个16-byte conditional descriptors，KUnit-off catalog start/end相等。最后的
Host-diagnostic refinement后重跑RV64 KUnit-on build；维护者接受LA64不为该owner-local Copy diagnostic变化重复build/runtime。

`build/nemophila-stage4-ckpt2-rv64-smp2.log`使用`qemu-virt-rv64-release`、`smp=2`、`memory=1G`及wrapper生成的pretest
rootfs与从preliminary master重新复制的worktree-local disk。QEMU报告2 HART，guest完成652/652 KUnit，其中20项Nemophila
cases全部通过；typed callback日志marker可见，SMP case实际返回`ok`，socket LTP profile 6/6，最后进入PowerOff machine action。
SMP case的`kinfo` ENTER/PASS受普通console policy过滤；维护者明确接受production lock/lifetime source proof、2-HART case进入与
完整suite/terminal result，不要求为marker改变日志级别或追加运行，也不把本证据描述为直接观测mutex waiter挂队。

LA64本checkpoint最终source的focused guest与hardware **Not Run**；LA64 build/ELF证据不外推guest execution。维护者在该明确
proof limit下指示收口。Checkpoint 2与Stage 4据此**Closed**，Contract Cutover保持None；不更新current contract或register，
也不接入task clone seam、不删除Stage 2 temporary Host fixture、不引入management/artifact ingress/public ABI。Stage 5--6仍未获
解析或执行授权。

### Deliverables

- point-owner static descriptor与Nemophila-owned immutable provider catalog，以及不暴露runtime/instance/private lock的typed
  provider invocation capability；
- canonical WIT `weave-clone` registration Host wiring、typed callback lookup与load-only call-window enforcement，无第二份WIT/
  policy truth；
- runtime-owned transaction-local reservations、dynamic registration result、fatal/accepted failure分支与identity/instance/binding
  atomic publication/rollback；
- 唯一published lifecycle/binding/in-flight owner、stable invocation ownership、single-selection fanout cohort与per-instance
  sleepable serial execution；
- module-caused callback trap classification、pre-slot-release poison、queued cancellation、retained-inert bindings/resources与
  diagnostic-only poison snapshot；
- kernel-internal无副作用busy try-unload与live/poisoned guest-free retirement，identity继续不复用；
- owner-local deterministic/KUnit concurrency evidence、Not Run矩阵、两个checkpoint各自的review/Architecture Friction结论与
  最终Stage状态写回。

### Validation

- **Catalog / provider boundary：** source与linker audit证明descriptor只含immutable point-owned facts，catalog identity不依赖
  link order/address，runtime而非provider拥有lookup/binding；duplicate/invalid descriptor是kernel invariant failure。ordinary build
  无production point，conditional `Fanout`/`Exclusive` fixtures不泄漏；provider调用只提交typed values且不获得instance/runtime
  representation。
- **Registration / transaction：** provider unavailable、same-instance duplicate、exclusive live/poisoned/reservation occupied均为
  无副作用typed failure；fanout允许多个instances。successful registration建立不可dispatch reservation，fatal module return/trap
  释放全部reservation且无publication，accepted failure发布零binding instance，successful commit同时发布identity/instance/bindings
  且不存在late conflict。
- **WIT / callback / Host：** exact registration import与callback signature通过，missing/wrong callback、load-window外registration、
  invalid Host value形成contained module failure；normal callback可以使用Stage 3 logging call window。kernel不解析WIT metadata/
  exact envelope，普通export不自动注册，module-visible capability仍只有weave与value-only logging。
- **Cohort / concurrency：** 两个fanout live instances在单一selection点都取得invocation ownership后才执行第一个callback；load/
  retirement竞争不产生漂移或use-after-free。same-instance calls串行、different instances不由global lock串行，cohort不同时持有
  多个execution slots，callback order不进入asserted contract。live-scheduling tests有明确phase、predicate、cleanup与实际SMP
  disposition，source/lock proof仍是并发correctness主证据。
- **Trap / poison：** guest trap与module-caused Host trap在execution slot释放前发布Poisoned，triggering ownership保持到containment
  结束；同instance已经admit但未进guest的callbacks取消并释放，新的admission失败，fanout其它instances继续。kernel/interpreter
  invariant error不降格为poison；diagnostic snapshot不驱动lifecycle，poisoned binding/resource保持失活并继续exclusive占位。
- **Unload / cleanup：** live和poisoned instance在in-flight或cancellation/containment cleanup期间返回busy且状态、binding、resource、
  occupancy完全不变；零in-flight在一个线性化结果中关闭admission、撤销binding和published membership，再在state guard外销毁
  interpreter/resource。not-found与busy区分，不等待callback、不进入guest、不auto/force unload、不调用module cleanup，也不以
  strong-count或`Drop`偶然决定retirement。
- **Regression / repository integration：** 使用repository-owned `just test nemophila-wasm`、`just test nemophila-module`与
  `just test xtask`保持interpreter、WIT/SDK/canonical artifact和build owner自洽；任何interpreter error-classification API变化重跑
  受影响proof。双架构KUnit-on/KUnit-off build与RV64/LA64真实boot核对focused/full-suite/terminal outcome，至少一个RV64
  `smp=2` run真实进入concurrency cases；不修改现有end-to-end wrapper来伪装tuple，使用repository `just qemu`显式binding时记录
  完整preset、disk/rootfs来源和log。
- **Proof limits：** Stage 4证明kernel-internal provider/registration/invocation/poison/unload protocol及实际执行的architecture/
  topology，不证明真实task clone call placement、canonical observer production registration、management authorization、embedded/
  supplied ingress、public ABI、hardware、callback progress/unload bounded completion、恶意module DoS或完整R0 acceptance。Stage 2
  host fixture仍只证明module-local conformance，Stage 4 synthetic/conditional provider也不能冒充Stage 5 task vertical slice。

### Cutover

None。Stage 4交付仍无production point/caller的kernel-internal weave与完整lifecycle protocol；没有task/clone visible semantics、
management ABI、artifact ingress、current contract、register baseline或`NEMOPHILA-R0-CUTOVER`。Checkpoint 1只扩展transactional
publication，Checkpoint 2只关闭internal invocation/retirement correctness，二者都不是R0 semantic/contract cutover。

### Exit / Stop

Stage 4使用两个execution checkpoint；两者共享本节完整target/non-goals、provider/runtime handoff、transaction/lifecycle owner、
failure/cleanup、protected surface、validation claim与Cutover。Checkpoint 1必须独立安全且不发布可调用production point；
Checkpoint 2一次性关闭invocation、poison/cancellation与retirement，不能把会执行callback但尚无完整trap/unload protocol的
中间态作为Stage deliverable。每个checkpoint都需要独立授权、review、execution evidence与Architecture Friction Scan；维护者
只授权Checkpoint 1时，关闭后必须停止。

如果实现需要让provider持有binding/lifecycle state、让runtime发明point policy、按link order形成callback order、以字符串/
untyped value建立主要SPI、把global spin guard或IRQ-off窗口跨guest、用reference count/mutex/diagnostic字段决定busy/retirement、
把任意interpreter `Err`降格为poison、在busy path做partial cleanup、引入blocking/force/auto unload、module cleanup、第二个
production point、KUnit-only Host service/pause hook/native-callback abstraction、public management/task/ABI surface，或者需要在两个
checkpoint间重新解析reservation、publication、cohort、poison、cancellation、retirement、owner、acceptance或validation claim，
必须停止并回RFC review / Target Renegotiation。若真实proof只能通过降低SMP/guest oracle、修改production control flow理解测试
协议或把Stage 5 clone seam提前引入，同样停止。Stage 4关闭后仍须等待维护者另行授权解析Stage 5。

## Stage 4 Feedback Interlude — Owner surface、模块形状与WIT维护边界

**Resolution：** Resolved / Closed

**Execution Authorization：** 单次实施授权已消费；不拆execution checkpoint

**Purpose：** 在Stage 5消费Stage 4机制前，修正最小实现留下的软件工程摩擦：clone概念进入Nemophila通用runtime/Host，
point声明macro固化单一point，callback identity与typed callable可被分开传递，flat `weave.rs`/`host.rs`与过载
`runtime/mod.rs`缺少稳定职责边界，以及canonical WIT与手写kernel consumer之间的维护规则不够显式。本interlude不重开或
修订Stage 4 target，只对保持既有registration、publication、cohort、poison与retirement语义的内部owner/API/module shape负责。

### Implementation Boundary

- **Point/provider owner：** 具体point的kernel identity、binding policy、native observation context、WIT consumer identity和
  lowering属于实际subsystem；Nemophila只拥有`PointSpec`/typed `Point`机制、descriptor catalog、registration、dispatch和
  lifecycle。task clone因此定义`CloneObserver`与包含typed `Tid`的`CloneObservation`，而不是让通用runtime理解
  creator/child TID。Stage 4 ordinary build仍不贡献production descriptor；task owner只在KUnit条件下声明provider，以证明
  sibling subsystem能够消费SPI。真实provider activation与clone成功路径调用仍严格属于Stage 5。
- **Declaration/API shape：** 保留最小声明macro，只让它原子地产生typed point capability与immutable linker descriptor；
  descriptor只编码point identity/policy，动态callback和全部lifecycle state继续由runtime拥有。当前没有需要token parser、derive、
  代码生成诊断或跨crate发布的语法义务，因此不引入proc-macro crate。`CallbackBinding::new::<P>`把point identity、Wasm typed
  callable和context lowering在擦除前闭合；runtime selection、cohort与invocation均以`P: PointSpec`表达，clone分支和free-function
  TID参数不再进入通用机制。
- **Host composition / provider availability：** canonical WIT Host contract仍由Nemophila显式composition root安装；具体point的
  registration lowering泛化为`install_point::<P>`。Host contract不能由当前provider catalog反向决定是否安装，否则empty catalog
  会把合法registration的`provider-unavailable`结果错误变成unknown import link failure。catalog只决定编译进kernel的provider
  availability/policy；Host API composition与runtime availability保持两个不同职责，但不形成并列lifecycle truth。
- **WIT maintenance：** `nemophila/wit/nemophila.wit`继续是logical interface唯一真相。Rust SDK low-level bindings继续由
  `wit_bindgen`直接生成；kernel因当前first-party interpreter只提供Core Wasm Linker而保留集中、窄小的手写Host consumer。
  本阶段采用同一review change内同步修改WIT、kernel consumer及真实link/call tests的人工审查策略，并在WIT与consumer旁记录
  约束；不增加generated kernel schema mirror、WIT hash、admission metadata、runtime parser或完整Host codegen。只有后续接口
  数量/变化率使手工映射成为可观测维护问题时，才以独立边界评估生成方案。
- **Module/file shape：** `weave`目录按typed SPI/composition、descriptor catalog、typed callback binding与Host registration
  lowering拆为`mod.rs`、`catalog.rs`、`binding.rs`、`host.rs`；`host`目录按API composition/context/trap classification与logging
  lowering拆为`mod.rs`、`logging.rs`；`runtime`目录按published runtime state、invocation/retirement与unpublished
  transaction/registration拆为`mod.rs`、`invocation.rs`、`transaction.rs`。这些都在原owner内保持行为，不扩大crate public
  surface。跨load/Host/runtime/lifecycle的composition KUnit继续位于最低共同owner `nemophila/mod.rs`；不为减小行数机械建立
  `tests.rs`、`kunit.rs`、`utils`或无义务facade。
- **Protected surface / stop：** 保持canonical WIT/SDK可见surface、load/registration result、provider catalog layout约束、
  transaction publication/rollback、cohort/in-flight/serialization、poison/cancellation、try-unload与cleanup顺序；不删除Stage 2
  temporary Host fixture，不接入task clone seam，不引入management/artifact ingress/public ABI/current contract。若重整需要
  owner迁移、WIT/SDK语义变化、新registry、另一份schema/状态真相或降低既有validation oracle，必须停止并重新分级。

### Result / Validation / Stop

实现已经把具体clone point移入task clone owner，并将通用weave、Host与runtime按上述稳定角色目录化；旧的flat
`weave.rs`/`host.rs`和clone-specific runtime入口已消失。WIT-visible Host composition仍显式包含当前R0 logging与clone
registration contract，而ordinary catalog仍为空；Stage 5 seam没有接入。repository format、interpreter/module regression、
双架构KUnit-on kernel build、WIT/SDK fixture、文档与whitespace检查构成本interlude的validation；具体命令和proof limits见
transaction。Architecture Friction Scan确认没有第二份lifecycle/schema truth、provider owner穿透、private runtime表示泄漏、
无真实义务的抽象层、无退出条件临时桥或Stage 5特判。

本interlude据此**Closed**，Contract Cutover保持None；Stage 5--6仍未解析或授权。真实task clone placement、production
descriptor、canonical module经真实point的vertical slice、temporary fixture删除、management/ABI、双架构guest acceptance与
hardware仍Not Run / Not Proven。

## Stage 5 — Clone observer vertical slice

**Purpose：** 由 task owner 在 clone/clone3 共用成功路径接入唯一 R0 typed point，并让 canonical Wasm observer 通过
Stage 2--4 的共同路径完成 registration、callback、logging、trap isolation、poison、try-unload 与 reload 集成闭环，形成
尚未 cut over 的 R0 candidate。

**Prerequisites：** Stage 4 关闭并证明完整 invocation/lifecycle protocol；canonical artifact 与 kernel logging handoff 可用；
task clone live seam 仍满足 child publish/enqueue 后、vfork wait/creator return 前且 owner-private guard 已释放；维护者另行
授权 Stage 5。

**Protected Boundary：** observer 只接收 creator/child TID values，无 task handle 和决策返回；callback absence、normal
return、trap 及日志过滤/截断/覆盖均不能改变已提交 clone result。不得分别 hook syscall wrapper、移动 point 位置、携带
owner-private guard 进入 interpreter，或为 Stage 5 建立绕过共同 admission/lifecycle 的 embedded fast path；本 Stage 不发布
半套 management ABI 或 current contract。

## Stage 6 — Management activation 与 R0 cutover

**Purpose：** 激活以 current effective `CAP_SYS_MODULE` 授权的 embedded/supplied load 和按 instance identity
try-unload，以当前第一方 interpreter/WIT/artifact source 与完整 Git/validation evidence 完成同一 Wasm artifact 在 RV64/LA64 上的完整
acceptance、最终 source/owner/ABI audit 与 Architecture Friction Scan，并在全部证据闭合后原子执行
`NEMOPHILA-R0-CUTOVER`。

**Prerequisites：** Stage 1--5 全部关闭；management ABI、两种 ingress 的 common-path handoff、canonical artifact、runtime
lifecycle、clone observer 与双架构验证环境均可解析为最终 acceptance；不存在未关闭的 Keter/Apollyon；维护者另行授权
Stage 6。

**Protected Boundary：** 两种 ingress 必须经过同一 kernel admission/runtime lifecycle，instance identity 不是 bearer
authority，缺少 capability 的请求在任何 mutation 前失败。只有父 RFC 的全部 mandatory evidence 闭合后才能发布 ABI、
提取最小 current contracts 并关闭 RFC；单架构、单 ingress、smoke、host check 或部分 lifecycle 证据不能换取 cutover，
未运行项必须诚实记录为 Not Run / Not Cut Over。
