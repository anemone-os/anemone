# 2026-07-20 - Unix Job Control

**Status:** Active
**Owners:** doruche, Codex
**Area:** signal / task / process group / user entry / wait ABI / procfs
**Canonical Plan:** [RFC-20260720-unix-jobctl](../../rfcs/unix-jobctl/index.md), [目标与不变量](../../rfcs/unix-jobctl/invariants.md), [迁移实施计划](../../rfcs/unix-jobctl/implementation.md)
**Canonical Revision:** R0
**Current Phase:** Stage 1 closed; Stage 2 Not Started

## Scope

本事务按授权执行 RFC R0 的 Stage 0 与 Stage 1 checkpoint。Stage 0 只把现有 `task::sig`
根模块按既有职责做行为保持型目录化拆分；Stage 1 建立 ThreadGroup-owned dormant
job-control state、membership exposure与统一 user-entry gate，但没有 production stop /
continue ingress。child report、wait ABI、procfs projection 与 control-signal semantics仍未实现。
`UJ-CUTOVER` 为 None；全部 current contract 保持 effective，pending successor 仅作导航。

## Contract and register boundary

受影响 current contract IDs 的 pending successor 已加入对应 contract 页，但没有修改
effective rule：`SIGNAL-PENDING-*`、`SIGNAL-ACTION-*`、`SIGNAL-TEMP-MASK-*`、
`PROCFS-TASK-STATE-001`、`PGRP-SIGNAL-*`、`TASK-LIFE-*`、`CHILD-WAIT-*`、
`USER-ENTRY-001`。本阶段不更新 register / current limitations；现有
`ANE-20260527-PROCESS-GROUP-SESSION-STAGE1` 仍记录 job-control 缺口。

## Stage 0 preflight and resolved write-set manifest

R0 acceptance、transaction 创建、RFC/SUMMARY 导航和 pending-successor 导航均已完成。
Stage 0 的逐文件 manifest 冻结如下：

- `anemone-kernel/src/task/sig/mod.rs`
- `anemone-kernel/src/task/sig/mask.rs`（新建）
- `anemone-kernel/src/task/sig/pending.rs`（新建）
- `anemone-kernel/src/task/sig/generation.rs`（新建）
- `anemone-kernel/src/task/sig/delivery.rs`（新建）
- `anemone-kernel/src/task/sig/disposition.rs`（仅在 sibling visibility 必要时收窄）
- `anemone-kernel/src/task/mod.rs`（仅 import/re-export 路径调整）
- 本事务日志及 RFC/contract 导航的执行事实更新

其它 kernel、architecture、scheduler、wait、topology、apps、tests、rootfs、LTP profile
和 effective contract 正文为只读。任何需要超出此 manifest、扩大 public API、改变 owner、
lock order、ABI、visible semantics 或 target invariant 的情况，均按 implementation plan
停止条件停止，不在本事务内绕行。

## Inventory evidence

Stage 0 前的 source inventory 记录了当前 `sig/mod.rs` 约 1380 行的职责边界：

- `TaskSigMaskState`、temporary-mask token 与 mask mutation 位于既有 mask owner；
- `PendingSignals` 及 reserved-delivery queue primitive 位于 pending leaf；
- `Task::recv_signal`、`ThreadGroup::recv_signal`、pending flush 和 notification admission
  位于 generation；
- private/shared source selection、temporary-mask classifier、handler frame、no-frame
  cleanup、`handle_signals` 与 ordinary action loop 位于 delivery；
- `SigNo` 与 `Signal` 保留在 module root，既有 `disposition`、`info`、`set`、`altstack`、
  `hal`、`api` 子模块不移动。

外部调用面、direct field access 和锁序已核对：现有 `Task` / `ThreadGroup` inherent method、
`handle_signals`、temporary-mask types、pending snapshots、signal constructors 与 syscall/
architecture callers 保持 root symbol 形状；现有 `sig_pending -> sig_mask -> disposition`
顺序、shared pending 的 topology guard 和 notification guards-out 关系只做机械搬迁。

## Stage 0 execution log

### 2026-07-20 - Split-only implementation

**Change:** 将 mask、pending、generation、delivery 的既有实现移动到 manifest 指定模块；
`mod.rs` 保留 module docs、声明、窄 re-export、`SigNo` 与 `Signal`。跨 sibling 使用的
helper 只收窄为 `pub(super)`，没有新增 public API。没有创建 `task/jobctl`，没有修改
architecture、wait、scheduler、topology 或 syscall 行为。

**Review focus:** 逐项核对 root export、Task/ThreadGroup inherent method、pending fetch
顺序、temporary-mask restore responsibility、reserved-delivery finality、handler-frame
commit/no-frame cleanup、notification 与锁序。若发现 ownership、reservation、temporary
restore、generic carrier 或 visibility 需要语义扩张，Stage 立即停止。

**Validation:**

- `just fmt kernel --check` 通过。
- `git diff --check` 通过；四个新文件分别通过 `git diff --no-index --check` 空白检查。
- 首次在 sandbox 中运行 `just build` 时，`lwext4` 编译子进程因 sandbox `Bad system call` 失败；随后以仓库入口、经批准的 escalation 重跑同一 `just build`，构建通过。该环境失败不被记录为代码失败。
- source audit 通过：79 个函数体 hash 未发现删除或改变；root 不再包含 `impl Task`、`impl ThreadGroup`、`handle_signals` 或 `perform_signal_action`；调用者闭包、sibling visibility、direct-field 使用及既有锁序保持不变。
- 按 Stage 0 规定未运行 QEMU/LTP，也未运行 LA64；本阶段不产生 runtime 证据。

**Review:** 未另行启动独立 reviewer；完成冻结 review focus 的 self-review，未发现
Apollyon、Keter 或 Euclid 级别的 owner、生命周期、锁序、ABI 或可观测语义问题。每一行
代码改动均限于职责移动、module declaration、import 或 sibling visibility 收窄。

**Result:** Stage 0 split-only checkpoint closed。行为保持型模块拆分完成；没有新增
jobctl state、runtime ingress、scheduler/wait/topology 变化或 public API，`UJ-CUTOVER`
仍为 None，Stage 1 Not Started。

## Stop conditions and feedback

本 checkpoint 未命中 Stage 0 停止条件。没有 tracking issue 文件；当前没有已确认的
target blocker。若后续阶段发现无法保持 pending ownership、reservation semantics、
temporary-mask restore protocol、lock order 或现有 visibility，则停止并回写 implementation
plan / RFC review，不通过兼容桥或额外 manager/carrier 继续。

## Handoff

Stage 0 关闭后，下一 gate 只能是 Stage 1 manifest 冻结与 dormant ThreadGroup/user-entry
foundation preflight。Stage 1 不在本事务当前变更中执行，也不因本 checkpoint 自动开始。

## Correction note - 2026-07-21

收口复核发现总 RFC 索引、双周 devlog 和 invariants 页仍保留 Stage 0 启动期措辞；本条
补充后已同步为 Stage 0 closed、Stage 1 Not Started。该修正只澄清文档状态，不改变 R0
target、current contract、register、write set 或 validation floor。

## Stage 1 preflight - 2026-07-21

**Authority and transaction correction:** 用户明确授权完成 Stage 1，并反馈现有
`task/api/jobctl` 可以迁移到 `task/jobctl/api`。本 transaction 对应仍在实现中的同一 R0，
页首 `Closed` 只正确描述 Stage 0 checkpoint、错误地关闭了整条 R0 transaction；现已更正
为 `Active`，同时保留 Stage 0 closed 的全部历史事实。该状态修正不重开 Stage 0、不改变
R0 target，也不创建并列 transaction。

**Resolved Write Set Manifest:** Stage 1 manifest 已冻结为：

- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/task/jobctl/{mod,group,user_entry}.rs`
- `anemone-kernel/src/task/jobctl/api/{mod,getpgid,getsid,setpgid,setsid}.rs`
- `anemone-kernel/src/task/api/mod.rs` 与旧
  `anemone-kernel/src/task/api/jobctl/{mod,getpgid,getsid,setpgid,setsid}.rs`
- `anemone-kernel/src/task/topology/{mod,thread_group}.rs`
- `anemone-kernel/src/task/api/clone/mod.rs`
- `anemone-kernel/src/task/api/execve/kernel.rs`
- `anemone-kernel/src/arch/{riscv64,loongarch64}/exception/trap/utrap.rs`
- `anemone-kernel/src/arch/{riscv64,loongarch64}/sched.rs`
- RFC implementation plan 与本 transaction 的 Stage 1 执行事实更新

旧 syscall 目录到 `task/jobctl/api` 的移动只允许改变物理归属和 module declaration；syscall
registration、ABI、policy、可见语义与 public API 不得扩大。lifecycle、wait、procfs、Signal
semantic path、scheduler、apps、rootfs、LTP profile、LA64 runtime 和 effective contract 正文
保持只读。

**Entry and membership inventory:** live source 中的 ThreadGroup construction 位于
`topology/mod.rs` 的 root、user leader 与 kthread 三条路径；user member join 位于同文件的
`TaskBinding::Member`，detach 与 dethread membership key replacement 位于
`topology/thread_group.rs`。两架构 ordinary return 位于各自 `rust_utrap_entry()`，fresh task
位于各自 `sched.rs::user_task_entry_secondary()`，clone child 经
`enter_cloned_user_task() -> TrapArch::load_utrapframe()`，exec new image 经
`load_context(TaskContext::from_user_fn(...))` 落入相同 fresh path。两架构 raw
`utrap_return_to_task()` wrapper 是 fresh / clone / exec 的共同最终 user-transition facade。

**Frozen baseline:** 输入冻结为仅含 `signal` / `wait` 的现有 LTP profile、
`etc/sdcard-rv.img`、`./scripts/run-user-test-rv64.sh` 和日志
`build/unix-jobctl-stage1-baseline-rv64.log`。修改前 wrapper 已完成完整 rootfs、kernel、QEMU
链；182 项 KUnit 全部通过。glibc 与 musl 各自结果均为
`attempted=56 passed=49 failed=5 infra_failed=0 skipped=2`，合计
`attempted=112 passed=98 failed=10 infra_failed=0 skipped=4`。Stage 1 退出运行必须用相同输入
对比 exact case/result classification；这些既有 failure 不得掩盖新增回退。

**Cutover / register:** `UJ-CUTOVER` 仍为 None；current contract 与
`ANE-20260527-PROCESS-GROUP-SESSION-STAGE1` 保持不变。Stage 1 只建立 dormant readiness，
不得出现 production stop / continue ingress，也不得进入 Stage 2。

## Stage 1 execution log - 2026-07-21

**Change:** 新建 `task/jobctl/{mod,group,user_entry}.rs`。user ThreadGroup 构造 dormant
`Running` phase、continue ordering identity与 predicate-only `jobctl_unblocked` capability；
membership wrapper直接在每个 live user member value中承载 `Unexposed / Exposed`，没有
task-local flag、participant set或派生 behavioral counter。kthread construction保留
`job_control=None`，user/kthread presence由 `ThreadGroupType` construction与 shape assertion
约束。旧 `task/api/jobctl` 同 owner物理迁移到 `task/jobctl/api`，四个 syscall实现文件内容
逐字不变，registration、ABI与 policy不变。

**Entry closure:** RV64 与 LA64 的 `rust_utrap_entry()` 在继续内核处理前通过
`on_user_trap_entry()`清 exposure。两架构 ordinary return与 raw
`utrap_return_to_task()` facade均在 FPU restore前调用 `before_user_entry()`；fresh task经
`sched.rs::user_task_entry_secondary()`、clone child经 `TrapArch::load_utrapframe()`、exec new
image经 `TaskContext::from_user_fn()`进入该 facade。gate只在 ThreadGroup owner下登记
exposure；FPU restore完成后才发布 `Privilege::User`，因此 future park不会跨 context switch
保留错误 FPU ownership。gate没有外层 guard、active wait registration或线性 token；Event
只发布 predicate rescan，不携带 phase或 permit。

**Membership / owner audit:** root、user leader、kthread三条 construction，user member join、
ordinary detach、dethread rekey与 kthread unpublish均已闭合。user detach / rekey要求
`Unexposed`，trap entry与 Running gate分别断言 `Exposed -> Unexposed` 与
`Unexposed -> Exposed`；generic removal只接受 kthread variant。`Option<UserJobControl>`不参与
ThreadGroup type决策，diagnostic-only phase timestamps在字段旁明确不驱动行为。未发现
Signal ingress、scheduler stop state、wait/lifecycle/procfs semantic change或旧 syscall路径残留。

**Review:** 独立 reviewer 初检发现 task-internal visibility与 FPU/gate顺序两个 Keter，均在
runtime前修复；后续 Euclid 指出的 removal escape、幂等 exposure写入、diagnostic字段标注与
状态导航分叉也已 neutralize。最终复核未留 Apollyon、Keter或 Euclid。未为 dormant gate
强造 KUnit；真实 trap entry与 lifecycle closure由 mandatory RV64 wrapper及源码审计覆盖。

**Validation:**

- `just fmt kernel --check` 通过。
- `just build` 通过；sandbox内首次 lwext4 C build因 `Bad system call`被阻止，随后使用同一
  repository入口经批准在 sandbox外构建通过，该环境失败不记为代码失败。
- `./scripts/run-user-test-rv64.sh etc/sdcard-rv.img build/unix-jobctl-stage1-rv64.log`
  完整通过并正常关机；KUnit 182 项全部通过。
- glibc与musl各自为
  `attempted=56 passed=49 failed=5 infra_failed=0 skipped=2`，合计
  `attempted=112 passed=98 failed=10 infra_failed=0 skipped=4`。
- 以 libc、testcase名与exit code归一化比较
  `build/unix-jobctl-stage1-baseline-rv64.log` 和最终日志，diff为空；没有新增结果分类回退。
- RV64 / LA64 source closure、syscall registration uniqueness、新文件 whitespace与
  `git diff --check` 通过；按 Stage 1 floor未运行 LA64 runtime。

**Docs-only closure write set:** 为同步唯一 canonical 状态，closure增加
`docs/src/rfcs/unix-jobctl/index.md`、`docs/src/rfcs.md` 与当前双周 devlog。该扩展只写
Stage 1 closed / Stage 2 Not Started及已有验证证据，不修改 R0、invariants、current contract、
register、ABI、visible semantics或 `UJ-CUTOVER=None`；验证为 stale wording / link audit和
`mdbook build docs`。

**Result:** Stage 1 checkpoint closed。所有 production user ThreadGroup仍为 `Running`，没有
signal可触发 stop / continue。Stage 2 manifest未冻结、未获授权且未进入实现；下一步只能在
新的明确授权下执行 Stage 2 preflight。
