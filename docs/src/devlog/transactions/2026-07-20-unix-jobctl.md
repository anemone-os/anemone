# 2026-07-20 - Unix Job Control

**Status:** Closed
**Owners:** doruche, Codex
**Area:** signal / task / process group / user entry / wait ABI / procfs
**Canonical Plan:** [RFC-20260720-unix-jobctl](../../rfcs/unix-jobctl/index.md), [目标与不变量](../../rfcs/unix-jobctl/invariants.md), [迁移实施计划](../../rfcs/unix-jobctl/implementation.md)
**Canonical Revision:** R0
**Current Phase:** Stage 0 - Signal module-boundary split-only checkpoint (closed)

## Scope

本事务执行 RFC R0 的 Stage 0。Stage 0 只把现有 `task::sig` 根模块按既有职责做
行为保持型目录化拆分；不实现 jobctl phase、continue ordering、opposite-class cleanup、
user-entry gate、child report、wait ABI、procfs projection 或任何新的 runtime state。
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
