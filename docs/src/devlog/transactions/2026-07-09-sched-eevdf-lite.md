# 2026-07-09 - Sched EEVDF-lite

**Status:** Active
**Owners:** doruche, Codex
**Area:** scheduler / fairness / runtime accounting / scheduler class
**Canonical Plan:** [RFC-20260622-sched-eevdf-lite](../../rfcs/sched-eevdf-lite/index.md), [不变量需求](../../rfcs/sched-eevdf-lite/invariants.md), [迁移实施计划](../../rfcs/sched-eevdf-lite/implementation.md), [Tracking Issues](../../rfcs/sched-eevdf-lite/tracking-issues.md)
**Current Phase:** Checkpoint 1A 已关闭；下一步是 Checkpoint 1B - Typed Pending、Schedule Entry、Trap / IPI Plumbing 与 `EEVDF-005`

## Scope

本事务跟踪 `sched-eevdf-lite` RFC 从阶段 0 到第一版 default normal scheduler 切换和收口的实现过程。

阶段顺序以 RFC `implementation.md` 为准：

- 阶段 0 只关闭文档协议、建立事务日志并审计当前 sched-split / scheduler class 接缝；
- 阶段 1A 先做 method-first `Scheduler` trait、`RunQueue` facade、`SchedEntity` 拆分和 RR / Idle 行为保持；
- 阶段 1B 再接 typed pending resched、schedule entry、trap / IPI plumbing 和 `EEVDF-005` switch-in source audit；
- 阶段 2A / 2B / 2C / 2D 分别关闭 EEVDF payload scaffold、runtime accounting、`rq_vtime` / arithmetic / bounded yield、wake clamp / parked handoff；
- 阶段 3 才翻转 default normal class；
- 阶段 4 收口事务、register / current limitations 和后续优化队列。

非目标：

- 不重新设计 sched-split、wait-core `TaskSchedState`、`WakeToken`、`PrePark/Parked`、stale-safe wake placement 或 trap entry ownership；
- 不把 `iozone`、LTP 长日志或用户侧 baseline 数字写成 agent 必跑 gate；
- 不在阶段 0 或 1A 引入 EEVDF 算法语义、default class switch、Kconfig scheduler constants 或 clone fresh entity 修复；
- 不为服从旧 write set 引入长期 compatibility layer、catch-all `SchedEvent` / `on_event`，或 generic `enqueue_runnable()` 默认底座。

## Invariants

- `TaskSchedState` 继续拥有 task runnable / waiting / zombie 逻辑状态。
- `SchedEntity::on_runq()` 继续只表示 owner CPU runqueue 上的物理排队事实。
- `Task::cpuid()` 继续是 task 的 owner CPU 真相源；第一版不做跨核迁移。
- `Task::nice()` / `set_nice()` 是 nice / weight 的唯一长期真相源。
- `ScheduleMode` 属于 scheduler core entry permission，不得泄漏到 scheduler class。
- `PendingResched` 只能作为 processor / scheduler-core private pending request；class 最多按值读取 preempted-current transaction 参数。
- scheduler class contract 必须是 method-first class-local atomic transaction surface。
- wake clamp 只能由 `enqueue_woken()` 与 `handoff_woken_current()` 这类 class transaction 表达；wait-core private identity 不进入 class 算法状态。
- worker 未经总控和用户批准不得越过当前阶段 write set；若更好的架构需要扩大 write set，先上报 expansion request，并把批准结果写入本事务日志。

## Handoff

**Last Updated:** 2026-07-09

**Current Branch:** `drc/eevdf`

**Completed:** 公开 RFC 目录已存在，阶段 0 所需四份 RFC 文档已读取：`index.md`、`invariants.md`、`implementation.md`、`tracking-issues.md`。register、current limitations、RFC workflow、RFC template、devlog workflow、templates、事务索引、当前双周 devlog、SUMMARY 和 RFC 列表已读取。阶段 0 建立本事务日志，并把 RFC、tracking issues、事务索引、当前双周 devlog、mdBook Summary 和 RFC 列表连接到同一实现记录。总控完成阶段 0 source audit；未启动 subagent，因为阶段 0 不需要 worker 写代码。Checkpoint 1A 已完成：`Scheduler` trait 改为 method-first transaction surface，`RunQueue` 与 `SchedEntity` 拆出同一 owner 内文件边界，RR / Idle 完成行为保持适配。

**In Progress:** 无 worker 正在运行。下一步是 Checkpoint 1B，必须限制在 1B write set 内处理 typed pending、schedule entry、trap / IPI plumbing 和 `EEVDF-005` source audit。

**Open Blockers:** 无阶段 0 / 1A 停止条件。当前 active Keter `EEVDF-001`、`EEVDF-002`、`EEVDF-004`、`EEVDF-005`、`EEVDF-017`、`EEVDF-020` 都已有后续 checkpoint 或 gate 归属；`EEVDF-005` 仍等待 1B 完整 source audit，不能在 1A 关闭。

**Next Action:** 分派或执行 Checkpoint 1B：typed pending、schedule entry、trap / IPI plumbing 与 `EEVDF-005`。不得提前触碰 EEVDF payload、default normal switch、Kconfig constants 或 clone fresh entity 修复。

## Phase Log

### 2026-07-09 - 阶段 0 文档协议关闭与 sched-split 接缝审计

**Phase:** 阶段 0 - 文档协议关闭与 sched-split 接缝审计。

**Change:** 在实现前建立本事务日志，并更新 RFC / transaction / devlog / mdBook 导航。阶段 0 没有修改内核代码。

**Preflight:**

- 分支为 `drc/eevdf`，阶段启动时 `git status --short --branch` 只显示当前分支。
- RFC `implementation.md` 当前是 implementation canonical source；阶段 1 已拆为 1A / 1B，阶段 2 已拆为 2A / 2B / 2C / 2D。
- register 相关开放项仍包括 scheduler/event wait 交错、signal wait-core 语义、LTP post-summary hang 和 IRQ/off-tail allocation audit。本事务不关闭这些条目；后续 runtime feedback 若命中这些 owner，应按 register / 对应 RFC 路由。

**Source audit:**

```sh
rg -n "schedule\(\)" anemone-kernel/src
rg -n "SchedEvent|on_event|EnqueueReason|RequeueReason|SwitchOutReason" anemone-kernel/src/sched
rg -n "ScheduleMode" anemone-kernel/src
rg -n "task_enqueue\(|local_enqueue\(|remote_enqueue\(|local_requeue_current\(|wake_enqueue\(|local_wake_enqueue\(|remote_wake_enqueue\(" anemone-kernel/src
rg -n "current_task\.sched_entity\(|sched_entity\(\)" anemone-kernel/src
rg -n "SchedClassPrv::RoundRobin\(\(\)\)|SchedClassPrv::Idle\(\(\)\)|SchedEntity::new" anemone-kernel/src
rg -n "Instant::now\(" anemone-kernel/src/sched anemone-kernel/src/time scripts/xtask/src/config conf/.defconfig kconfig
rg -n "nice\(|set_nice|setpriority|getpriority|kernel_setpriority" anemone-kernel/src/task anemone-kernel/src/sched
```

Findings:

- 裸 `schedule()` 无匹配，sched-split wrapper 已经是当前下层前提。
- `ScheduleMode` 只在 `sched/mod.rs` 内部出现；class 模块没有保存或解释 scheduler-private mode。
- scheduler implementation 中没有 `SchedEvent` / `on_event` / `EnqueueReason` / `RequeueReason` / `SwitchOutReason`。
- 当前 `Scheduler` trait 仍是旧 `enqueue()` / `dequeue()` / `pick_next()` / `on_tick()`；`RunQueue`、`SchedEntity` 和 `SchedClassPrv` 仍集中在 `sched/class/mod.rs`，`SchedEntity` 仍是 `Copy` POD。这正是 Checkpoint 1A 的 split-only + method-first surface 工作。
- `RoundRobin` 保持 `VecDeque` FIFO，tick 总是请求 resched；`Idle` 保持 fallback singleton，但 idle loop 仍通过 bool `fetch_clear_need_resched()` 进入 `schedule_idle()`。
- processor pending request 仍是 `need_resched: bool`，`mark_need_resched()` / `fetch_clear_need_resched()` 仍压扁 tick 与 runnable arrival。trap tail 和 idle tail 仍使用该 bool；typed `PendingResched` 属于 Checkpoint 1B。
- `schedule_runnable()` 与 `schedule_idle()` 目前都使用 `ScheduleMode::Runnable`；yield / idle semantic split 属于 Checkpoint 1B。
- `local_requeue_current()` 仍是 generic current runnable requeue，调用点在 `schedule_inner()` 的 runnable path 和 wait park abort path。yield / preempt / abort-park / parked handoff method split 属于 Checkpoint 1B。
- `task_enqueue()` 跨模块调用点只在 clone publish、`kthreadd` publish 和 kthread spawn；`remote_enqueue()` 的 IPI handler 在目标 CPU 通过 `local_enqueue()` 执行。当前命名仍是 generic `task_enqueue` / `local_enqueue` / `remote_enqueue`，语义化 new-task publication 命名属于 Checkpoint 1B。
- `wake_enqueue()` 只由 wait core wake completion 和 stale-safe wake IPI path 调用；`WakeEnqueueResult::{Stale, AlreadyCurrent, ParkPending, AlreadyQueued, Enqueued}` 已存在，但尚未映射到 class-local `enqueue_woken()` / `handoff_woken_current()` / no-reward transactions。
- remote new-task and stale-safe wake IPI handler 当前在目标 CPU placement 后保守 `mark_need_resched()`；它还没有 owner CPU placement 后的 class `decide_preempt_current()`，也没有 typed `ReschedCause::RunnableArrival`。这属于 Checkpoint 1B 和后续 EEVDF placement gate。
- bootstrap 和 kthread 初始化点仍显式创建 `SchedClassPrv::RoundRobin(())`；idle 初始化是唯一 `SchedClassPrv::Idle(())`。default normal switch 不得在阶段 1/2 偷跑，阶段 3 才处理这些点。
- `kernel_clone()` 当前仍复制 `current_task.sched_entity()`，然后单独继承 nice。该旧形状在 RR `Copy` 实现下存在，但会复制未来 EEVDF runtime state；修复归属 Checkpoint 2A / 阶段 3 fresh entity gate，不能在阶段 0 或 1A 顺手补。
- `Task::nice()` 是 atomic 单一 truth；`setpriority()` 只调用 `Task::set_nice()`，当前 scheduler 不消费 nice。weight visibility 属于 Checkpoint 2C。
- 当前 scheduler path 中 `Instant::now()` 用于 wait origin 诊断与 `schedule_wait_with_timeout()` 起点；`Instant::now()` 读 `monotonic_uptime()`，后者读取本地硬件 monotonic counter和 percpu boot offset，不分配、不睡眠、不拿复杂锁。未来 EEVDF accounting 仍需在 1B/2B 按实际 call site 复核 noirq/tick 使用。
- 当前 switch-in 顺序是 scheduler loop `local_pick_next()` -> `switch_mapping(prev, next)` -> `switch_to(next)`；`switch_to()` 内执行 `Task::on_switch_in()`、`set_current_task()` 和 arch switch。当前没有 class `set_next_task(task, now)` 单一落点，且 mapping 准备发生在 class hook 之前；这正是 `EEVDF-005` 的 1A/1B source-audit gate。

**Stop-condition assessment:** 未命中阶段 0 停止条件。当前旧形状都能归入 RFC 已定义的后续 checkpoint；未发现必须在 EEVDF-lite 内改变 wait-core、IPI payload ABI、task topology、trap entry 或 user-visible `sched_*` ABI 的证据。`kernel_clone()` 复制 `SchedEntity`、generic enqueue/requeue 命名、bool `need_resched`、缺少 `set_next_task()` 和 RoundRobin default 都是后续 gate 的输入事实，不是阶段 0 自行拍板修复项。

**Next worker contract:** Checkpoint 1A worker 只能触碰 1A write set：`sched/class/mod.rs`、`sched/class/runqueue.rs`、`sched/class/entity.rs`、`sched/class/rr.rs`、`sched/class/idle.rs`、以及必要的 `sched/processor.rs`、`sched/mod.rs` facade 调用同步和 `task/sched.rs` helper 签名同步。不得修改 trap/IPI pending plumbing、wait-core helper、task topology、clone fresh entity、Kconfig 或 EEVDF payload。实现完成后必须由独立 review gate 确认 RR / Idle 行为保持、class module 不引用 `ScheduleMode`、没有 event bus、拆分未扩大 owner boundary、临时 wrapper 只有明确收口点。

**Validation:**

```sh
git diff --check
mdbook build docs
```

结果：`git diff --check` clean；`mdbook build docs` 通过，HTML 输出到 `docs/book`。agent 未运行 `just build`、QEMU、LTP、iozone 或 scheduler smoke；阶段 0 是 docs / audit gate，内核代码未修改。

**Next:** 阶段 0 commit 后进入 Checkpoint 1A。

### 2026-07-09 - Checkpoint 1A Trait / RunQueue / Entity Split 与 RR / Idle 机械适配

**Phase:** 阶段 1 - Checkpoint 1A。

**Change:** 总控直接执行本 checkpoint，未启动 subagent；本轮 write set 只触碰 1A 允许的 scheduler class / processor facade 文件。`sched/class/mod.rs` 现在只承载 `Scheduler` trait、`TickAction`、`PreemptDecision`、`PendingResched` 和 `ReschedCause` 等 class contract；`sched/class/runqueue.rs` 承载 `RunQueue` facade、`ntasks`、`on_runq` 维护和 class dispatch；`sched/class/entity.rs` 承载 `SchedEntity` / `SchedClassPrv`，并保持 RR/Idle 形状与 `Copy` 行为。RR 显式实现 `enqueue_new()`、`enqueue_woken()`、`requeue_yielded_current()`、`requeue_preempted_current()`、`handoff_woken_current()`、`requeue_aborted_wait_current()`、`put_prev_*()`、`pick_next_task()`、`set_next_task()`、`task_tick()` 和 `decide_preempt_current()`，但行为仍是 FIFO `VecDeque`、tick 请求 resched、missing dequeue panic。Idle 保持 fallback singleton，对不应发生的 enqueue / requeue / dequeue / block / exit transaction fail closed。

**Compatibility bridge:** `processor::local_requeue_current()` 仍作为 1A 临时跨模块入口存在，但只转发到 `RunQueue::requeue_current_legacy()`；该 helper 带注释说明 1B 必须把调用点拆成 yield / preempt / parked handoff / abort-wait transaction。`PendingResched` 只作为值类型进入 trait 形状，processor `need_resched: bool`、trap tail、idle tail 和 IPI producer 尚未切换到 typed plumbing，符合 1A 边界。

**Source audit:**

```sh
rg -n "ScheduleMode" anemone-kernel/src/sched/class anemone-kernel/src/sched/processor.rs
rg -n "SchedEvent|on_event|EnqueueReason|RequeueReason|SwitchOutReason" anemone-kernel/src/sched
rg -n "PROCESSOR|mark_need_resched|fetch_clear_need_resched|get_current_task|current_processor|percpu" anemone-kernel/src/sched/class
rg -n "local_requeue_current|requeue_current_legacy|local_enqueue\\(|remote_enqueue\\(|task_enqueue\\(|wake_enqueue\\(" anemone-kernel/src/sched anemone-kernel/src/task anemone-kernel/src/exception
rg -n "\\.enqueue\\(|\\.pick_next\\(|\\.on_tick\\(|OnTickAction" anemone-kernel/src/sched anemone-kernel/src/task
```

Findings:

- `ScheduleMode` 不在 class / processor facade 中出现；它仍只属于 `sched/mod.rs` scheduler core。
- `SchedEvent` / `on_event` / `EnqueueReason` / `RequeueReason` / `SwitchOutReason` 在 scheduler implementation 中无匹配。
- class 模块没有从 tick transaction 访问 processor percpu；唯一 `fetch_clear_need_resched()` / `#[percpu]` 命中是既有 idle loop，不是 `Scheduler::task_tick()`。
- 旧 trait method 名称 `OnTickAction`、`.enqueue()`、`.pick_next()`、`.on_tick()` 在 scheduler / task 中无匹配。
- 旧跨模块 `local_requeue_current` 仍是 1B 待拆入口；本 checkpoint 未重命名 `local_enqueue` / `remote_enqueue` / `task_enqueue`，因为 new-task publication 命名族清理属于 1B。

**Stop-condition assessment:** 未命中 1A 停止条件。RR / Idle 行为保持，不需要读取 wait-core private identity；class facade 不解释 `ScheduleMode`；没有引入 event bus；未触碰 trap/IPI pending plumbing、wait-core helper、task topology、clone fresh entity、Kconfig 或 EEVDF payload。

**Validation:**

```sh
just build
timeout 25s just xtask qemu --platform qemu-virt-rv64-pretest --image build/anemone.elf
git diff --check
just fmt kernel --check
```

Results:

- `just build` 通过，最终 kernel release build 完成，无 Rust warning。
- pretest QEMU smoke 通过：内核启动，KUnit `99` 项全通过，`/sbin/init` 启动 user-test；当前 pretest wait profile 跑完 glibc / musl，汇总 `attempted=38 passed=38 failed=0 infra_failed=0 skipped=0` 后正常关机。
- `git diff --check` clean。
- `just fmt kernel --check` 失败，但失败只来自本轮未触碰的既有格式漂移：generated `anemone-kernel/src/kconfig_defs.rs`、generated `anemone-kernel/src/platform_defs.rs` 和 `anemone-kernel/src/task/topology/parent_child.rs`；本轮 touched scheduler files 不再出现在 fmt diff 中。本 checkpoint 未运行全量 format，以免改动无关文件。

**Feedback:** None. 未发现需要改变 1A write set、stage order、review gate、accepted contract 或 tracking issue 状态的实现反馈。

**Review gate:** 独立只读 reviewer `Sartre` 按 1A write set、method-first surface、RR / Idle 行为保持、`ScheduleMode` / event-bus 泄漏、临时 wrapper 边界和 1B / 2A 越界风险检查当前 diff，结论为 no blocking findings。

**Next:** Checkpoint 1B。`EEVDF-005` 仍 active，必须在 1B 完成 pick / set-next / mapping 准备 / task switch-in hook / current-task 更新顺序 source audit 后再审查能否 neutralize。

## Open Items

- `EEVDF-005` 仍 active：1A 提供 `set_next_task(task, now)` 落点，1B 完整 source-audit switch-in 顺序。
- `EEVDF-001` / `EEVDF-020` 仍 active：Checkpoint 2C 关闭 `rq_vtime`、eligibility、arithmetic 与 anomaly 语义。
- `EEVDF-002` 仍 active：Checkpoint 2B 关闭 EEVDF private `account_current(now)` 幂等边界。
- `EEVDF-004` 仍 active：Checkpoint 2D 关闭 ordinary wake / parked handoff exactly-once wake clamp。
- `EEVDF-017` 仍 active：阶段 3 default switch 前必须消费 1A / 1B / 2A / 2B / 2C / 2D 全部 gate。

## Closure

事务仍在进行中。
