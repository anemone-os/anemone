# Nemophila R0 实施路线

**状态：** Draft
**最后更新：** 2026-08-14
**父 RFC：** [RFC-20260814-nemophila](./index.md)
**当前修订：** R1
**Stage 状态：** Stage 1 / Resolved / Not Started；Stage 2--6 / Outline Only / Not Started
**Execution Authorization：** None；本轮只解析 Stage 1，不授权执行
**Contract Cutover：** None

本页只组织父 RFC 当前 Accepted R1 所定义的 Nemophila R0 Target 的实施顺序、依赖和受保护边界，不重新定义 target、
owner、ABI、Contract Impact 或 acceptance。当前 Stage 1 已解析但尚未开始，Stage 2--6 仍只有 outline；没有 Stage 获得
执行授权，也没有 production code、current contract 或 cutover。

Stage 1 的 Deliverable、Validation、Cutover 和 Stop / Exit 已在本页原位解析；维护者仍需另行授权才能开始执行。
Stage 2--6 只保留 Purpose、Prerequisites 和 Protected Boundary，只有维护者明确授权解析对应 Stage 后才补充其可执行
边界，不提前冻结类型、算法、文件列表、精确命令或 checkpoint。关闭一个 Stage 不自动授权下一个 Stage 的解析或执行。

## 全局 Implementation Boundary

- **Target / non-goals：** 交付父 RFC 定义的 in-tree 第一方 Core Wasm interpreter crate、显式 R0 profile、WIT/SDK/artifact
  toolchain、kernel runtime、`weave`、值型日志 service、clone observer vertical slice 与管理面。trusted/good-faith
  module 边界、无 fuel/preemption/force-unload、无 guest-controlled concurrency/shared execution state、无 shared
  compiled artifact、无第二个 point 或其它 Host service、无 module-side cleanup 等 non-goals 保持不变。
- **Owner / handoff / failure / cleanup：** `anemone-kernel/crates/nemophila-wasm` crate 只拥有导入后 interpreter source、
  通用 Core Wasm profile、parse、validation、translation、execution 与 trap classification；Nemophila API owner 拥有 WIT
  logical interface；task credentials 拥有
  effective `CAP_SYS_MODULE` truth；Nemophila runtime 唯一拥有 admission、instance、registration/reservation、execution
  serialization、in-flight、poison 与 retirement；具体 subsystem 只拥有 point semantics/call site/binding policy，kernel
  logging owner 保留日志 truth。跨 owner 只使用父 RFC 定义的窄 typed handoff，failure 与 cleanup 继续服从 load rollback、
  callback poison quarantine 和无副作用 busy try-unload。
- **Source / revision handoff：** 固定 Wasmi `v1.1.0` 只固定可审计的上游源码 provenance，不冻结导入到
  `anemone-kernel/crates/nemophila-wasm` 后的 crate source、内部 API 或最终 R0 profile revision。该 crate 是贯穿
  Stage 1--6 持续演进的第一方 source owner；每个 Stage 的 evidence 和 downstream consumer 都必须记录包含精确 crate
  source 的 Anemone commit 与 profile identity，不消费 branch、floating tag 或环境中的 `HEAD`。后续 Stage 可以在同一
  interpreter owner 和 R0 profile envelope 内直接修改 crate；profile feature/limit/validation 语义变化必须形成新的 profile
  revision，任何 crate source 修改都必须重跑其影响到的既有 interpreter proof 与当前 Stage consumer proof。Stage closure
  关闭当时的 deliverable 和 evidence，不把 crate source 永久冻结。
- **Protected ABI / contract / acceptance：** 保持 start-free artifact、唯一 module-side `load` entry、WIT 单一接口来源、
  load-scoped hierarchical SDK、per-instance interpreter ownership/serial execution、module-decided registration failure、
  runtime-owned reservation/cohort/poison lifecycle、clone observer 的 TID snapshot 与非决策语义，以及 embedded/supplied
  common path 和同一 artifact 的 RV64/LA64 acceptance。`NEMOPHILA-R0-CUTOVER` 前没有 Nemophila effective contract；
  Stage outline 和 RFC Accepted 状态都不发布部分 ABI 或 current semantics。
- **Validation claim：** interpreter crate 只证明显式 R0 profile 内的通用 Core Wasm correctness；Nemophila owner 分别
  证明 artifact/WIT/authorization admission、transactional lifecycle、weave registration/dispatch、trap containment、
  logging handoff 与 clone semantics；最终 Stage 以同一 artifact 的双架构 vertical slice 闭合父 RFC acceptance。R0 不
  外推 execution progress、unload bounded completion、恶意 module DoS containment 或日志持久性。
- **Stop conditions：** 需要改变父 RFC 的 target/non-goals、interpreter/runtime/provider owner、profile envelope、WIT/SDK
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
| Stage 1 | Resolved / Not Started | 从固定 Wasmi provenance 形成可持续演进的 in-tree `nemophila-wasm` crate | None |
| Stage 2 | Outline Only | 建立 WIT、SDK、canonical artifact，并与 interpreter profile 共同收敛 | None |
| Stage 3 | Outline Only | 以 pinned interpreter revision 建立 kernel transactional runtime core | None |
| Stage 4 | Outline Only | 闭合 weave、并发调用与完整 instance lifecycle | None |
| Stage 5 | Outline Only | 接入真实 clone observer vertical slice | None |
| Stage 6 | Outline Only | 激活 management、完成双架构 acceptance 并原子 cut over | `NEMOPHILA-R0-CUTOVER`（Future） |

## Stage 1 — `nemophila-wasm` 裁剪与适配

**Resolution：** Resolved / Not Started

**Execution Authorization：** None

**Purpose：** 先 clone 固定 Wasmi `v1.1.0` 源码，再将实际 interpreter source 导入 Anemone 仓库的
`anemone-kernel/crates/nemophila-wasm`，形成由第一方直接裁剪、适配、维护且会在后续 Stage 继续演进的 in-tree crate。
该 crate 不是 submodule、nested Git repository、外部独立 project、`anemone-kernel/crates/anemos` 下的 crate，也不是依赖
另一个 upstream `wasmi` crate 的 wrapper/adapter。Stage 1 建立单一、显式、版本化且 fail-closed 的 initial R0 profile
revision，提供可由后续 consumer 使用的 Core Wasm parse/validation/eager translation/execution/trap substrate，并关闭
属于该 crate owner 的源码导入、基础裁剪、窄 handoff 与 kernel embedding 可行性。它不冻结最终 R0 crate/profile
revision，也不提前证明 Stage 2 的 canonical artifact 或 Stage 3 的真实 kernel runtime integration。

**Prerequisites：** 父 RFC R1 保持 Accepted；当前没有 Nemophila code、current contract 或 register baseline；固定 Wasmi
`v1.1.0` 上游基线及 provenance 可复现。进入本 Stage 时读取 live kernel/build owner 与真实 embedding/toolchain 约束，
并以 commit `8273dfb09d493971b7bb12fe614d740cdc857175` 作为唯一导入起点；维护者另行授权 Stage 1 执行。

**Protected Boundary：** `nemophila-wasm` 只拥有通用 Core Wasm correctness 和每次调用所需的 interpreter substrate；
不得吸收 WIT policy、Host capability、provider catalog、kernel synchronization、instance lifecycle 或 management authority。
裁剪服从真实 embedding、依赖和审计需求，不为追求表面体积而重写无关语义，也不承诺通用外部 compatibility surface。
若出现无法由 source/build 直接回答的具体高风险假设，只在本 Stage 内解析具有明确 hypothesis、failure signal、write-back
和退出条件的最小 probe；probe 失败时停止，不用兼容桥、第二套 validator 或较弱 oracle 绕过。

### Implementation Boundary

- **Target / non-goals：** 从固定 Wasmi clone 导入 interpreter source、license/notices 与可复现 provenance，在
  `anemone-kernel/crates/nemophila-wasm` 形成以 `no_std + alloc` 为 production substrate 的第一方 interpreter crate、一个
  真实可执行的 initial R0 profile revision、窄 consumer handoff 和双架构 kernel-embedding build proof。Stage 1 不建立
  WIT、module SDK、canonical clone
  observer、Nemophila admission/runtime、Host import、provider、management ABI、kernel current contract 或通用外部 Wasmi
  compatibility；不保留 upstream Git metadata，不把导入源码放入 `anemos` 或外部 repository，也不保留 upstream `wasmi`
  作为 production Cargo dependency 再由 `nemophila-wasm` 包装。
- **Crate / source shape：** production dependency graph 只保留通用 interpreter 所需能力，不把 upstream CLI、WASI、C API、WAT
  parser、fuzz/spec runner 或 host-only tooling 带入 kernel consumer；这些组件中的 oracle、fixture 或回归语料可以作为
  crate-local validation input 保留。裁剪以 owner、依赖、审计和 embedding 边界为依据，不要求为了行数或制品体积重写
  已有正确语义。
- **Profile owner / handoff：** `nemophila-wasm` crate 内必须存在一个 crate-owned、显式版本化的 R0 profile definition，
  完整决定该 revision 的 Core Wasm feature、结构 limit、eager translation 和通用 validation 行为。production consumer
  只能经该 profile 取得已验证/翻译的 interpreter entity，不能从 upstream 宽松默认 `Config` 推导 profile，也不能逐 flag 覆盖、复制 matrix 或
  绕过 validation。profile 必须提供后续 Nemophila admission 所需的 checked module metadata，但不能自行决定 WIT identity、
  Host imports、Core Wasm start-section policy、provider availability 或 lifecycle。profile 结构 limit 是跨 module build/load
  一致的版本化 compatibility 事实；kernel runtime 的运行期容量与背压属于后续 Stage 的 Kconfig/owner policy，不能反向放宽
  profile。
- **Runtime boundary：** interpreter API 只表达 Wasm values、checked module/interpreter entity、由 consumer 在其自有 call
  window 内调用的窄 Host adapter，以及稳定 trap classification；interpreter 不拥有 call window，也不得持有 `Task`、`File`、
  kernel lock/raw pointer、provider、registration、in-flight、poison 或 retirement state。kernel allocator 下自然、适度的
  有界分配可以由 embedding consumer 提供，Stage 1 不为消除分配引入 pool、镜像状态或新的 resource owner。
- **Safety boundary：** supplied bytes 即使来自 R0 trusted/good-faith caller 也必须作为未验证输入进入 checked parse/
  validation path；production surface 不暴露 unchecked module construction。retained production unsafe code/unsafe impl 必须有
  owner-local invariant audit，release build 保留 Wasmi `extra-checks` 或等价的 executor invariant checks，不能把 translation
  bug 转化为 unchecked UB。该边界不扩大为恶意 module progress/DoS containment claim。
- **Revision rule：** Stage 1 交付一个供 Stage 2 起步、包含精确 `nemophila-wasm` source 的 Anemone commit 与 initial profile
  revision，不承诺它们是 R0 最终 revision。后续 Stage 可以在上述 owner/envelope 内直接修改 `nemophila-wasm`；crate
  source 改变时 downstream recorded revision 必须同步，profile definition 的 feature/limit/validation 语义改变时必须发布新的
  profile identity，并重跑受影响 proof。

### Deliverables

- Anemone 仓库内 `anemone-kernel/crates/nemophila-wasm` 第一方 crate，直接包含从固定 Wasmi provenance 导入并开始裁剪/
  适配的 interpreter source、许可材料和 crate identity；无 submodule/nested Git metadata、`anemos` 归属或 production
  upstream `wasmi` dependency；
- production `no_std + alloc` interpreter substrate 与窄 consumer surface，覆盖通用 binary parse、profile validation、
  eager translation、execution、checked module metadata 和稳定 trap classification；
- 单一 initial R0 profile definition 及其版本 identity、正反例和结构 limit tests；每个禁用 feature/越界输入都 fail closed，
  consumer 无法退回 upstream default feature set；
- crate-local host test/oracle 与 validation-only `no_std` embedding consumer；后者是真实调用 production profile/handoff 的
  consumer，但不进入 production dependency，也不成为未来 kernel runtime 的临时 facade；
- Stage 2 handoff 记录，精确 pin 包含 Stage 1 crate source 的 Anemone commit、profile identity、production feature selection、
  依赖审计结果及验证 evidence，不把本地 checkout path、branch 或 `HEAD` 写成 authority。

### Validation

- **Provenance / hygiene：** 从干净 Anemone checkout 证明 upstream tag/commit、源码导入关系、license/notices、crate identity 与
  lock/dependency inputs 可追溯；源码树和 dependency audit 证明没有 submodule/nested repository、production upstream
  `wasmi` dependency、无来源代码或未说明的第二份 interpreter implementation。
- **Profile correctness：** crate owner tests 覆盖显式启用 feature 的 parse/validate/translate/execute 正例、每个禁用 feature
  与每类结构 limit 的拒绝、malformed/type/control-flow failure，以及 R0 需要保留的 trap classification；使用适用于该
  initial profile 的 WebAssembly spec/upstream regression corpus，不把“能解析”冒充“允许执行”。
- **Owner surface：** source/API/dependency audit 证明 production consumer 只能取得 crate-owned profile 和窄 interpreter
  handoff，不能构造宽松配置；production graph 不依赖 `std`、WASI、WAT、CLI/C API、host filesystem/thread/random source，
  也不出现 kernel object、同步或 lifecycle owner。
- **Safety：** source audit 枚举 retained production unsafe boundary 及其不变量，证明 untrusted bytes 不能进入 unchecked
  constructor，release configuration 启用 `extra-checks` 或已证明等价的 invariant checks；结合适用的 Miri、fuzz regression
  corpus 与 malformed module tests 覆盖 parser/translator/executor 的安全敏感路径。此证据不能外推为对全部 interpreter bug
  或恶意 module DoS 的形式化证明。
- **Embedding：** validation-only consumer 真实构造 initial profile，并覆盖至少一次 parse、validation、eager translation、
  instance execution、Host value round trip 与 trap classification；同一 pinned Anemone commit/profile 以 Anemone pinned Rust
  toolchain 对 `riscv64gc-unknown-none-elf` 和 `loongarch64-unknown-none` 完成 `no_std + alloc` build/link。该证据只证明
  crate-level embedding feasibility，不外推真实 kernel load path、QEMU runtime 或双架构 R0 acceptance。
- **Reproducibility：** execution evidence 记录所有实际命令、toolchain/target identity、Anemone commit、profile identity、
  result 与 Not Run；Stage 1 不运行 kernel KUnit、QEMU、LTP 或 Nemophila lifecycle tests。

### Cutover

None。Stage 1 不发布 production kernel code、public ABI、visible semantics、current contract 或 register baseline；initial
profile/crate source revision 只是 Stage 2 的显式输入，不是 `NEMOPHILA-R0-CUTOVER`。

### Exit / Stop

Stage 1 在上述 deliverables、profile/owner/embedding proof 和 Stage 2 pinned handoff 全部闭合后关闭。当前只使用一个 Stage
closure checkpoint；普通 import/cut/test commit 不形成额外 gate。Stage 1 closure 不冻结 `nemophila-wasm` source，后续
Stage 修改 crate 时按 revision rule 重新 pin 并重跑受影响 proof，不重新打开 Stage 1。

如果实现需要改变 Wasmi 上游源码基线、R0 profile envelope、interpreter/Nemophila owner 分工或 validation claim，必须停止并
回到 RFC review / Target Renegotiation。如果 `no_std` 双架构 embedding 只能通过吸收 kernel synchronization/lifecycle、复制
validator/profile truth、保留未受约束的 upstream default config、引入无退出条件兼容桥或降低拒绝 oracle 才能成立，也必须
停止。当前 resolution 不预设 probe；出现 source/build 无法回答的具体高风险假设时，先在本节补全 hypothesis、failure signal、
write-back、code disposition 与 exit 并取得对应授权，再执行最小 probe。

## Stage 2 — WIT、SDK 与 artifact toolchain

**Purpose：** 建立由同一 WIT source 驱动的 logical interface、Rust module SDK 与 canonical clone observer artifact build，
并让真实 toolchain output 与 `nemophila-wasm` profile 在同一 owner/envelope 内共同收敛，形成可供后续 kernel wiring 真实
消费的 start-free 跨架构制品路线。

**Prerequisites：** Stage 1 关闭并交付可复现的 in-tree crate、initial profile revision 与 pinned handoff；进入本 Stage 时读取真实
module toolchain 约束，并在 R0 envelope 内解析 WIT lowering、artifact identity/version、canonical build、interpreter/profile
共同修订与验证边界；维护者另行授权 Stage 2。

**Protected Boundary：** WIT 只拥有逻辑接口而不驱动 runtime policy；高层 SDK 保持 load-scoped、按 capability/provider/
point 分层，不暴露 raw import、generic point tag 或 callback representation。canonical artifact 不引入 WASI、Core Wasm
start、额外 Host service、第二个 point 或架构专用 module build；真实 artifact 需要的 interpreter/profile 调整仍由
`nemophila-wasm` owner 完成并形成新的 pinned Anemone commit/profile identity，不把 WIT/toolchain 需求复制成第二份 feature matrix。
本 Stage 不宣称 kernel runtime 或 management ABI 已生效。

## Stage 3 — Kernel transactional runtime core

**Purpose：** 在 kernel 内接入 Stage 2 共同验证并精确 pin 的 `nemophila-wasm` 与 WIT consumer，建立共同 artifact
admission、per-instance interpreter ownership、恰好一次 module-side `load` entry、值型日志 call window，以及
unpublished rollback / atomic live publication 的 runtime core。

**Prerequisites：** Stage 1 和 Stage 2 关闭，interpreter revision、profile、WIT interface 与 canonical artifact 可被同一
kernel integration 消费；进入本 Stage 时从父 RFC management envelope 与 live task/ABI owner 解析 management-to-runtime
内部 handoff 和 proof route，public management ABI 仍留待 Stage 6 激活；维护者另行授权 Stage 3。

**Protected Boundary：** interpreter validation 与 Nemophila admission 不能互相替代或形成第二份 feature truth；每次 load
独占完整 interpreter entity，不共享 Engine/code/cache。module load error/trap 只能完整 rollback，日志诊断不能伪装 live
publication。真实 kernel embedding 暴露的 interpreter 修改仍回到 `nemophila-wasm` owner，并在本 Stage 关闭前重新 pin
revision、重跑受影响 interpreter/kernel proof；不得把 kernel object、同步或 lifecycle state 下沉进 interpreter。本 Stage
不公开 management ABI、不接入真实 subsystem point，也不提前建立 current contract。

## Stage 4 — Weave、并发调用与完整 lifecycle

**Purpose：** 在 Stage 3 transactional core 上闭合 typed point/provider handoff、registration/reservation、fanout cohort、
per-instance serial execution、in-flight ownership、无副作用 busy try-unload、callback trap poison/cancellation 与 live/
poisoned retirement，使 runtime correctness 在接入 task owner 前独立成立。

**Prerequisites：** Stage 3 关闭且 load/rollback/publication owner 已稳定；Stage 2 的 point-specific SDK contract 可由 kernel
consumer 真实接线；维护者另行授权 Stage 4。

**Protected Boundary：** provider 不持 instance、binding collection、in-flight 或 lifecycle truth；runtime 不发明 point
cardinality；poison 不是自动 unbind/unload，busy failure 不做部分 cleanup，同一 instance 不并发解释、不同 instances 不被
全局串行。不得为验证建立第二个 production point、无真实 consumer 的 public facade 或 KUnit-aware production protocol。

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
try-unload，pin 最终 candidate interpreter/profile/WIT/artifact revision set，完成同一 Wasm artifact 在 RV64/LA64 上的完整
acceptance、最终 source/owner/ABI audit 与 Architecture Friction Scan，并在全部证据闭合后原子执行
`NEMOPHILA-R0-CUTOVER`。

**Prerequisites：** Stage 1--5 全部关闭；management ABI、两种 ingress 的 common-path handoff、canonical artifact、runtime
lifecycle、clone observer 与双架构验证环境均可解析为最终 acceptance；不存在未关闭的 Keter/Apollyon；维护者另行授权
Stage 6。

**Protected Boundary：** 两种 ingress 必须经过同一 kernel admission/runtime lifecycle，instance identity 不是 bearer
authority，缺少 capability 的请求在任何 mutation 前失败。只有父 RFC 的全部 mandatory evidence 闭合后才能发布 ABI、
提取最小 current contracts 并关闭 RFC；单架构、单 ingress、smoke、host check 或部分 lifecycle 证据不能换取 cutover，
未运行项必须诚实记录为 Not Run / Not Cut Over。
