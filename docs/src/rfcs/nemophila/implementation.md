# Nemophila R0 实施路线

**状态：** Draft
**最后更新：** 2026-08-14
**父 RFC：** [RFC-20260814-nemophila](./index.md)
**当前修订：** R0
**Stage 状态：** Stage 1--6 / Outline Only / Not Started
**Execution Authorization：** None
**Contract Cutover：** None

本页只组织父 RFC R0 Accepted Target 的实施顺序、依赖和受保护边界，不重新定义 target、owner、ABI、Contract
Impact 或 acceptance。当前只接受 Stage 1--6 的 outline；没有 Stage 已解析、激活或获得执行授权，也没有 production
code、current contract 或 cutover。

未来只在维护者明确授权进入某个 Stage 后，才为该 Stage 原位补充 Deliverable、Validation、Cutover 和 Stop / Exit，
并解析必要的内部选择。尚未到达的 Stage 继续只保留 Purpose、Prerequisites 和 Protected Boundary，不提前冻结类型、
算法、文件列表、精确命令或 checkpoint。关闭一个 Stage 不自动授权下一个 Stage。

## 全局 Implementation Boundary

- **Target / non-goals：** 交付父 RFC 定义的第一方 Core Wasm interpreter project、显式 R0 profile、WIT/SDK/artifact
  toolchain、kernel runtime、`weave`、值型日志 service、clone observer vertical slice 与管理面。trusted/good-faith
  module 边界、无 fuel/preemption/force-unload、无 guest-controlled concurrency/shared execution state、无 shared
  compiled artifact、无第二个 point 或其它 Host service、无 module-side cleanup 等 non-goals 保持不变。
- **Owner / handoff / failure / cleanup：** `nemophila-wasm` project 只拥有通用 Core Wasm profile、parse、validation、
  translation、execution 与 trap classification；Nemophila API owner 拥有 WIT logical interface；task credentials 拥有
  effective `CAP_SYS_MODULE` truth；Nemophila runtime 唯一拥有 admission、instance、registration/reservation、execution
  serialization、in-flight、poison 与 retirement；具体 subsystem 只拥有 point semantics/call site/binding policy，kernel
  logging owner 保留日志 truth。跨 owner 只使用父 RFC 定义的窄 typed handoff，failure 与 cleanup 继续服从 load rollback、
  callback poison quarantine 和无副作用 busy try-unload。
- **Protected ABI / contract / acceptance：** 保持 start-free artifact、唯一 module-side `load` entry、WIT 单一接口来源、
  load-scoped hierarchical SDK、per-instance interpreter ownership/serial execution、module-decided registration failure、
  runtime-owned reservation/cohort/poison lifecycle、clone observer 的 TID snapshot 与非决策语义，以及 embedded/supplied
  common path 和同一 artifact 的 RV64/LA64 acceptance。`NEMOPHILA-R0-CUTOVER` 前没有 Nemophila effective contract；
  Stage outline 和 RFC Accepted 状态都不发布部分 ABI 或 current semantics。
- **Validation claim：** interpreter project 只证明显式 R0 profile 内的通用 Core Wasm correctness；Nemophila owner 分别
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
| Stage 1 | Outline Only | 从固定 Wasmi 基线形成独立 `nemophila-wasm` project | None |
| Stage 2 | Outline Only | 建立 WIT、SDK 与 canonical artifact toolchain | None |
| Stage 3 | Outline Only | 建立 kernel transactional runtime core | None |
| Stage 4 | Outline Only | 闭合 weave、并发调用与完整 instance lifecycle | None |
| Stage 5 | Outline Only | 接入真实 clone observer vertical slice | None |
| Stage 6 | Outline Only | 激活 management、完成双架构 acceptance 并原子 cut over | `NEMOPHILA-R0-CUTOVER`（Future） |

## Stage 1 — `nemophila-wasm` 裁剪与适配

**Purpose：** 从固定 Wasmi `v1.1.0` 源码基线形成位于 Anemone kernel tree 之外、独立版本化、第一方维护的
`nemophila-wasm` project，使其以单一显式 R0 profile 提供可由后续 kernel runtime 消费的 Core Wasm
parse/validation/translation/execution/trap substrate，并关闭属于该 project owner 的裁剪、适配与 kernel embedding
可行性。

**Prerequisites：** 父 RFC R0 保持 Accepted；当前没有 Nemophila code、current contract 或 register baseline；固定 Wasmi
`v1.1.0` 上游基线及 provenance 可复现。进入本 Stage 时读取 live kernel/build owner 与真实 embedding/toolchain 约束，
并在 R0 envelope 内解析精确 profile、project handoff 与验证边界；维护者另行授权 Stage 1。

**Protected Boundary：** `nemophila-wasm` 只拥有通用 Core Wasm correctness 和每次调用所需的 interpreter substrate；
不得吸收 WIT policy、Host capability、provider catalog、kernel synchronization、instance lifecycle 或 management authority。
裁剪服从真实 embedding、依赖和审计需求，不为追求表面体积而重写无关语义，也不承诺通用外部 compatibility surface。
若出现无法由 source/build 直接回答的具体高风险假设，只在本 Stage 内解析具有明确 hypothesis、failure signal、write-back
和退出条件的最小 probe；probe 失败时停止，不用兼容桥、第二套 validator 或较弱 oracle 绕过。

## Stage 2 — WIT、SDK 与 artifact toolchain

**Purpose：** 建立由同一 WIT source 驱动的 logical interface、Rust module SDK 与 canonical clone observer artifact build，
形成可由 `nemophila-wasm` profile 接受、并可供后续 kernel wiring 真实消费的 start-free 跨架构制品路线。

**Prerequisites：** Stage 1 关闭并交付固定的 `nemophila-wasm` revision/profile；进入本 Stage 时读取真实 module toolchain
约束，并在 R0 envelope 内解析 WIT lowering、artifact identity/version、canonical build 与验证边界；维护者另行授权
Stage 2。

**Protected Boundary：** WIT 只拥有逻辑接口而不驱动 runtime policy；高层 SDK 保持 load-scoped、按 capability/provider/
point 分层，不暴露 raw import、generic point tag 或 callback representation。canonical artifact 不引入 WASI、Core Wasm
start、额外 Host service、第二个 point 或架构专用 module build；本 Stage 不宣称 kernel runtime 或 management ABI 已生效。

## Stage 3 — Kernel transactional runtime core

**Purpose：** 在 kernel 内接入固定 `nemophila-wasm` 与 WIT consumer，建立共同 artifact admission、per-instance
interpreter ownership、恰好一次 module-side `load` entry、值型日志 call window，以及 unpublished rollback / atomic live
publication 的 runtime core。

**Prerequisites：** Stage 1 和 Stage 2 关闭，interpreter revision、profile、WIT interface 与 canonical artifact 可被同一
kernel integration 消费；进入本 Stage 时从父 RFC management envelope 与 live task/ABI owner 解析 management-to-runtime
内部 handoff 和 proof route，public management ABI 仍留待 Stage 6 激活；维护者另行授权 Stage 3。

**Protected Boundary：** interpreter validation 与 Nemophila admission 不能互相替代或形成第二份 feature truth；每次 load
独占完整 interpreter entity，不共享 Engine/code/cache。module load error/trap 只能完整 rollback，日志诊断不能伪装 live
publication。本 Stage 不公开 management ABI、不接入真实 subsystem point，也不提前建立 current contract。

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
try-unload，完成同一 Wasm artifact 在 RV64/LA64 上的完整 acceptance、最终 source/owner/ABI audit 与 Architecture
Friction Scan，并在全部证据闭合后原子执行 `NEMOPHILA-R0-CUTOVER`。

**Prerequisites：** Stage 1--5 全部关闭；management ABI、两种 ingress 的 common-path handoff、canonical artifact、runtime
lifecycle、clone observer 与双架构验证环境均可解析为最终 acceptance；不存在未关闭的 Keter/Apollyon；维护者另行授权
Stage 6。

**Protected Boundary：** 两种 ingress 必须经过同一 kernel admission/runtime lifecycle，instance identity 不是 bearer
authority，缺少 capability 的请求在任何 mutation 前失败。只有父 RFC 的全部 mandatory evidence 闭合后才能发布 ABI、
提取最小 current contracts 并关闭 RFC；单架构、单 ingress、smoke、host check 或部分 lifecycle 证据不能换取 cutover，
未运行项必须诚实记录为 Not Run / Not Cut Over。
