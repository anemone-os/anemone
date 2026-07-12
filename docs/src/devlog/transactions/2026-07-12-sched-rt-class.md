# 2026-07-12 - Sched RT Class

**状态：** Active
**负责人：** doruche, Codex
**领域：** scheduler / realtime / FIFO / RR / scheduler class
**权威计划：** [RFC-20260711-sched-rt-class](../../rfcs/sched-rt-class/index.md), [不变量需求](../../rfcs/sched-rt-class/invariants.md), [迁移实施计划](../../rfcs/sched-rt-class/implementation.md), [Tracking Issues](../../rfcs/sched-rt-class/tracking-issues.md)
**当前阶段：** Scheduler-Core 前置 Gate 已关闭；Checkpoint 1 待前置 commit 后启动

## 范围

本事务跟踪 `sched-rt-class` RFC 的 staged implementation：

- 先证明现有 `PendingResched` full-pick acknowledgement、deferred restore 和 wait no-switch 边界可直接被 RT class 消费；
- 原子引入共享 `Realtime` class、typed priority、FIFO/RR policy、99 个 priority bucket、RR quantum、`RunQueue` dispatch、default constructor 和 Kconfig selector；
- 删除 legacy `RoundRobin` identity / queue owner，不保留双 dispatch 或 identity alias；
- 通过 source proof、focused KUnit、两个 compile-time selector build 和用户态同 priority smoke 关闭第一版。

非目标仍以 RFC 为准：本事务不实现调度属性 syscall、published-task policy/priority mutation、RT bandwidth、跨核迁移、不同 priority 用户态 workload 或 hard realtime guarantee。

## 不变量

- `RtEntity` 是 effective RT priority、policy 和 RR remaining budget 的唯一真相源；full quantum 只来自受约束的生成配置。
- `PendingResched` 只作为 pre-pick value snapshot 进入 preempted-current transaction；RT class 不保存 processor slot 或 task-local pending state。
- `RunQueue` 单独拥有跨 class precedence 和 physical membership；RT bucket node 不复制 priority。
- FIFO/RR 共享一个 `Realtime` identity 和同 priority FIFO 序列；不保留 legacy `RoundRobin` fallback。
- worker 未经批准不得越过当前 checkpoint write set；必要扩张先记录理由、owner surface、合同影响和验证 gate。
- bucket `VecDeque` 的 noirq allocation 风险继承 legacy RR，按用户裁定暂时接受，并继续由 [ANE-20260622-IRQ-OFF-HEAP-ALLOCATION](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation) 跟踪；实现不得把它宣称为 allocation-free。

## Handoff

**Last Updated:** 2026-07-12

**Current Branch:** `dev/drc/rt`

**Canonical RFC:** [RFC](../../rfcs/sched-rt-class/index.md), [Invariants](../../rfcs/sched-rt-class/invariants.md), [Implementation Plan](../../rfcs/sched-rt-class/implementation.md), [Tracking Issues](../../rfcs/sched-rt-class/tracking-issues.md)

**Completed:** 公共 RFC 已提升。Scheduler-Core 前置 Gate source audit 已证明 full-pick acknowledgement、deferred restore、wait no-switch abort、parked handoff 和 removed abort-park surface。文档层 review 发现的 stage/write-set、production cutover、quantum source 和 review-gate Keter 已折回 canonical implementation；noirq allocation 风险按用户裁定链接既有 register 条目。独立 closure reviewer 对最终 canonical / transaction diff 未发现未关闭的 Apollyon、Keter 或 Euclid。

**In Progress:** 无；前置 Gate 文档待提交。

**Open Blockers:** 无 scheduler-core blocker。Checkpoint 1 仍未开始；只有前置文档复审和 commit 关闭后才可启动 implementation worker。

**Next Action:** 提交前置 Gate 后，按 [Checkpoint 1 原子 write set](../../rfcs/sched-rt-class/implementation.md) 启动 implementation worker；worker 返回后由不同 reviewer 执行独立 gate。

**Do Not Redo:** 不重新设计 wait-core / pending acknowledgement；不把 RT payload 暂时伪装成 `RoundRobin`；不硬编码临时 quantum；不保留双 queue fallback；不通过 task-local state 或隐藏 setter 制造测试入口。

## Phase Log

### 2026-07-12 - Scheduler-Core 前置 Gate 与实施协议修正

**阶段：** 前置 Gate - source audit / document review。

**前置状态：**

- checkout root 为 `/home/doruche/dev/anemone`，分支 `dev/drc/rt`；阶段开始时 `git status --short` 为空，HEAD 为 `e7db92d7e1b5acfd509a267ef5d472017fdf7c92`。
- 已读取 repository `AGENTS.md`、`LOCAL.md`、RFC workflow/template、register open issues/current limitations、当前双周 devlog、相邻 scheduler transaction，以及 `sched-rt-class` 全部 canonical 文档。
- 私有 `etc/plans/sched-rt-class` 只用于 promotion 对照；公开 `docs/src/rfcs/sched-rt-class` 是唯一 canonical source。

**Scheduler-Core source audit：**

```sh
rg -n "PendingResched|take_pending_resched|restore_pending_resched|local_pick_next" anemone-kernel/src/sched anemone-kernel/src/arch
rg -n "schedule_preempt|DeferredPreempt|AbortWaitSleep|handoff_woken_current" anemone-kernel/src/sched anemone-kernel/src/arch
rg -n --glob '*.rs' "requeue_aborted_wait_current|aborted_wait_current|abort.*park.*class|abort.*wait.*requeue" anemone-kernel/src
rg -n "WaitState|WakeToken|WaitReason|ParkState|wait_id|wait_identity" anemone-kernel/src/sched/class
```

结果：

- `Processor::pending_resched` 是唯一长期 slot；`take_pending_resched()` 按值复制后清空，`restore_pending_resched()` 用 union 恢复 destructive-take snapshot 并保留并发新增 cause。
- `local_pick_next()` 在 IRQ disabled owner-CPU path 中先成功 `pick_next_task()`，再清 processor slot，随后调用 `set_next_task()`。Idle 兜底保证正常路径没有 selection `None`。
- rv64 / la64 的 user/kernel trap 四个 destructive-take caller 都只在 `schedule_preempt()` 返回 `Deferred` 时恢复同一 snapshot；PrePark deferred path 在 `switch_out()` 和 full pick 前返回。
- wait no-switch `AbortWaitSleep` 只返回 `DidNotSwitch`，不调用 class transaction、`switch_out()` 或 full pick，也不确认 processor slot。
- parked completion 的 production current path只通过 `local_handoff_woken_current()` 收口。
- production tree 无 `requeue_aborted_wait_current()` 或等价 abort-park class transaction；class 模块无 wait identity / park state 字段。

**前置 Gate 结论：** PASS。未命中 Scheduler-Core 停止条件；无需修改 `processor.rs`、wait-core、trap/IPI pending plumbing 或对应 scheduler-core transaction。

**文档层 review findings 与修正：**

- `KETER-RT-001`：原 ckpt1 无法在不修改 exhaustive `RunQueue` dispatch 的前提下让 `Arc<Task>` 取得唯一 `RtEntity`。修正为 Checkpoint 1 原子切换 class payload、identity、dispatch 与 legacy owner。
- `KETER-RT-002`：原 ckpt2 删除 legacy owner，却把 production constructor 迁移留给 ckpt3，无法形成独立可编译 checkpoint。constructor switch 已并入 Checkpoint 1。
- `KETER-RT-003`：RR refill 所需 full quantum 原先晚于算法接入。typed selector、timeslice config 和生成链已前移到 Checkpoint 1。
- `KETER-RT-004`：RFC entry、tracker 与 implementation 对 stage/write set 状态互相矛盾，且缺少独立 review pass rule / build floor。canonical 状态、write set、review gate 和验证 floor 已同步修正。
- noirq `VecDeque` allocation 风险不是 RT 新引入的问题；用户裁定沿用既有限制。RFC 与 transaction 链接 register，Checkpoint 1 实现必须加边界与删除条件注释。

**Checkpoint 1 worker contract：**

- 允许写入 `sched/class/{entity.rs,rt.rs,mod.rs,runqueue.rs,rr.rs}`、两架构 bootstrap、clone、`kthreadd`、`conf/.defconfig`、xtask kconfig owner和 build-generated `kconfig_defs.rs`；`rr.rs` 只允许删除。ignored 根 `kconfig` 是 live build input，只允许增加/切换本 checkpoint 的 selector 与 timeslice，必须保留其它开发者本地选项且不得提交。
- 禁止修改 `processor.rs`、wait-core、trap/IPI pending plumbing、task topology、调度属性 syscall 或其它 scheduler class 算法。
- implementation worker 与独立 reviewer 必须是不同 agent；review pass 要求无未关闭 Apollyon / Keter / Euclid。

**Review Gate：** 独立 reviewer 首轮发现 identity/write-set、atomic production cutover、full quantum source、canonical status/review floor 等 Keter；修正后又逐项补齐 live `kconfig`、Checkpoint 2 gated write set、class precedence/idle fallback、fresh-only constructor 与 published-task setter audit，以及 stable issue-ID / artifact-boundary 问题。最终 closure 未发现未关闭的 Apollyon、Keter 或 Euclid。

**Validation：** 在最终 closure edits 后，`git diff --check` clean；新增 transaction 的 no-index whitespace check clean；`mdbook build docs` 通过，只有既有 large search index warning。未运行 kernel build、KUnit、QEMU 或 LTP；Checkpoint 1 kernel implementation 尚未开始。

## Open Items

- Checkpoint 1 implementation / independent review / validation 尚未执行。
- RT/RR 与 RT/FIFO 用户态 smoke 属于 Checkpoint 2，当前未运行。

## Closure

事务仍 Active。只有 Checkpoint 1 原子 cutover、后续集成验证和 user-run / unrun 状态明确后，才按 RFC 收口。
