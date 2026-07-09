# 2026-07-09 - Sched EEVDF-lite

**Status:** Active
**Owners:** doruche, Codex
**Area:** scheduler / fairness / runtime accounting / scheduler class
**Canonical Plan:** [RFC-20260622-sched-eevdf-lite](../../rfcs/sched-eevdf-lite/index.md), [不变量需求](../../rfcs/sched-eevdf-lite/invariants.md), [迁移实施计划](../../rfcs/sched-eevdf-lite/implementation.md), [Tracking Issues](../../rfcs/sched-eevdf-lite/tracking-issues.md)
**Current Phase:** Checkpoint 2A 已关闭；下一步是 Checkpoint 2B - Gate P1 `account_current(now)` 与入队前执行段结算

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

**Completed:** 公开 RFC 目录已存在，阶段 0 所需四份 RFC 文档已读取：`index.md`、`invariants.md`、`implementation.md`、`tracking-issues.md`。register、current limitations、RFC workflow、RFC template、devlog workflow、templates、事务索引、当前双周 devlog、SUMMARY 和 RFC 列表已读取。阶段 0 建立本事务日志，并把 RFC、tracking issues、事务索引、当前双周 devlog、mdBook Summary 和 RFC 列表连接到同一实现记录。总控完成阶段 0 source audit；未启动 subagent，因为阶段 0 不需要 worker 写代码。Checkpoint 1A 已完成：`Scheduler` trait 改为 method-first transaction surface，`RunQueue` 与 `SchedEntity` 拆出同一 owner 内文件边界，RR / Idle 完成行为保持适配。Checkpoint 1B 已完成：processor pending request 升级为 `PendingResched` flags，trap / idle / tick / IPI producer 接入 typed pending，schedule entry 拆分为 preempt / yield / idle，new-task enqueue 与 current requeue facade 改为语义化命名，owner CPU placement 后的 preempt decision 已接线，`EEVDF-005` 已通过 source audit neutralized。Checkpoint 2A 已完成：`Eevdf` class scaffold、`EevdfEntity` payload 字段位置、非 `Copy` class-specific `SchedEntity`、显式 `new_eevdf()` constructor、fresh clone/default-normal entity 构造和 EEVDF scheduler constants 的 kconfig schema / live root config / generated defs plumbing 已接入；default normal constructor 仍保持 RR，未提前切换到 EEVDF。

**In Progress:** 无 worker 正在运行。下一步是 Checkpoint 2B，必须限制在 2B write set 内实现 EEVDF private `account_current(now)` 与入队前执行段结算；不得消费 2C eligibility / yield、2D wake clamp 或阶段 3 default normal switch。

**Open Blockers:** 无阶段 0 / 1A / 1B / 2A 停止条件。当前 active Keter `EEVDF-001`、`EEVDF-002`、`EEVDF-004`、`EEVDF-017`、`EEVDF-020` 都已有后续 checkpoint 或 gate 归属；`EEVDF-005` 已关闭。下一项待关闭 blocker 是 `EEVDF-002`，归属 Checkpoint 2B / Gate P1。

**Next Action:** 分派或执行 Checkpoint 2B：实现 EEVDF private `account_current(now)`，证明 tick / switch-out / requeue 不双记，且 `DeferredPreempt` 不触发 fair accounting。

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

### 2026-07-09 - Checkpoint 1B Typed Pending、Schedule Entry、Trap / IPI Plumbing 与 `EEVDF-005`

**Phase:** 阶段 1 - Checkpoint 1B。

**Change:** 总控直接执行本 checkpoint，并启动只读 explorer `Gauss` 做 1B source map；无 worker 越界写代码。本轮 write set 限制在 1B 允许的 scheduler core / processor / class facade / IPI / trap / bootstrap enqueue rename / clone-kthreadd publication rename。processor `need_resched: bool` 替换为 `PendingResched` flags，并提供 `request_resched(ReschedCause)`、`take_pending_resched()` 和 `restore_pending_resched(PendingResched)`。`task_tick()` 请求 `ReschedCause::Tick`；owner CPU new / wake placement 后通过 `RunQueue::decide_preempt_current()` 决定是否插入 `ReschedCause::RunnableArrival`。trap tail、kernel trap tail 只在 pending 非空时调用 `schedule_preempt(pending)`；idle loop 只在 pending 非空时调用 `schedule_idle()`；deferred preempt 必须保留同一组 pending bits。

**Schedule entry split:** `ScheduleMode::Runnable` 拆为 `Yield` 和 `Idle`；`schedule_runnable()` 更名为 `schedule_yield()`，idle task 只走 `schedule_idle()`。`local_requeue_current()` / `RunQueue::requeue_current_legacy()` 泛名入口删除，跨模块 current lifecycle 改为 `local_requeue_yielded_current()`、`local_requeue_preempted_current()`、`local_handoff_woken_current()`、`local_requeue_aborted_wait_current()`、`local_put_prev_blocked()` 和 `local_put_prev_exiting()`。`AbortWaitSleep` 是 no-switch abort，直接返回 `DidNotSwitch`，不调用 class transaction；`Parked` 后被 wake 的 current 归入 parked handoff，调用 `handoff_woken_current()`。`local_requeue_aborted_wait_current()` 作为已接入 facade 保留给后续确有 abort-park requeue 的路径，第一版 1B source audit 未发现需要调用它的 no-switch path。

**New-task enqueue split:** `task_enqueue()` / `local_enqueue()` / `remote_enqueue()` 清理为 `enqueue_new_task()` / `local_enqueue_new_task()` / `remote_enqueue_new_task()`；bootstrap first enqueue 同步改为 `local_enqueue_first_new_task()`；clone publish 和 `kthreadd` publish 只调用 new-task publication path。wait completion 仍只通过 stale-safe `wake_enqueue()`。

**Source audit:**

```sh
rg -n "mark_need_resched|fetch_clear_need_resched|need_resched|PendingResched|request_resched|take_pending_resched|restore_pending_resched" anemone-kernel/src
rg -n "schedule_runnable|schedule_yield|ScheduleMode::Runnable|ScheduleMode::Yield|ScheduleMode::Idle|local_requeue_current|requeue_current_legacy|local_requeue_|local_handoff_woken_current|local_put_prev_" anemone-kernel/src/sched
rg -n "task_enqueue\\(|local_enqueue\\(|remote_enqueue\\(|enqueue_new_task|local_enqueue_new_task|remote_enqueue_new_task|local_enqueue_first\\(|local_enqueue_first_new_task" anemone-kernel/src
rg -n "SchedEvent|on_event|EnqueueReason|RequeueReason|SwitchOutReason" anemone-kernel/src/sched
rg -n "switch_mapping|local_pick_next|set_next_task|on_switch_in|set_current_task|switch_to\\(" anemone-kernel/src/sched anemone-kernel/src/task anemone-kernel/src/arch
```

Findings:

- 旧 `mark_need_resched()` / `fetch_clear_need_resched()` / `need_resched` bool 不再存在；typed pending 覆盖 tick、runnable arrival 和 caller-owned deferred-preempt restore。
- `schedule_runnable()`、`ScheduleMode::Runnable`、`local_requeue_current()` 和 `requeue_current_legacy()` 无匹配；yield / preempt / idle / block / exit / parked handoff 已分流。
- 旧 `task_enqueue(` / `local_enqueue(` / `remote_enqueue(` / `local_enqueue_first(` 无匹配；`WakeUpTaskStaleSafe` 仍是 wait-core stale-safe wake IPI，不属于 new-task publication rename。
- scheduler implementation 没有引入 `SchedEvent` / `on_event` / `EnqueueReason` / `RequeueReason` / `SwitchOutReason`。
- `local_pick_next()` 在 interrupt-disabled scheduler loop 中调用 `RunQueue::pick_next_task()` 后立即调用 `RunQueue::set_next_task(&task, Instant::now())`；scheduler loop 随后执行 `switch_mapping(prev, next)` 和 `switch_to(next)`；`switch_to()` 内执行 `Task::on_switch_in()`、`set_current_task(Some(task))` 和 architecture context switch。
- `AbortWaitSleep` 和 `DeferredPreempt` 都在 `schedule_inner()` 中提前返回，不调用 `switch_out()`，也不会进入 `local_pick_next()` / `set_next_task()`；真正切换路径都复用同一 next selection 顺序。`EEVDF-005` 因此移入 Neutralized。

**Stop-condition assessment:** 未命中 1B 停止条件。typed pending 可在 trap / idle / IPI / tick 路径间保持；remote runnable arrival 通过目标 CPU IPI/local placement 后决策，不在 source CPU 比较目标 current；current lifecycle 均有 method-first facade；wait-core private identity 未暴露给 scheduler class；deferred preempt 不触发 switch-out accounting；`EEVDF-005` source audit 具备关闭证据。

**Validation:**

```sh
just build
timeout 25s just xtask qemu --platform qemu-virt-rv64-pretest --image build/anemone.elf
git diff --check
just fmt kernel --check
```

Results:

- `just build` 通过，最终 kernel release build 完成，无 Rust warning。
- rv64 pretest QEMU smoke 已启动内核，KUnit `99` 项全通过，`/sbin/init` 启动 user-test，并进入 LTP read-write profile；该运行跑到 `pwritev03_64` 后在 25s timeout 到期时结束，不是 clean shutdown，不能作为完整 user-test 通过证据，只作为 boot / KUnit / early user-test sanity。
- `git diff --check` clean。
- `just fmt kernel --check` 仅报告本轮未触碰的既有格式漂移：generated `anemone-kernel/src/kconfig_defs.rs`、generated `anemone-kernel/src/platform_defs.rs` 和 `anemone-kernel/src/task/topology/parent_child.rs`；touched files 不在 fmt diff 中。
- 用户明确要求不用测试 la 的 QEMU，本 checkpoint 未运行 loongarch64 QEMU。

**Feedback:** Source audit 发现 `implementation.md` 的 1B bullets 容易把 no-switch abort、abort-park requeue 和 `ParkPending` handoff 混读，已回写澄清：`ParkPending` 由 `handoff_woken_current()` 收口，no-switch abort 不调用 class transaction，`requeue_aborted_wait_current()` 只保留给无 wake reward 的 abort-park requeue 路径。未发现需要改变 1B write set、stage order、review gate 或 accepted contract 的实现反馈；`EEVDF-005` 按 1B source-audit 关闭条件移入 Neutralized。

**Feedback:** 用户反馈指出 `schedule_preempt(pending)` deferred 时内部 restore caller 传入的 `PendingResched` value，会把普通 Copy flags value 误表达成 processor pending slot capability。已接受并修正为 caller-owned restore：trap tail 执行 `take_pending_resched()` 后，若 `schedule_preempt(pending)` 返回 `Deferred`，由该 caller 显式调用 `restore_pending_resched(pending)`；scheduler core 不再在 `DeferredPreempt` 分支写 processor pending state。该反馈不改变 1B write set、阶段顺序或 `EEVDF-005` 关闭结论。

**Review gate:** 只读 explorer `Gauss` 先行列出 1B 风险点：旧 bool pending、trap/idle/IPI producers、`ScheduleMode::Runnable`、`local_requeue_current`、block / exit `put_prev_*`、旧 enqueue names 和 `EEVDF-005` no-switch / deferred-preempt audit。总控按这些点完成 source audit；未发现 blocking finding。

**Next:** Checkpoint 2A。不得在 2A 之前切换 default normal class；`EEVDF-001` / `EEVDF-002` / `EEVDF-004` / `EEVDF-017` / `EEVDF-020` 仍按阶段 2 / 3 gate 关闭。

### 2026-07-09 - Checkpoint 2A Payload / Class Compile Scaffold

**Phase:** 阶段 2 - Checkpoint 2A。

**Change:** 总控直接执行本 checkpoint，未启动实现 worker；按用户要求启动只读 reviewer `Aquinas` 审查代码。`sched/class/eevdf.rs` 新增 `Eevdf` class scaffold、线性 `Vec<Arc<Task>>` ready queue、RR-like conservative tick / runnable-arrival request 占位和 `EevdfEntity` payload 字段位置：`vruntime`、`deadline`、`slice`、`exec_start`、`initialized`、fallback anomaly 诊断字段。`SchedEntity` 不再是 `Copy`，`SchedClassPrv` 改为 class-specific payload，并增加 `SchedClassKind` 作为 observation-only class snapshot。`SchedEntity::new_normal()` 当前仍返回 RR，`SchedEntity::new_eevdf()` 只作为显式定向 constructor，`SchedEntity::new_idle()` 收敛 idle payload 构造。bootstrap / kthread / clone publication 改为调用 `new_normal()`；clone 不再复制 `current_task.sched_entity()`。

**Kconfig:** 新增 EEVDF scheduler constants 的配置 schema 和默认值：`eevdf_base_slice_us`、`eevdf_wake_clamp_us`、`eevdf_yield_penalty_us`、`eevdf_anomaly_threshold`。已运行 `just defconfig` 同步 live root `kconfig`，并通过 `just build` 生成 `anemone-kernel/src/kconfig_defs.rs` 中的 `EEVDF_BASE_SLICE_US`、`EEVDF_WAKE_CLAMP_US`、`EEVDF_YIELD_PENALTY_US` 和 `EEVDF_ANOMALY_THRESHOLD`。2A 只建立配置路径；base slice / yield penalty / anomaly threshold 的语义消费仍归属 2C，wake clamp window 的语义消费仍归属 2D。

**Boundary repair:** reviewer `Aquinas` 初审发现一个 Keter：`RunQueue::{dequeue, enqueue_with, requeue_current_with}` 在持有 task `SchedEntity` lock 时调用 class transaction，会阻塞 2B / 2C 在 `Eevdf` class 内部修改自己的 per-task payload，迫使 self-lock 或把 EEVDF policy 泄漏回 `RunQueue`。已修正为：`RunQueue` 先读取短 `SchedClassKind` snapshot，只在短锁内检查 / 更新 `on_runq` 和 class-kind consistency；class transaction 在不持有 task entity lock 时执行。`pick_next_task()` 清 `on_runq` 时也检查被选择 task 的 expected class kind。`Aquinas` 复审结论为 no blocking Checkpoint 2A findings。

**Source audit:**

```sh
rg -n "sched_entity\(\)|current_task\.sched_entity\(|SchedEntity::new\(SchedClassPrv|SchedClassPrv::RoundRobin\(\(\)\)|SchedClassPrv::Idle\(\(\)\)|SchedEntity::new_normal|SchedEntity::new_eevdf|SchedEntity::new_idle" anemone-kernel/src -g '*.rs'
rg -n "ScheduleMode|SchedEvent|on_event|EnqueueReason|RequeueReason|SwitchOutReason" anemone-kernel/src/sched/class anemone-kernel/src/sched/processor.rs
rg -n "cached_weight|nice:" anemone-kernel/src/sched/class/eevdf.rs anemone-kernel/src/sched/class/entity.rs anemone-kernel/src/task -g '*.rs'
rg -n "EEVDF_BASE_SLICE_US|EEVDF_WAKE_CLAMP_US|EEVDF_YIELD_PENALTY_US|EEVDF_ANOMALY_THRESHOLD|eevdf_base_slice_us|eevdf_wake_clamp_us|eevdf_yield_penalty_us|eevdf_anomaly_threshold" conf/.defconfig kconfig scripts/xtask/src/config/kconfig.rs anemone-kernel/src/kconfig_defs.rs anemone-kernel/src/sched/class/eevdf.rs
```

Findings:

- `sched_entity()` / `current_task.sched_entity()` 旧 by-value copy getter 已消失；clone 使用 fresh `SchedEntity::new_normal()`，不复制父 task entity。`Task::nice()` 继承仍通过原有 `set_nice(current_task.nice())` 路径完成。
- default normal 入口仍通过 `SchedEntity::new_normal()` 返回 `SchedClassPrv::RoundRobin(())`；`new_eevdf()` 只在 entity constructor 中存在，普通 task、bootstrap task、kthread 和 clone child 未提前切到 EEVDF。
- class / processor facade 中没有 `ScheduleMode` 泄漏；scheduler class implementation 没有 `SchedEvent` / `on_event` / `EnqueueReason` / `RequeueReason` / `SwitchOutReason`。
- EEVDF entity 没有长期 `nice` 或 `cached_weight` 字段；2A 没有把 `Task::nice()` 缓存成第二真相源。
- root `kconfig`、`conf/.defconfig`、`scripts/xtask/src/config/kconfig.rs`、generated `kconfig_defs.rs` 和 `eevdf.rs` 可见 EEVDF constants；live root config 已消费新增参数。
- `Eevdf` scaffold 中的 `rq_vtime`、fallback anomaly、tick/preempt decision 都是字段 / conservative placeholder；没有声明 `EEVDF-001`、`EEVDF-002`、`EEVDF-004` 或 `EEVDF-020` 语义闭合。

**Stop-condition assessment:** 未命中 2A 停止条件。2A 没有提前切换 default normal class；没有复制父 `SchedEntity`；没有把 `Task::nice()` 缓存成 EEVDF 第二 truth；没有实现或沉淀 runtime accounting、eligibility、yield penalty 或 wake clamp 语义；没有扩大 wait-core、task topology、trap/IPI 或 public scheduler policy 边界。`RunQueue` lock-boundary Keter 已在本 checkpoint 内修正并通过复审。

**Validation:**

```sh
just defconfig
just build
git diff --check
just fmt kernel --check
```

Results:

- `just defconfig` 通过，将 `.defconfig` 中新增 EEVDF constants 同步到 live root `kconfig`。
- `just build` 通过，最终 kernel release build 完成，无 Rust warning。
- `git diff --check` clean。
- `just fmt kernel --check` 仍失败，但只报告 generated `anemone-kernel/src/kconfig_defs.rs` 和 generated `anemone-kernel/src/platform_defs.rs` 的既有格式漂移；本 checkpoint touched 非 generated Rust source 不再出现在 fmt diff 中。本 checkpoint 未运行全量 format，以免改动 generated / unrelated 文件。
- 未运行 QEMU / LTP / directed EEVDF runtime smoke；2A 是 compile scaffold gate，explicit EEVDF entity 的 runtime smoke 留给后续 gate 或定向 probe。

**Review gate:** 只读 reviewer `Aquinas` 初审报告 `RunQueue` 持 entity lock 调 class transaction 的 Keter；总控修复后复审结论为 no blocking Checkpoint 2A findings。残余 gap 是尚无 explicit EEVDF entity runtime smoke，按 2A scaffold 边界可接受。

**Next:** Checkpoint 2B / Gate P1。不得在 2B 中消费 2C `rq_vtime` / arithmetic / bounded yield、2D wake clamp 或阶段 3 default normal switch。

## Open Items

- `EEVDF-001` / `EEVDF-020` 仍 active：Checkpoint 2C 关闭 `rq_vtime`、eligibility、arithmetic 与 anomaly 语义。
- `EEVDF-002` 仍 active：Checkpoint 2B 关闭 EEVDF private `account_current(now)` 幂等边界。
- `EEVDF-004` 仍 active：Checkpoint 2D 关闭 ordinary wake / parked handoff exactly-once wake clamp。
- `EEVDF-017` 仍 active：阶段 3 default switch 前必须消费 1A / 1B / 2A / 2B / 2C / 2D 全部 gate；其中 1A / 1B / 2A 已关闭。

## Closure

事务仍在进行中。
