# 2026-07-09 - Sched EEVDF-lite

**Status:** Active
**Owners:** doruche, Codex
**Area:** scheduler / fairness / runtime accounting / scheduler class
**Canonical Plan:** [RFC-20260622-sched-eevdf-lite](../../rfcs/sched-eevdf-lite/index.md), [不变量需求](../../rfcs/sched-eevdf-lite/invariants.md), [迁移实施计划](../../rfcs/sched-eevdf-lite/implementation.md), [Tracking Issues](../../rfcs/sched-eevdf-lite/tracking-issues.md)
**Current Phase:** 阶段 3 runtime stop；RFC correction gates 已重开，下一门为 R1 weighted FairClock

## Scope

本事务跟踪 `sched-eevdf-lite` RFC 从阶段 0 到第一版 default normal scheduler 切换和收口的实现过程。

阶段顺序以 RFC `implementation.md` 为准：

- 阶段 0 只关闭文档协议、建立事务日志并审计当前 sched-split / scheduler class 接缝；
- 阶段 1A 先做 method-first `Scheduler` trait、`RunQueue` facade、`SchedEntity` 拆分和 RR / Idle 行为保持；
- 阶段 1B 再接 typed pending resched、schedule entry、trap / IPI plumbing 和 `EEVDF-005` switch-in source audit；
- 阶段 2A / 2B / 2C / 2D 分别关闭 EEVDF payload scaffold、runtime accounting、`rq_vtime` / arithmetic / bounded yield、wake clamp / parked handoff；
- 阶段 3 才翻转 default normal class；
- 阶段 3 runtime feedback 命中停止条件时，按 R1 / R2 / R3a / R3b 纠正 2C / 2D contract，再恢复 clean-tree runtime 验收；
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
- `Nice` newtype 约束值域；Task 内部原子 nice 表示是 nice / weight 的唯一长期真相源。
- `ScheduleMode` 属于 scheduler core entry permission，不得泄漏到 scheduler class。
- `PendingResched` 只能作为 processor / scheduler-core private pending request；class 最多按值读取 preempted-current transaction 参数。
- scheduler class contract 必须是 method-first class-local atomic transaction surface。
- true wake lag restore 只由 `enqueue_woken()` 表达；`handoff_woken_current()` 是 continuous active-to-ready transfer，不执行 wake reward；wait-core private identity 不进入 class 算法状态。
- worker 未经总控和用户批准不得越过当前阶段 write set；若更好的架构需要扩大 write set，先上报 expansion request，并把批准结果写入本事务日志。

## Handoff

**Last Updated:** 2026-07-12

**Current Branch:** `dev/drc/eevdf`

**Completed:** 公开 RFC 目录已存在，阶段 0 所需四份 RFC 文档已读取：`index.md`、`invariants.md`、`implementation.md`、`tracking-issues.md`。register、current limitations、RFC workflow、RFC template、devlog workflow、templates、事务索引、当前双周 devlog、SUMMARY 和 RFC 列表已读取。阶段 0 建立本事务日志，并把 RFC、tracking issues、事务索引、当前双周 devlog、mdBook Summary 和 RFC 列表连接到同一实现记录。总控完成阶段 0 source audit；未启动 subagent，因为阶段 0 不需要 worker 写代码。Checkpoint 1A 已完成：`Scheduler` trait 改为 method-first transaction surface，`RunQueue` 与 `SchedEntity` 拆出同一 owner 内文件边界，RR / Idle 完成行为保持适配。Checkpoint 1B 已完成：processor pending request 升级为 `PendingResched` flags，trap / idle / tick / IPI producer 接入 typed pending，schedule entry 拆分为 preempt / yield / idle，new-task enqueue 与 current requeue facade 改为语义化命名，owner CPU placement 后的 preempt decision 已接线，`EEVDF-005` 已通过 source audit neutralized。Checkpoint 2A 已完成：`Eevdf` class scaffold、`EevdfEntity` payload 字段位置、非 `Copy` class-specific `SchedEntity`、显式 `new_eevdf()` constructor、fresh clone/default-normal entity 构造和 EEVDF scheduler constants 的 kconfig schema / live root config / generated defs plumbing 已接入；default normal constructor 仍保持 RR，未提前切换到 EEVDF。2B 前 feedback 已收口：`Eevdf` class 内部增加 typed entity accessor，后续 `account_current(now)` 可通过 class-private helper 短暂访问 `EevdfEntity`，不把 `SchedEntity` guard 或 typed payload 参数加入 `Scheduler` trait；RFC canonical 文本补充了 future scheduler policy / class switch 必须通过 owner CPU `RunQueue` command / IPI 线性化，远端不得直接修改 `SchedEntity` class 或 EEVDF payload。Checkpoint 2B 已完成：`Eevdf` private `account_current(now)` 成为唯一推进 current execution segment 的 helper；`set_next_task()` 记录 `exec_start`；tick、yield / preempt requeue、parked handoff、abort-park requeue、block 和 exit switch 都通过同一 helper 结算并刷新 `exec_start`；`Task::on_switch_out()` 保持 task / CPU usage bookkeeping，不成为 fair scheduler accounting truth；`EEVDF-002` 已移入 Neutralized。Checkpoint 2C 已完成：weighted virtual-time arithmetic、monotonic `rq_vtime`、eligible / fallback pick、new placement、deadline renewal、tick / runnable-arrival preempt、bounded yield、nice visibility 和 anomaly observation 已实现；2C / 2D wake 边界与 default-normal RR 边界保持不变，`EEVDF-001` / `EEVDF-020` 已移入 Neutralized。2C 后 class-shape feedback correction 已完成：class precedence 集中为单一 high-to-low order，pick / preempt 共用；EEVDF payload 与通用 class constructor 可见面已收窄。Checkpoint 2D 已完成：ordinary wake 与 parked current handoff 通过同一个 bounded wake clamp transaction 收口，stale / already-current / already-queued / no-switch abort / abort-park 路径保持 no-reward，`EEVDF-004` 已移入 Neutralized；阶段 2 全部 checkpoint 关闭。阶段 3 前 Nice / Priority 边界反馈纠正也已关闭：typed nice、Task writer、clone inheritance、priority syscall 目录化和低代价 target-selection / ABI 修复均已落地，不新增 Checkpoint 2E。

**Scheduler-core correction:** 上述阶段 1B / 2B / 2D 对 abort-park requeue 的记录只描述当时保留的假设性 surface。2026-07-12 source audit 已证明 production 状态机只有 already-completed no-switch abort 与 parked handoff 两条路径，并删除无调用者的 `requeue_aborted_wait_current()`；后文同日 correction 条目 supersede 这些历史表述，不改变 R1-R3b 状态。

**Correction:** 2026-07-11 user-run evidence 已撤销上述 Checkpoint 2C min-floor / arithmetic 与 Checkpoint 2D parked-handoff clamp 的算法 closure。default constructor、typed pending、single accounting owner 和 `EEVDF-022` outcome propagation 仍有效；`EEVDF-001` / `EEVDF-018` / `EEVDF-004` / `EEVDF-020` 已重开。当前 canonical 顺序为 R1 -> R2 -> R3a -> R3b -> Stage 3 clean-tree runtime。

**In Progress:** 文档层 reopening 已建立 factual evidence packet、weighted FairClock / competition membership / service-lag / arithmetic invariants 和 correction gates。尚未开始 R1 生产代码；evidence-only probe commit `d0d4196f` 与公共实现基线 `a76a00ac` 保持分离。

**Open Blockers:** `EEVDF-001` / R1 是当前首个 blocker；R1 未证明 actual weighted eligibility 与 direct-causality intervention 前不开始 R2。`EEVDF-018` / `EEVDF-004` 等待 R2，`EEVDF-020` 等待 R2 / R3a / R3b。

**Next Action:** 按 Gate R1 在 clean product branch 实现 weighted FairClock 单变量修复；若需要 runtime probe，在独立 validation branch 复用同一观察语义，不整体 cherry-pick `d0d4196f`。先完成 build / KUnit / source audit，再由用户运行 instrumented signal profile。

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

### 2026-07-09 - 2B 前 feedback：EEVDF typed entity accessor

**Phase:** Checkpoint 2B 前 implementation feedback。

**Change:** 用户反馈确认 2A 为避免 self-lock 保留的短锁 snapshot 写法可以在 2B 前收口。总控将 `Eevdf` class 内部的 class-kind assertions 收敛为 class-private `with_entity_mut()` / `assert_entity()` helper。`Scheduler` trait surface 保持 method-first `Arc<Task>` transaction 形状，不增加 typed `SchedEntity` 或 entity guard 参数；`RunQueue` 仍负责 queue membership、`on_runq`、`ntasks` 和全局 scheduler 线性化。

**Boundary:** 该反馈只清理 EEVDF class 内部 payload access 形状，不实现 `account_current(now)`、eligibility、yield penalty、wake clamp 或 default normal switch。`with_entity_mut()` 的锁生命周期保持在 class-private helper 内，避免把 task entity lock order 扩散到 trait API；未来 2B 的 accounting helper 可以复用该入口读写 `EevdfEntity`。

**RFC update:** `invariants.md` 现在明确 effective scheduler class、class-specific payload 和 queue membership 只能由 owner CPU `RunQueue` transaction 修改；future scheduler policy / class switch 必须作为 owner CPU command / IPI 线性化。`index.md` 把 runtime policy / class switch 列为非目标和 follow-up RFC 事项；`implementation.md` 明确 2C 的 nice-to-weight visibility 不等同 class migration，nice 是 task-owned weight truth 的例外。

**Source audit:**

```sh
rg -n "with_entity_mut|assert_entity|sched_class_kind\\(\\)" anemone-kernel/src/sched/class/eevdf.rs anemone-kernel/src/sched/class/runqueue.rs
rg -n "fn .*SchedEntity|EevdfEntity" anemone-kernel/src/sched/class/mod.rs anemone-kernel/src/sched/class/eevdf.rs
```

Findings:

- `Scheduler` trait 没有新增 typed entity 参数或 guard 参数。
- `Eevdf` 内部不再直接调用 observation-only `sched_class_kind()` 断言自身任务类型，改由 class-private typed helper 验证 payload。
- `RunQueue` 的短锁 snapshot / membership update 形状保持不变；本反馈没有回退到持 entity lock 调 class transaction。

**Next:** 正式进入 Checkpoint 2B / Gate P1。

### 2026-07-09 - Checkpoint 2B Gate P1 `account_current(now)` 与执行段结算

**Phase:** 阶段 2 - Checkpoint 2B / Gate P1。

**Change:** 总控直接执行本 checkpoint，未启动 implementation worker；启动只读 explorer `Newton` 做 2B source-map 审计。`Eevdf` class 新增 private `account_current(now)` helper，并把 current execution accounting 接入所有 2B 生命周期 transaction：`task_tick()`、`requeue_yielded_current()`、`requeue_preempted_current()`、`handoff_woken_current()`、`requeue_aborted_wait_current()`、`put_prev_blocked()` 和 `put_prev_exiting()`。`set_next_task(task, now)` 只记录下一段 `exec_start`。`account_current(now)` 在成功推进后刷新 `exec_start = now`，因此 tick 之后的 switch-out / requeue 只结算 tick 之后的新执行段。`switch.rs::switch_out()` 增加边界注释，明确 `Task::on_switch_out()` 仍只负责 task / CPU usage bookkeeping，不是 EEVDF fair accounting truth。

**Boundary:** 2B 只证明 accounting transaction ordering 和 `exec_start` 刷新纪律。`runtime_delta_to_vruntime()` 暂用单调 actual-runtime scalar 作为 2B 证明载体；weighted virtual-time arithmetic、`rq_vtime` 更新、deadline / slice fail-closed 规则、bounded yield、nice-to-weight visibility 和 anomaly 语义仍归属 Checkpoint 2C / Gate P2。wake clamp 仍归属 Checkpoint 2D；default normal constructor 仍未切换到 EEVDF。

**Source audit:**

```sh
rg -n "account_current|set_exec_start|runtime_delta_to_vruntime|on_switch_out|DeferredPreempt|local_requeue_|local_put_prev_|local_handoff" anemone-kernel/src/sched anemone-kernel/src/task -S
```

Findings:

- `local_pick_next()` 在 `RunQueue::pick_next_task()` 后调用 `RunQueue::set_next_task(&task, Instant::now())`；`Eevdf::set_next_task()` 只设置 `exec_start`。scheduler loop 之后才执行 `switch_mapping(prev, next)` 和 `switch_to(next)`。
- `task_tick()` 通过 `Eevdf::account_current()` 推进当前段并刷新 `exec_start`，随后仍保守返回 `TickAction::RequestResched`；真实 tick preemption policy 留给 2C。
- `requeue_yielded_current()`、`requeue_preempted_current()`、`handoff_woken_current()` 和 `requeue_aborted_wait_current()` 都先调用同一个 helper 再入队。
- `put_prev_blocked()` 和 `put_prev_exiting()` 只调用同一个 helper，不入队。
- `DeferredPreempt` 在 `schedule_inner()` 中提前返回，不调用 `switch_out()`、`local_pick_next()`、`set_next_task()` 或任何 EEVDF class transaction；trap caller 仍拥有 restore pending flags。
- `switch_out()` 只在 class transaction 之后调用；`Task::on_switch_out()` 仍通向 `task/cpu_usage.rs`，没有 EEVDF accounting 依赖。
- 只读 explorer `Newton` 独立确认当前 write set 只包含 `sched/class/eevdf.rs` 与 `sched/switch.rs`，均在 2B 允许范围内；未发现需要越过 2B write set、进入 2C / 2D / 阶段 3，或改变 `Task::on_switch_out()` ownership 的停止信号。

**Stop-condition assessment:** 未命中 2B 停止条件。没有发现某个 schedule path 在 class transaction 前重新入队；`account_current(now)` 能在 tick 与 switch-out / requeue 之间通过刷新 `exec_start` 避免双记；fair accounting 不依赖 `switch.rs::switch_out()` 的 task hook。

**Validation:**

```sh
just build
git diff --check
mdbook build docs
just fmt kernel --check
```

Results:

- `just build` 通过，最终 kernel release build 完成，无 Rust warning。
- `git diff --check` clean。
- `mdbook build docs` 通过，HTML 输出到 `docs/book`。
- `just fmt kernel --check` 仍失败，但只报告 generated `anemone-kernel/src/kconfig_defs.rs` 和 generated `anemone-kernel/src/platform_defs.rs` 的既有格式漂移；本 checkpoint touched Rust source 不再出现在 fmt diff 中。本 checkpoint 未运行全量 format，以免改动 generated / unrelated 文件。
- 未运行 QEMU、LTP、iozone 或 directed EEVDF runtime smoke；2B 的最低验证是 build + source audit。tick/switch 不双记的 runtime smoke 若后续低成本可用，可作为 2C/2D 前额外证据，但不是 2B 关闭条件。

**Review gate:** 只读 reviewer `Meitner` 审查 2B diff，未发现 Apollyon / Keter / Euclid；唯一 Safe note 是在接受本 review 后把本条 pending review 记录改为实际结论。总控已接受该 review 结论。

**Tracking:** `EEVDF-002` 已移入 Neutralized。`EEVDF-001` / `EEVDF-020` 仍 active，归属 2C；`EEVDF-004` 仍 active，归属 2D；`EEVDF-017` 仍约束阶段 3 default switch。

**Next:** Checkpoint 2C / Gate P2。不得在 2C 前消费 2D wake clamp 或切换 default normal class。

### 2026-07-10 - Checkpoint 2C 前设计共识

**Phase:** Checkpoint 2C / Gate P2 前文档层共识。

**Change:** 用户要求在实现前用 grill-me 方式收敛 2C 的前置设计点。本轮未修改内核代码，只把共识回写到 RFC canonical 文本和本事务日志，作为后续 2C worker contract。

**Consensus:**

- Virtual-time state：`Vruntime`、`Deadline` 和 `rq_vtime` 长期存储为 normalized nanoseconds 的 `u64` scalar；nice 0 下 `1ns` actual runtime 对应 `1` virtual ns。不引入额外 fixed-point fractional scale。
- Arithmetic helper：`delta_exec_ns * NICE_0_WEIGHT / weight` 与 slice/deadline 乘除使用 `u128` 中间值，统一在 EEVDF private helper saturate 回 `u64`；overflow / saturation 记录 anomaly，不 panic，不把 `Result` 扩散到 `Scheduler` trait 或 `RunQueue` surface。正 `delta_exec` 若计算为 0，至少推进 `1`。
- `rq_vtime`：第一版使用 monotonic min-vruntime floor，visible runnable set 包含 ready queue 和当前正在运行的 EEVDF task；`rq_vtime = max(rq_vtime, min_visible_vruntime)`，visible set 为空时保持不变。current 不回到 queue，也不参与 pick scan。
- Eligibility / fallback：eligible 判断为 `task.vruntime <= rq_vtime`。normal pick 在 eligible tasks 中选择最小 deadline；no-eligible fallback 选择最小 `vruntime` task，记录 anomaly，并把 `rq_vtime` 推进到 fallback task 的 `vruntime`。
- Placement：fresh `enqueue_new()` 使用 `vruntime = rq_vtime`，deadline 按当前 nice weight 和 base slice 计算。2C 的 `enqueue_woken()` 不做 wake clamp；未初始化 entity 只做安全初始化，已初始化 entity 保留既有 virtual-time state，真实 wake clamp 留给 2D。
- Deadline：deadline 只在初始化或 `vruntime >= deadline` 时自然续期；普通 requeue 不无条件重算 deadline。
- Preemption：`task_tick()` 先 account current；当前 `vruntime >= deadline`，或存在 eligible 且 deadline 严格早于 current deadline 的 queued task 时请求 resched。`decide_preempt_current()` 先 account current；candidate eligible 且 deadline 严格早于 current deadline 时请求 runnable-arrival resched。deadline 相等保持 current。
- Yield：bounded yield 只把 yielding task 的 deadline 后推到至少 `rq_vtime + yield_penalty_window_vruntime(weight)`，不修改 `vruntime`、nice 或 weight。
- Nice visibility：`Task::nice()` 保持唯一 weight truth；`setpriority()` / clone nice inheritance 后，下一次 owner CPU accounting / enqueue / pick / preempt decision 读取最新 nice。已存在 deadline 不因 renice 立即重算；2C 不引入远端 runqueue 重排、class migration 或直接修改 EEVDF payload 的路径。
- Anomaly：`anomaly` 是 EEVDF-lite 本地诊断概念，不是 Linux / EEVDF 标准状态；覆盖 no-eligible fallback 和 arithmetic saturation，默认以计数和 last reason 为最低观察面，不参与调度决策。

**Boundary:** `EEVDF-001` / `EEVDF-020` 仍保持 Active；本轮只关闭设计口径，不提供实现、source audit 或 smoke 证据。后续 2C 实现若发现公式或 arithmetic contract 不成立，必须回写 `index.md` / `invariants.md` / `tracking-issues.md`，不能只把事实写在本事务日志。

**Validation:**

```sh
git diff --check
```

结果：`git diff --check` clean。

### 2026-07-10 - Checkpoint 2C / Gate P2 实现启动与 Source Preflight

**Phase:** 阶段 2 - Checkpoint 2C / Gate P2，implementation in progress。

**Preflight:** 总控重新读取 RFC 四份 canonical 文档、transaction handoff、register 中 scheduler / wait 相关开放项和当前 scheduler class source。当前 `Eevdf` 仍是显式定向 class，`SchedEntity::new_normal()` 保持 RR；2B 的 `account_current(now)` 已存在，但 virtual-time conversion 仍是未加权 actual-runtime scalar，pick 仍取队首，tick / runnable-arrival 仍无条件请求 resched，`rq_vtime` 和 anomaly 字段尚未消费。`Task::nice()` 使用 Acquire load，`set_nice()` 使用 Release store；owner CPU 后续 accounting / placement 可以直接读取最新 nice，不需要修改 `task/api/priority.rs` 或新增远端 payload 更新路径。

**Implementation contract:**

- `Eevdf` 使用 class-owned weak current handle 表达“已从 queue pick 出、仍在运行并参与 visible set”的协议状态；该 handle 不拥有 task 生命周期，也不是诊断字段。
- `rq_vtime` 只通过 monotonic floor helper 推进；queue scan 与 current entity access 都保持短锁，不在持有一个 entity lock 时扫描其它 entity。
- nice-to-weight 使用固定 Linux 40 项表；所有 duration / slice / yield 乘除使用 `u128` 中间值和统一 saturating result，正 runtime delta 至少推进 `1`。
- no-eligible fallback 与 arithmetic saturation 进入 class-owned anomaly count / last reason；threshold 只限制连续 fallback 的诊断日志，不参与 pick。
- `enqueue_woken()` 在 2C 只安全初始化未初始化 entity，不执行 wake clamp；`handoff_woken_current()` 与 abort requeue 也不消费 wake clamp window。
- `RunQueue::decide_preempt_current()` 只在 current 与 candidate 同 class 时调用 class-local comparison；跨 class 只按现有 `Eevdf > RoundRobin > Idle` dispatch priority 决定，避免把 RR current 交给 EEVDF accounting。

**Write set:** `anemone-kernel/src/sched/class/eevdf.rs`、必要时 `anemone-kernel/src/sched/class/runqueue.rs`、本事务日志，以及 gate 关闭时的 RFC status / tracking issue 回写。未批准其它 owner surface；若实现要求修改 wait-core、task topology、priority ABI、trap/IPI 或 default constructor，命中停止条件。

**Review setup:** 已尝试启动两个只读 subagent 做 source map 与实现前风险审查，但外部代理服务以消费额度上限拒绝请求，两个 agent 都未运行、未修改文件。总控继续执行本 checkpoint；diff 形成后仍会尝试独立只读 review gate，若服务异常持续则如实记录，不伪造 review 结论。

### 2026-07-10 - Checkpoint 2C / Gate P2 实现、Review 与关闭

**Phase:** 阶段 2 - Checkpoint 2C / Gate P2 closed。

**Change:** `Eevdf` 接入固定 Linux nice weight 表和统一 `u128` 中间计算 / `u64` saturating virtual-time helper；正 runtime delta 至少推进 `1`，saturation 记录本地 anomaly。class-owned weak current handle 让已离开 ready queue 的运行任务继续参与 visible set，`rq_vtime` 以 monotonic min-vruntime floor 在 enqueue、dequeue、pick 和 accounting 后推进。pick 在线性 scan 中先选 eligible 最小 deadline，无 eligible 时 fallback 到最小 `vruntime`、推进公平时钟并记录 anomaly。fresh placement、自然 deadline renewal、tick / runnable-arrival preempt、bounded yield 和 Kconfig 常量消费均按 canonical contract 实现。`RunQueue` 只增加同 class / 跨 class preempt dispatch 分流，保持既有 `Eevdf > RoundRobin > Idle` 顺序，避免让 EEVDF accounting 读取 RR current。

**Boundary:** `enqueue_new()` 通过 release `assert!` fail closed 拒绝 initialized entity；`enqueue_woken()` 只为未初始化 entity 做安全初始化，已初始化 wake entity 保留原 virtual-time state。wake clamp window、ordinary wake / parked handoff reward、default normal constructor 和阶段 3 class switch 均未消费；`SchedEntity::new_normal()` 仍返回 RR。

**Source audit:** 线性 candidate scan 明确分离 eligible 最小 deadline 与 no-eligible 最小 `vruntime` fallback，没有 deadline-only 索引；ready queue 与 weak current 共同构成 visible set，current 不参与 queue membership / pick scan。nice 每次从 `Task::nice()` 读取，没有第二份长期 weight truth；base slice、yield penalty window 和 anomaly threshold 来自 live Kconfig。clone / fresh entity 不继承父 EEVDF payload，2D wake clamp constant 没有进入 2C 公式或调用路径。

**Independent review:** 用户补充消费限额后，两个只读 reviewer 均已自然完成，没有被终止或打断。source-map reviewer 最终报告无 Apollyon / Keter / Euclid；risk reviewer 初审发现 `enqueue_new()` freshness 未 fail closed 的 Keter，修复为 `initialize_fresh_entity()` 的 release assertion 后复审通过，无新增 blocker。剩余非阻塞验证建议是使用真实 `Task` / `RunQueue` 覆盖完整 weak-current 生命周期；当前 source audit 与算法 KUnit 已证明 Gate P2，default normal 尚未切换使该缺口不阻塞 2C。

**Validation:**

- `just build` 通过。
- `git diff --check` clean。
- `just fmt kernel --check` 只因既有 generated `kconfig_defs.rs` / `platform_defs.rs` 格式漂移失败；本 checkpoint 修改的 Rust 文件未出现在 formatter diff 中。
- `./scripts/run-user-test-rv64.sh etc/sdcard-rv.img build/eevdf-phase2c.log` 以提权 rootfs 构建路径执行并退出 `0`；QEMU 正常启动、完成测试并关机。
- rv64 pretest 运行 105 项 KUnit，全部通过；其中 6 项 EEVDF KUnit 覆盖 weighted arithmetic、saturation / anomaly、eligible pick、fallback、monotonic `rq_vtime` 和 bounded yield。
- read-write LTP 汇总为 `attempted=118 passed=96 failed=16 infra_failed=0 skipped=6`。这是当前 profile 的集成结果；default normal class 仍是 RR，因此不能作为真实 EEVDF workload 或 fallback-anomaly 稳态证据。

**Tracking:** `EEVDF-001` 与 `EEVDF-020` 已移入 Neutralized。未发现需要改变公式、不变量、write set、验证 floor 或接受边界的实现反馈；`EEVDF-004` 保持 active 并归属 Checkpoint 2D，`EEVDF-017` 继续阻止提前 default switch。

**Next:** Checkpoint 2D / Gate P3。不得把现有 RR 下的 LTP 结果描述成 EEVDF runtime 证据，也不得在 2D 前或 2D 内顺带切换 default normal class。

### 2026-07-10 - Checkpoint 2C 后 Class-shape Feedback Correction 启动

**Phase:** Checkpoint 2C closed 后的 implementation feedback correction，in progress。

**Accepted feedback:**

- 跨 class precedence 是 scheduler class 自身的静态 metadata；`RunQueue` 是比较结果的消费者，不应以 `RunQueue::class_rank()` 保存另一份 class-order truth。
- `EevdfEntity` 除 class owner 所需 constructor 外没有模块外消费者；其字段 accessor、payload enum 和通用 payload constructor 不应扩大为公共接口。
- nice 值域、`Task::nice()` / `set_nice()`、`MIN_NICE` / `MAX_NICE` 与 Linux weight table 的整体架构边界需要单独设计，本反馈明确延期，不顺带修改。

**Approved write set:** `anemone-kernel/src/sched/class/mod.rs`、`entity.rs`、`runqueue.rs`、`eevdf.rs`、`rr.rs`、`idle.rs`、RFC `implementation.md` 的 feedback / write-set 记录和本事务日志。用户已批准该同 owner 扩展；不触碰 task / priority owner、wait-core、Kconfig、2D wake clamp 或 default constructor。

**Implementation contract:** `sched/class/mod.rs` 以 high-to-low class order 集中保存唯一 precedence truth；每个 `Scheduler` implementation 只通过 `KIND` 关联自己的 class identity，不在 class 文件保存 rank 数字。`RunQueue` 的 pick 与跨 class preempt comparison 都消费同一 class-domain order，不复用 `SchedClassKind` discriminant、Linux `SCHED_*` policy number 或 syscall representation。`EevdfEntity` 与 `SchedClassPrv` 收窄到 class owner，外部 task creation 继续只使用语义化 `SchedEntity` constructor。

**Correction:** 初版中间实现把 typed numeric precedence 分散到三个 class implementation，仍要求调序时修改多个文件，且 `RunQueue::pick_next_task()` 保留独立硬编码顺序。用户指出后立即放弃该形状；最终实现必须让调序只修改 `sched/class/mod.rs` 的单一 order 定义，各 class 文件只保留 identity association。

**Validation floor:** `git diff --check`、`just fmt kernel --check`、`just build`、source audit 确认 `RunQueue::class_rank` 和无消费者 EEVDF entity accessor 已消失，class precedence 仍为 `Eevdf > RoundRobin > Idle`，default normal 仍为 RR，nice / weight 与 wake clamp diff 为空。形成 diff 后执行独立只读 review；本结构纠正不要求重复 QEMU / LTP。

### 2026-07-10 - Checkpoint 2C 后 Class-shape Feedback Correction 关闭

**Phase:** Checkpoint 2C closed 后的 implementation feedback correction，closed。

**Change:** `sched/class/mod.rs` 现在以 high-to-low `CLASS_PRECEDENCE` 集中保存唯一跨 class 顺序；每个 `Scheduler` implementation 只通过 `KIND` 关联 class identity，不保存 rank 数字。`RunQueue::pick_next_task()` 遍历该集中顺序，跨 class runnable-arrival preempt 通过 `outranks()` 查询同一顺序；`RunQueue::class_rank()` 和复制具体顺序的 rustdoc 已删除。`EevdfEntity` / `new()` 收窄到 `pub(super)`，五个无消费者字段 getter 已删除；`SchedClassPrv` 不再公开 re-export，通用 `SchedEntity::new()`、`SchedClassPrv::kind()` 和无消费者 `SchedEntity::class()` 也已收窄或删除。

**Correction history:** 初版中间实现把 typed numeric precedence 分散在 EEVDF / RR / Idle 三个 class 文件，并让 pick 保留独立顺序。用户指出调序仍需修改多个文件后，总控立即放弃该形状；最终 diff 只有 `class/mod.rs` 的 `CLASS_PRECEDENCE` 决定相对优先级，各 class 文件只关联身份，pick 与 preempt 不再维护第二份行为顺序。

**Boundary:** EEVDF arithmetic、nice-to-weight、`Task::nice()` / `set_nice()`、Kconfig、wake clamp、default normal constructor 和 syscall policy translation 均无语义改动。`SchedEntity::new_normal()` 仍返回 RR；nice 值域与 weight owner boundary 反馈明确延期。

**Validation:**

- `git diff --check` clean；source audit 未发现 `class_rank`、分散 numeric precedence、无消费者 public EEVDF entity getter、`SchedClassPrv` public re-export 或 Linux `SCHED_*` / enum discriminant coupling。
- `just build` 通过。
- `just fmt kernel --check` 仍只报告既有 generated `kconfig_defs.rs` / `platform_defs.rs` 格式漂移；本次触碰的六个 Rust 文件不在 formatter diff 中。
- `mdbook build docs` 通过。
- 未重复 QEMU / LTP；本纠正不改变调度算法、wake/default 路径或 ABI，验证 floor 为 build + source audit + review。

**Independent review:** precedence reviewer 的初审 Keter / Euclid 分别指出分散 numeric value、pick 独立顺序和 RunQueue rustdoc 复制顺序；全部按用户单一真相要求修正后，最终复审无 Apollyon / Keter / Euclid。entity visibility reviewer 最终复审同样无 Apollyon / Keter / Euclid，确认 2D 所需 sibling payload access 保持有效，nice / weight、wake clamp 和 default RR 均无 diff。唯一 Safe 是 `Eevdf::{rq_vtime, anomaly_count, last_anomaly}` 仍为无模块外消费者的 class-level observability surface；它不属于本次 `EevdfEntity` accessor 反馈，不阻塞 2D，留待后续单独整理。

**Next:** Checkpoint 2D / Gate P3。继续禁止顺带处理 nice / weight 架构或提前切换 default normal class。

### 2026-07-10 - Checkpoint 2D / Gate P3 实现启动与 Source Preflight

**Phase:** 阶段 2 - Checkpoint 2D / Gate P3，implementation in progress。

**Preflight:** 总控重新读取 RFC 四份 canonical 文档、transaction handoff、register 中 scheduler / wait 相关开放项和当前 wake / requeue source。普通 wake 只有目标 owner CPU 的 stale-safe placement 返回 `WakeEnqueueResult::Enqueued` 时调用 `RunQueue::enqueue_woken()`；`Stale`、`AlreadyCurrent` 和 `AlreadyQueued` 都在 class transaction 前返回。`ParkPending` 不立即入队，scheduler 在已 park wait 变为 runnable 的收口分支调用 `local_handoff_woken_current()`；no-switch abort 直接返回，当前没有 `local_requeue_aborted_wait_current()` caller。现有 method boundary 足以表达 Gate P3，不需要改变 wait-core contract、`WakeEnqueueResult`、IPI payload 或 scheduler entry。

**Implementation contract:** 已初始化 wake entity 的 `vruntime` 只允许被提升到 `rq_vtime - wake_window_vruntime(weight)` 的下界；已经位于该 bounded window 内或领先于 `rq_vtime` 的 entity 保持原值。clamp 后若 `vruntime >= deadline`，按现有自然续期规则从新 `vruntime` 计算 deadline。ordinary wake 在 `enqueue_woken()` 中执行该 transaction；parked current handoff 先通过唯一 `account_current(now)` 结算执行段，再执行同一个 clamp。`requeue_aborted_wait_current()`、yield、preempt、block 和 exit path 不调用 clamp。window 从 live `EEVDF_WAKE_CLAMP_US` 读取并按当前 `Task::nice()` weight 转成 virtual time；不缓存第二份 nice / weight truth。

**Write set:** 算法实现限制在 `anemone-kernel/src/sched/class/eevdf.rs`；本事务日志以及 gate 关闭时的 RFC index / tracking issue 状态同步属于既有文档工作流。`sched/class/runqueue.rs`、`sched/processor.rs` 和 `sched/mod.rs` 仅做只读 source audit；若实现要求修改这些 method boundary、wait-core、Kconfig schema、task / priority owner 或 default normal constructor，命中停止条件并先上报扩展。

**Validation floor:** `just build`；focused KUnit 覆盖 bounded clamp、领先 entity 不回退和同一 `rq_vtime` 下重复应用幂等；source audit 覆盖 ordinary `Enqueued`、parked handoff、stale、already-current、already-queued、no-switch abort 和 abort-park no-reward；形成 diff 后执行独立只读 review gate。default normal 仍为 RR，因此本 checkpoint 不把现有 LTP 结果描述成 EEVDF wake-heavy runtime 证据。

### 2026-07-10 - Checkpoint 2D / Gate P3 实现、Review 与关闭

**Phase:** 阶段 2 - Checkpoint 2D / Gate P3 closed。

**Change:** `Eevdf` 新增 class-private bounded wake clamp：window 从 live `EEVDF_WAKE_CLAMP_US` 读取并按当前 `Task::nice()` weight 转换为 virtual time，过度落后的 entity 只被提升到 `rq_vtime - wake_window` 下界，窗口内或领先 entity 不回退。clamp 后若 `vruntime >= deadline`，通过与 accounting 共用的自然续期 helper 从新 `vruntime` 计算 deadline。ordinary wake 在 `enqueue_woken()` 初始化必要 payload 后 clamp；parked current handoff 先 `account_current(now)`、clear current，再 clamp 和 enqueue。abort requeue、yield、preempt、block 与 exit path 不调用 clamp。

**Source audit:** `WakeEnqueueResult::Stale`、`AlreadyCurrent` 和 `AlreadyQueued` 在 class transaction 前返回；`Enqueued` 只在目标 owner CPU 调用 `RunQueue::enqueue_woken()` 后返回。`ParkPending` 不立即 clamp，scheduler 只在已 park current 变为 runnable 的收口分支调用 `local_handoff_woken_current()`。remote IPI 在 owner CPU 重新执行同一 stale-safe placement；若 task 已切走则走普通 `Enqueued`，不会同时走 handoff。no-switch abort 直接返回；`local_requeue_aborted_wait_current()` 当前没有 caller，且其 EEVDF transaction 只 accounting / clear / enqueue，不带 clamp 或 yield penalty。`apply_wake_clamp()` 全树只有 ordinary wake 与 parked handoff 两个调用点。`SchedEntity::new_normal()` 仍返回 RR，阶段 3 default switch 未提前发生。

**Independent review:** 两个独立只读 reviewer 均自然完成，没有修改文件。path reviewer 与 algorithm reviewer 最终都报告无 Apollyon / Keter / Euclid、无停止条件；确认 exactly-once path mapping、remote owner boundary、clamp 方向、live nice / Kconfig 消费、deadline renewal、accounting 顺序和 2C 语义保持。唯一 Safe / 非阻塞缺口是真实 `Task` / `RunQueue` EEVDF wake transaction 与 wake-heavy / wait-abort runtime smoke 尚未覆盖；default normal 仍为 RR，按 RFC 留给阶段 3。

**Validation:**

- `just build` 通过；端到端脚本内部再次构建也通过。
- `git diff --check` clean。
- `just fmt kernel --check` 只报告既有 generated `kconfig_defs.rs` / `platform_defs.rs` 格式漂移；本 checkpoint 修改的 `eevdf.rs` 已按 formatter 输出修正。
- 初次复用先前被 timeout 中止的 pretest rootfs 时，既有 openat KUnit 因残留文件触发 `AlreadyExists`；该运行不作为最终证据。按用户指示改用 `./scripts/run-user-test-rv64.sh etc/sdcard-rv.img build/eevdf-phase2d.log` 重建 rootfs，脚本退出 `0`，QEMU 完成测试并正常关机。
- 干净端到端运行共执行 107 项 KUnit，全部通过；新增两项 EEVDF KUnit 覆盖 bounded wake floor、领先 entity 不回退、同一 `rq_vtime` 下重复应用幂等、underflow 和 clamp 后 expired deadline renewal。
- read-write LTP 汇总为 `attempted=118 passed=96 failed=16 infra_failed=0 skipped=6`。这是 RR default 下的集成 sanity，不是 EEVDF wake-heavy runtime 或 fairness 证据。

**Stop-condition assessment:** 未命中 Checkpoint 2D 停止条件。实现没有扩大 wait-core、IPI、task / priority owner、Kconfig schema 或 default constructor 边界，也没有要求读取 wait-core private identity。Gate P3 的 exactly-once 边界可以关闭。

**Tracking:** `EEVDF-004` 已移入 Neutralized。阶段 1A / 1B 与 Checkpoint 2A / 2B / 2C / 2D 现已全部关闭；`EEVDF-017` 保持 active，直到阶段 3 default switch source audit 证明无 production RR 特例。

**Next:** 阶段 3 default normal class 切换与中性验证。

### 2026-07-10 - 阶段 3 前 Anomaly Error 可观测性反馈启动

**Phase:** Checkpoint 2C closed 后、阶段 3 default switch 前的 implementation feedback correction，in progress。

**Accepted feedback:** 现有 `Eevdf` 会为 no-eligible fallback 和 arithmetic saturation 更新 `anomaly_count` / `last_anomaly`，但 arithmetic anomaly 没有日志，fallback 也只在连续次数达到 threshold 时使用 `knoticeln!` 输出摘要。用户要求每次 anomaly 发生时都通过 `kerrln!` 报告；这改变 RFC 中“可选 rate-limited log”的可观测性约定，必须先回写 canonical 文本，不能只改实现。

**Write set:** `anemone-kernel/src/sched/class/eevdf.rs`；同步 existing anomaly threshold 注释的 `kconfig`、`conf/.defconfig`、`scripts/xtask/src/config/kconfig.rs` 和由 xtask 再生成的 `anemone-kernel/src/kconfig_defs.rs`；RFC `index.md`、`invariants.md`、`implementation.md`、`tracking-issues.md` 与本事务日志。此次反馈不修改 placement、accounting、wake clamp、virtual-time arithmetic、default normal constructor、scheduler method boundary 或全局 console log level。

**Implementation contract:** `record_anomaly()` 每次更新 saturating count / last reason 后立即用 `kerrln!` 输出 reason 和累计次数，因此 `NoEligibleTask` 与 `ArithmeticSaturation` 的所有现有更新点共享同一报告入口。连续 fallback 达到 live `EEVDF_ANOMALY_THRESHOLD` 时保留额外 streak 摘要，并把该摘要从 NOTICE 升为 ERR；threshold 不再决定单次 anomaly 是否报告。普通非 anomaly 调度路径不打印。若真实 EEVDF workload 让错误日志持续出现并主导 benchmark，视为公式或 arithmetic 失败，不能通过降级、限流或隐藏日志继续阶段 3。

**Validation floor:** `git diff --check`、`just fmt kernel --check`、`just build`、`mdbook build docs`，source audit 确认所有 anomaly 更新点仍集中经过 `record_anomaly()`、旧 `knoticeln!` 报告消失、default normal 仍为 RR；随后按用户既有指示直接运行 rv64 端到端脚本。当前 `console_log_level = 0` 会过滤 Err=3 的控制台输出，本反馈不擅自扩大到全局日志策略；错误记录仍进入 kernel log buffer。

### 2026-07-10 - 阶段 3 前 Anomaly Error 可观测性反馈关闭

**Phase:** Checkpoint 2C closed 后、阶段 3 default switch 前的 implementation feedback correction，closed。

**Change:** `Eevdf::record_anomaly()` 在更新 saturating `anomaly_count` 和 `last_anomaly` 后立即通过 `kerrln!` 输出 reason 与累计次数；五个 arithmetic saturation 更新点和 no-eligible fallback 由同一个 helper 报告。连续 fallback 达到 `EEVDF_ANOMALY_THRESHOLD` 时保留额外 streak 摘要，并从 `knoticeln!` 升为 `kerrln!`。Kconfig schema 未改变，threshold 注释同步为“额外 error summary”，default normal constructor 仍返回 RR。

**Source audit:** 全树搜索确认 `EevdfAnomaly::ArithmeticSaturation` 和 `NoEligibleTask` 的运行时更新仍全部汇入 `record_anomaly()`；`eevdf.rs` 中不再存在 `knoticeln!`，普通非 anomaly transaction 没有新增日志。placement、accounting、wake clamp、virtual-time arithmetic、class precedence、method boundary 和全局 console log level 均无语义改动。

**Validation:** `just build` 通过，证明新的 `kerrln!` 路径与 Kconfig 生成链可编译；`mdbook build docs` 与 `git diff --check` 通过。`just fmt kernel --check` 仍只报告既有 generated `kconfig_defs.rs` / `platform_defs.rs` whitespace 漂移，未报告本次修改的 `eevdf.rs`。rv64 端到端脚本开始重建 rootfs 后，用户明确指出本次日志级别修正不会改变测试语义并要求停止；运行已立即中止，不计为失败，也不作为本反馈的验证证据。

**Boundary:** `kerrln!` 使用 Err=3；live `console_log_level = 0` 仍会把该级别保留在 kernel log buffer 而不输出到 console。本反馈严格按用户要求修改 anomaly 报告宏，不顺带改变全局日志策略。阶段 3 仍按既有 handoff 继续 default normal class switch。

### 2026-07-10 - 阶段 3 前 Nice / Priority 边界反馈纠正启动

**Phase:** 阶段 3 default switch 前的 implementation feedback correction，in progress；不新增 Checkpoint 2E。

**Accepted feedback:** 当前 nice 以裸 `AtomicIsize` 保存，`task/api/priority.rs` 与 EEVDF 各自维护 `MIN_NICE` / `MAX_NICE`，syscall 可通过公开 raw setter 直接写 task；这使值域、writer 和未来 owner-CPU renice 退出边界不自然。用户接受第一版非事务性 renice 的短暂 accounting 偏移，不要求本轮引入 owner-CPU IPI；若 syscall topology / permission 强一致性代价过高，保持 snapshot 语义并在注释与 RFC 中明确边界，不以长锁或临时 hack 假装完整 Linux 语义。

**Implementation contract:** `Nice` newtype 唯一约束 `[-20, 19]`、ABI clamp 和 weight-table index；内部 `Nice::new()` 对非法值直接 panic，不用 `Option` 隐藏内核 bug。Task 使用受约束的原子 nice 表示，clone 只在发布前通过 `&mut Task` 继承；`Task::set_nice(Nice)` 是已发布 task 的唯一写入方法，方法注释说明 deferred-accounting 边界，并注明 owner-CPU `RunQueue` command / IPI 落地后替换直接原子写入，不为该临时路径额外打印逐次 renice 日志。`task/api/priority` 拆成 `getpriority.rs` / `setpriority.rs` 与共享 target selector；修复 Linux 明确要求且不需要扩大事务边界的语义，包括 `PRIO_PGRP` 覆盖全部线程。`RLIMIT_NICE`、user namespace / LSM、topology-wide 原子快照、立即 deadline/requeue 和完整动态 renice 仍为非目标。

**Write set:** `anemone-kernel/src/sched/mod.rs`、新建 `sched/nice.rs`、`sched/class/eevdf.rs`、`task/mod.rs`、`task/sched.rs`、`task/api/clone/mod.rs`、将 `task/api/priority.rs` 目录化为 `task/api/priority/{mod,target,getpriority,setpriority}.rs`；RFC `index.md`、`invariants.md`、`implementation.md`、`tracking-issues.md` 与本事务日志。禁止修改 `RunQueue` / processor / IPI payload、EEVDF placement/accounting formula/deadline、Kconfig 和 default normal constructor。

**Validation floor:** focused KUnit 覆盖 `Nice` 内核构造、ABI clamp boundary、table index 和 Linux weight 方向；source audit 确认裸 nice atomic、重复 range 常量、published-task raw setter、单文件 priority syscall 和 `PRIO_PGRP` leader-only 路径消失，default normal 仍为 RR。运行 `git diff --check`、`just fmt kernel --check`、`just build`、`mdbook build docs`，形成 diff 后做独立架构/ABI review。该纠正不要求单独运行 broad LTP / QEMU；阶段 3 的 nice-direction / fairness smoke 继续承担集成验证。

**Stop conditions:** 若 typed nice 需要第二份长期 weight truth，syscall 修复要求持 topology / credential lock 进入 scheduler，或正确实现被迫新增 owner-CPU IPI、远端 runqueue observation、立即 deadline/requeue 或修改 default constructor，则停止本 feedback correction 并回到 RFC review，不在现有 write set 内绕过。

### 2026-07-10 - 阶段 3 前 Nice / Priority 边界反馈纠正关闭

**Phase:** 阶段 3 default switch 前的 implementation feedback correction，closed；未新增 Checkpoint 2E。

**Change:** `sched/nice.rs` 集中定义 `Nice` 值域、Linux ABI clamp、weight-table index 和受约束的原子存储；内部 `Nice::new()` 对非法值直接断言失败。Task 持有唯一 nice 状态，已发布 task 只允许 `Task::set_nice(Nice)` 写入，clone 则在发布前通过 `&mut Task` 继承。EEVDF 删除重复 range 常量并直接消费 typed nice。`task/api/priority.rs` 拆为 `priority/{mod,target,getpriority,setpriority}.rs`；get / set syscall 分文件，shared selector 明确 snapshot 语义，`PRIO_PGRP` 覆盖选中 thread group 的全部线程，getpriority 保持 raw `20 - nice` 返回编码，setpriority 在 ABI 边界 clamp。

**Owner boundary:** sched 层拥有 `Nice` 调度属性 domain 与 weight 映射；Task 拥有每个任务的存储、读取和唯一 published-task writer；priority syscall 只负责 ABI、选择与权限。`Task::set_nice` 没有带 `compat` 的名字，没有 scheduler free-function wrapper，也不为临时非事务性边界逐次打印日志；方法注释记录 deferred-accounting 限制和 owner-CPU transaction 的替换条件。

**Source audit:** task / sched 范围内不再存在裸 `AtomicIsize` nice、重复 `MIN_NICE` / `MAX_NICE`、`kernel_setpriority`、`set_task_nice`、`swap_nice` 或方法名中的 `compat`。已发布 task 的 nice 只有 priority set syscall 调用 `Task::set_nice`；clone 是 `inherit_nice_before_publish` 的唯一 caller。`PRIO_PGRP` 使用每个 thread group 的 `get_members()`，不再只选 leader。`SchedEntity::new_normal()` 仍返回 RR，阶段 3 default switch 未提前发生。

**ABI / consistency boundary:** target collection 先取得稳定 `Arc<Task>` snapshot，再释放 topology / group lock并检查 credentials 或写 nice；membership 在 snapshot 后可以变化，本轮不承诺 topology-wide linearizability。第一版仍不实现 `RLIMIT_NICE`、user namespace / LSM、renice 时立即结算 execution segment、重算 deadline 或 owner-CPU IPI command。这些是明确接受限制，不以长锁、额外 wrapper 或日志伪装强一致性。

**Validation:** `just build` 通过，typed nice、priority module split 和 focused KUnit 均完成编译。`just fmt kernel --check` 只报告既有 generated `kconfig_defs.rs` / `platform_defs.rs` whitespace 漂移，未报告本次触碰的 Rust 文件。`mdbook build docs`、`git diff --check` 和新文件 whitespace 检查通过。focused KUnit 本轮只完成编译、未在 QEMU 中执行；按 feedback correction 的验证边界未重复 broad QEMU / LTP，阶段 3 smoke 继续承担 nice direction 与集成验证。

**Review:** 总控完成最终只读架构 / ABI 审查，未发现 Apollyon / Keter / Euclid。保留的 Safe 是 snapshot membership、缺失 `RLIMIT_NICE` / namespace / LSM，以及 renice 到 owner CPU 后续 accounting / placement 之间的短暂偏移；它们均已进入 canonical 边界，且没有改变 EEVDF formula、runqueue owner、IPI contract 或 default constructor。

**Next:** 阶段 3 default normal class 切换与中性验证。

### 2026-07-10 - 阶段 3 Default Switch 与用户态中性验证启动

**Phase:** 阶段 3 default normal class 切换与中性验证，implementation in progress。

**Approved write-set expansion:** 用户批准把阶段 3 从原 scheduler/default-initialization write set 扩展到独立 `anemone-apps/eevdf-test`、`anemone-apps/user-test/src/main.rs`、`conf/rootfs/{minimal,pretest-rv64,pretest-la64}.toml` 和对应 RFC / transaction 文档。扩张用于真实用户态黑盒 workload，不引入 scheduler debug ABI、procfs hook、test-only syscall、Kconfig policy 或 hot-path debug 日志。所有安装 `user-test` 的公共 rootfs manifest 都安装测试 app，避免共享 local-test 入口依赖未公开的 checkout-local rootfs 配置。用户随后进一步批准扩展到 `anemone-rs/src/{sys,os}/linux.rs`，用于 nice 相关 `getpriority` / `setpriority` syscall 封装；低层只转发 raw syscall，高层提供 typed selector 和 raw return 到 nice 的解码，不改变 kernel ABI、permission 或 renice owner boundary。

**Metadata correction:** 公开 `invariants.md` 的状态从遗留 `Draft` 修正为 `Canonical`，与已接受进入实现的 RFC 生命周期一致；该修正不改变 scheduler contract、不变量或阶段 gate。

**Implementation contract:** `SchedEntity::new_normal()` 是 default switch 的唯一语义翻转点；ordinary task、clone child、两种架构 bootstrap task、`kthreadd` 和 ordinary kthread 都必须继续通过该 constructor 获得 fresh EEVDF payload。idle task 继续只使用 `new_idle()`。无调用者的 directed `new_eevdf()` 在 default switch 后删除，避免两个等价 normal constructor；RR implementation 暂留给阶段 4 分类或删除，但不得保留 production entity constructor。

**User-space smoke contract:** `eevdf-test` 使用 `anemone-rs` 的 `fork`、shared anonymous mapping、`sched_yield()`、`nanosleep()`、typed priority syscall、wait 和 wall-clock syscall，在单 CPU public pretest 上运行四组定向 workload：四个 equal-nice CPU-bound worker 均有进展且最大/最小计数不超过两倍；nice 0 worker 至少达到 nice 5 worker 的 1.5 倍；周期 yield worker 与 non-yield worker 均有进展；sleep/wake worker 与 CPU-bound worker 均有进展。测试只打印 case 边界和结果摘要；稳定 workload 中出现 `EEVDF anomaly` 由用户在 live `console_log_level = 3` 下观察并按 2C / 2D 路由，不通过 debug 日志或隐藏 anomaly 完成 gate。

**Validation ownership:** agent 负责 rv64 / loongarch64 app build、当前平台 kernel build、source audit、format check、`git diff --check` 和 mdBook build；用户负责 rv64 端到端 runtime smoke。用户结果尚未提供前，runtime 项明确记录为 `user-run pending`，不伪称通过。

**Stop conditions:** 若 default flip 需要修改 placement、accounting、wake clamp、preempt decision、virtual-time arithmetic、weight visibility、wait-core、IPI 或 task topology，停止并回到对应阶段 2 / RFC review；若某类 service kthread 必须保留 RR 或特殊 class 才能启动，停止并回到 RFC review；若用户 workload 持续产生 anomaly、nice 方向错误、yield / wake task 饥饿，则分别路由回 2C / 2D，不弱化阈值或可观测性。

### 2026-07-10 - 阶段 3 Default Switch、Source Audit 与 Agent Gate 关闭

**Phase:** 阶段 3 implementation / source-build gate closed；runtime user-run pending。

**Change:** `SchedEntity::new_normal()` 现在直接创建 fresh EEVDF payload；阶段 2 的 directed `new_eevdf()` 因与 default constructor 重复且无调用者而删除。`eevdf.rs` 只同步阶段注释，placement、accounting、wake clamp、preempt decision、virtual-time arithmetic 和 class precedence 无语义 diff。RR implementation 暂留给阶段 4 分类或删除，但没有 production entity constructor。

**User-space integration:** 新增独立 release app `anemone-apps/eevdf-test`，通过 shared anonymous mapping 与 fork barrier 运行四组约两秒 workload：4 个 equal-nice CPU-bound worker 的 max/min 不超过 2；nice 0 worker 至少获得 nice 5 worker 的 1.5 倍计数；周期 yield worker 与 non-yield peer 均有进展且 peer 计数更高；sleep/wake worker 至少完成 10 轮且 CPU-bound peer 有进展。`user-test` 在 competition chroot 前执行该 app，三份安装 `user-test` 的公共 rootfs manifest 都安装 app。case 只打印边界与结果摘要，不在 hot loop 打日志。

**Anemone-rs boundary:** `anemone-rs::sys::linux::process` 新增 raw `getpriority` / `setpriority` 薄封装；`anemone-rs::os::linux::process` 新增 typed `PriorityWhich`、Linux raw `20 - nice` 返回解码和高层 `setpriority()`。测试 app 不含 raw syscall number 或 scheduler internal type。该扩张不改变 kernel priority ABI、permission、Task nice owner、renice transaction 或 EEVDF formula。

**Source audit:** ordinary clone child、rv64 / loongarch64 bootstrap task、`kthreadd` 和 ordinary kthread 全部继续调用 `SchedEntity::new_normal()`；idle task 是唯一 `new_idle()` caller；全树没有 `new_eevdf()` 或 `SchedClassPrv::RoundRobin(...)` 构造调用。wake clamp 仍只有 EEVDF `enqueue_woken()` 与 `handoff_woken_current()` 调用 `apply_wake_clamp()`；abort requeue 不调用 clamp。`account_current()` 调用集合与 `DeferredPreempt` 提前返回路径未改变。所有包含 `user-test` 的公共 rootfs manifest 均包含 `eevdf-test`，TOML 解析通过。

**Review:** 总控按 scheduler owner、single-source constructor、clone freshness、idle/RR classification、ABI containment、test lifetime 和 observability 审查最终 diff，未发现 Apollyon / Keter / Euclid。剩余验证缺口只有真实 EEVDF runtime 结果和 `anemone-rs` 无法由当前 root `just fmt <package>` 路由单独检查；后者的两架构 app build 已覆盖编译，新增 wrapper 形状人工复核符合仓库 rustfmt 风格。

**Validation:** 当前 live `kconfig` 的 `console_log_level = 3`，`just build` 通过并生成 rv64 release kernel；`just app build --arch {riscv64,loongarch64} eevdf-test` 均通过；`just app build --arch {riscv64,loongarch64} user-test` 均通过；`just fmt eevdf-test --check` 与 `just fmt user-test --check` 通过；`just fmt kernel --check` 只报告既有 generated `kconfig_defs.rs` / `platform_defs.rs` whitespace drift，本次触碰的 kernel source 不在 diff 中；`just fmt anemone-rs` 因该 standalone workspace 不是 root workspace member 而无法由当前 wrapper 路由。`git diff --check`、rootfs TOML parse 和 `mdbook build docs` 通过。未运行 QEMU、LTP 或 `eevdf-test` runtime，按用户分工标记为 `user-run pending`。

**Stop-condition assessment:** agent-side build / source audit 未命中阶段 3 停止条件；没有需要回写阶段 2 contract 或引入 RR / service-kthread 特例的实现事实。`EEVDF-017` 按 source-classification closure 条件移入 Neutralized。用户 runtime 若失败，必须按 anomaly / nice / yield / wake / service-kthread owner 路由，不能用降低阈值、隐藏日志或恢复 production RR 通过 gate。

**Next:** 用户运行 rv64 端到端 smoke 并提供 `eevdf-test` case 摘要与测试区间内的 anomaly 输出；随后进入阶段 4 收口或按失败 owner 回退。

### 2026-07-10 - 阶段 3 Source Feedback 回退 Checkpoint 2C

**Phase:** `EEVDF-022` request-completion correction，implementation in progress。

**Confirmed defect:** `account_current(now)` 在推进 current `vruntime` 后立即调用 deadline renewal，并只把 arithmetic saturation 带出 helper。`task_tick()` 随后从续期后的 entity snapshot 检查 `vruntime >= deadline`，正常非 saturation 情况下该条件已被归一化为假；runnable-arrival 的 `decide_preempt_current()` 也会在同一 accounting 中吞掉 completion。renewal 还位于短路 `||` 的末项，前序 arithmetic saturation 为真时续期副作用不会执行。现有阶段 3 smoke 通过只证明常见 workload 可由“queued peer deadline 更早”路径触发重调度，不能关闭 source-level request-boundary contract。

**RFC routing:** 该事实命中阶段 3“发现 2C accounting / preempt decision 漏闭合则停止并回到对应 checkpoint”的条件。default constructor 翻转、用户态 app、nice / wake contract 和既有 `rq_vtime` 公式保持不变；新增 active Keter `EEVDF-022`，在其 neutralized 前不进入阶段 4。`EEVDF-001` / `EEVDF-002` 的公式与单一幂等 accounting 结论不回退，但组合验收补充显式 outcome propagation。

**Implementation contract:** `eevdf.rs` 内 deadline renewal 返回正交的 `renewed` / `saturated`；`account_current(now)` 返回 class-private outcome，不新增 entity pending flag、processor-global truth、shared trait 方法或新 `ReschedCause`。tick 在 completion 且存在其它 EEVDF runnable peer 时请求 resched；runnable-arrival 在 completion 或 candidate eligible 且 deadline 更早时请求 resched；已经进入 requeue / block / exit 的 transaction 显式丢弃 completion；wake clamp renewal 只做 placement normalization。arithmetic、renewal 和 anomaly aggregation 必须显式顺序执行，不允许 effectful short-circuit。

**Write set:** `anemone-kernel/src/sched/class/eevdf.rs`；RFC `index.md`、`invariants.md`、`implementation.md`、`tracking-issues.md` 与本事务日志。该范围属于原 Checkpoint 2C write set，不修改 `Scheduler` trait、`RunQueue`、processor、wait-core、task topology、Kconfig、default constructor 或用户态 app。

**Validation floor:** focused KUnit 覆盖正常 deadline completion、无 peer decision、runnable-arrival outcome consumption、arithmetic saturation 仍执行 renewal，以及 wake normalization 与 running completion 分层；随后运行 `just fmt kernel --check`、`just build`、`git diff --check`、`mdbook build docs` 和 rv64 阶段 3 runtime smoke。最终 diff 必须由独立 subagent reviewer 审查，若发现 Apollyon / Keter / Euclid 则修正后重新 review。

### 2026-07-10 - `EEVDF-022` Request-completion Correction 关闭

**Phase:** 阶段 3 修复状态内的 Checkpoint 2C correction，closed；阶段 3 本身仍 active。

**Change:** deadline renewal 返回独立的 `renewed` / `saturated`，`account_current(now)` 返回栈上瞬时 `AccountOutcome`。tick 仅在 completion 且存在其它 EEVDF peer，或存在更早 eligible deadline 时请求 resched；runnable-arrival 在 completion 或 candidate eligible 且 deadline 更早时请求 resched。switch / requeue / block / exit transaction 显式丢弃 outcome，因为调度边界已经成立；wake normalization 只保留 arithmetic saturation，不把 placement renewal 解释为 running completion。没有新增持久 completion flag、processor-global truth、trait / `RunQueue` surface 或 `ReschedCause`。

**Testability correction:** 首轮独立 review 指出 helper-level KUnit 未锁住 production caller，定级为 Euclid。随后 `task_tick()` / `decide_preempt_current()` 分别改为经过 class-private pure decision helper，wake clamp 经过只返回 saturation 的 normalization helper；KUnit 直接覆盖 completion + peer 且 peer deadline 不早、completion + no peer、arrival completion + later-deadline candidate，以及 wake normalization 不制造 `AccountOutcome`。

**Source audit:** `account_current()` 的 tick / runnable-arrival caller 消费 outcome；yield / preempt / handoff / abort requeue、block 与 exit caller 显式丢弃。deadline renewal 在 arithmetic saturation 之后仍无条件执行，anomaly aggregation 不再利用有副作用的短路表达式。default constructor、owner CPU、`rq_vtime` formula、wake exactly-once、nice truth 与 scheduler-core pending ownership 均未改变。

**Runtime validation:** `just build` 通过。`build/eevdf-request-completion-review.log` 中 113 项 KUnit 全部通过；equal-weight `[8626, 7797, 5755, 9802]`、nice `[12815, 5682]`、bounded-yield `[4714, 12935]`、sleep/wake `[135, 15087]` 四组 workload 全部通过，`BEGIN` / `END` 区间没有 `EEVDF anomaly`。read-write LTP 最终汇总仍为 `attempted=118 passed=96 failed=16 infra_failed=0 skipped=6`，nonzero failure multiset 与修复前 `build/eevdf.log` 完全一致；该 LTP 结果只作为无回归旁证，不承担本 correction 的语义证明。

**Repository validation:** `git diff --check` 与 `mdbook build docs` 通过。`just fmt kernel --check` 只报告既有 generated `kconfig_defs.rs` / `platform_defs.rs` whitespace 漂移，未报告本次修改的 `eevdf.rs`。

**Review:** 修正首轮 Euclid 后，独立 subagent 复审确认无 Apollyon / Keter / Euclid；`EEVDF-022` 移入 Neutralized。

**Boundary:** 本条关闭只恢复阶段 3 期间暴露的 Checkpoint 2C request-completion contract。阶段 3 仍处于修复状态，本次 correction 不关闭阶段 3，也不触发阶段 4。

### 2026-07-12 - 阶段 3 Runtime Stop 与 RFC Correction Reopening

**Phase:** 阶段 3 stopped；文档层 correction protocol closed，R1 implementation 尚未开始。

**User-run evidence:** rv64 单核 QEMU TCG 的 read-write 对照在相同 118-case result multiset 下，RR profile 区间为 `56.855s` / `58.260s`，多数 EEVDF 对照为 `192.038s` 至 `201.960s`，约慢 3.3 至 3.5 倍。移除用户态 `eevdf-test`、禁用本轮标注的 set-nice、把三个 slice / window 改为 `30000us` 均未改善；单次 `500Hz` 运行也未改善。详细 factual packet 见 [Stage 3 eligibility 与整体吞吐回归证据](../../rfcs/sched-eevdf-lite/backgrounds/stage3-eligibility-regression-20260711.md)。

**Exact mechanism evidence:** 相同 signal profile 中，EEVDF / RR 均为 `attempted=74 passed=60 failed=10 infra_failed=0 skipped=4`。EEVDF probe 关联 `1,494,290` 次 explicit yield，其中 `1,339,030` 次 self-pick；`1,338,814` 次属于 min-floor `self_only_eligible`。同一 snapshot 的 weighted-FairClock counterfactual 显示 `552,494` 次已有 eligible peer，`786,320` 次仍无 eligible peer。probe 的 `invalid`、`mismatch`、`pending_overwrite`、`missing_yielding`、`missing_pick` 均为 `0`。

**Conclusion strength:** 证据已关闭“monotonic minimum-`vruntime` floor 是正确 eligibility clock”这一假设，也关闭 forced-handoff / penalty-tuning 作为根修复的方向；它尚未在 intervention 前证明该 mechanism 对全部端到端差值的因果贡献。R1 必须用单变量 actual FairClock repair 关闭后者，不能把 counterfactual count 直接写成性能预测。

**RFC routing:** `index.md` / `invariants.md` 已改为 weighted FairClock、ready / active competition membership、true leave / join exact-rational service lag、full-set arrival decision、segmentation-invariant accounting、strict deadline catch-up 和 proactive rebase。`EEVDF-001` / `EEVDF-018` / `EEVDF-004` / `EEVDF-020` 重开为 Keter；原 P2 / P3 仅保留为失效历史 gate。

**Correction order:** R1 只替换 actual FairClock，并允许带退出条件的 `legacy_placement_floor` 暂时服务旧 placement；R2 删除 bridge 并关闭 membership / lag / arrival；R3a 关闭 remainder / request catch-up；R3b 关闭 coordinate rebase。每门均有独立 hypothesis、protected invariant、minimum write set、validation floor、failure signals、write-back 和 exit，前门未退出不得启动后门。

**Branch / probe boundary:** 公共实现基线仍为 `a76a00ac`；`d0d4196f` 是 evidence-only probe commit，包含 instrumentation、profile 选择和关机汇总，不是 feature commit。后续若需要修复后计数，在独立 validation branch 复用观察语义与自校验，不整体 cherry-pick probe commit，不把 hot-path counter / dump 留在 production tree。

**Validation ownership:** 本轮只修订公开文档，未运行 build / QEMU / LTP。后续 agent 负责每门 gate 的 build、focused KUnit、source audit、diff / docs validation；用户负责 R1 instrumented signal 和最终 clean signal / read-write runtime。未提供 user-run evidence 时不能关闭对应 gate。

**Docs validation:** `git diff --check` 通过；两份新增 background 文件分别通过 `git diff --no-index --check`；`mdbook build docs` 通过，生成页面中的 Gate R1 anchor 与 tracking link 一致。本轮新增和修改段落的公开边界扫描未发现开发者私有目录、本机绝对路径或私有日志链接；历史 transaction 条目未在本次更正中重写。

**Next:** Gate R1 production correction；R1 failure signal 命中时停止并重新分类，不自动继续 R2。

### 2026-07-12 - RFC Correction 独立审查与草案校正

**Phase:** docs-only correction review closed；未开始 R1 implementation。

**Independent review findings:** 第一轮只读审查报告 3 个 Keter 与 1 个 Euclid：R1 缺 validation-only write set 和公开 signal timing 基线；canonical 文本仍把当前修复路由到失效 P2 / P3；未定义的 `R4` 与阶段 4 收口混淆；accounting 伪代码没有保存并换基解释 remainder 的 historical weight。第二轮复核确认主体修复后，继续报告 1 个 Keter 与 2 个 Euclid：R1 validation write set 误纳入旧 evidence commit 中与 signal 无关的 `pipe2_04` testcase bypass / RR constructor 注释；factual background 承载了 gate 计划；R1 同时要求 result multiset 完全相等和记录 failure-set diff。第三轮复核未发现 Apollyon / Keter，报告 3 个 Euclid：R3a / Stage 3 runtime 仍残留严格 multiset-equality 口径；P2 correction 入口遗漏 R2 的 competition membership / full-set arrival；deadline-tie keep-current 没有限定 request-completion outcome。

**Accepted corrections:** R1 production 与 validation-only write set 已分离，`groups/pipe.txt` / `class/entity.rs` 明确排除，signal `78s` / `57s` 只作为各一份公开样本；P2 / P3 只保留为 historical gate，当前顺序固定为 R1 -> R2 -> R3a -> R3b -> clean Stage 3，完整 reweight 另走独立 follow-up RFC / gate。R3a 与最终 runtime 统一为 same case set、no regression、所有 result diff 先分类，结果改善不自动失败；R2 同时纠正 2C membership / full-set arrival 与 2D leave / join / handoff；deadline tie 只约束 preferred-entity comparison，存在 peer 的 request-completion outcome 仍可独立请求重新选择。remainder historical-weight 换基、yield penalty 暂态、legacy-floor consumer / update 区分、R2 enqueue-before-accounting preflight、`W0 == 0` interior anchor 与双向 common rebase 已折回 canonical 文本。

**Review integrity boundary:** 第三轮 reviewer 违反只读合同，曾临时修改 `index.md`、`invariants.md`、`implementation.md` 后再恢复，且没有保存写前 hash。因此本条不把 reviewer 的“未保留 tracked edit”声明当作最终 delta 完整性证明；总控基于当前完整 diff 重新应用并复核 accepted corrections。按用户要求不再延长 subagent review 链，最终 delta 没有再次交给独立 reviewer。

**Final docs validation:** `git diff HEAD --check`、新增 background whitespace check、`mdbook build docs`、Gate R1 / tracking / background anchor 检查和新增内容公开边界扫描通过。未运行 kernel build、QEMU、LTP 或用户 runtime；本轮只关闭 RFC 草案校正，不关闭 R1 或 Stage 3。

**Next:** Gate R1 production correction；R1 failure signal 命中时停止并重新分类，不自动继续 R2。

### 2026-07-12 - Scheduler-core Aborted-wait Surface Correction

**Phase:** scheduler-core post-close correction；与 R1-R3b EEVDF algorithm correction 独立。

**Trigger / source proof:** `schedule_inner()` 的 already-completed wait 在 current 已为 `Runnable` 且 token 已完成时产生 `AbortWaitSleep`，随后直接返回 `DidNotSwitch`；该路径不调用 `switch_out()`、不改变 `on_runq`，也不结束 current execution segment。wait 已进入 `Parked` 收口后变为 runnable 时，production 路径只调用 `local_handoff_woken_current()`。全树没有第三条需要“无 wake reward 地重新入队”的 caller；此前保留的 `local_requeue_aborted_wait_current()` 只是未被真实状态机消费的假设 surface。

**Correction:** 删除 processor facade、`RunQueue::requeue_aborted_wait_current()`、`CurrentRequeueTransaction::AbortedWait`、`Scheduler::requeue_aborted_wait_current()`，以及 EEVDF / RR / Idle 的对应实现。公开 EEVDF canonical 文本同步删除 abort-park requeue contract；dated 历史记录保留并由本条 supersede。scheduler-core 的 no-switch abort / parked handoff 所有权、wait identity、pending-resched lifecycle 和 R1-R3b 顺序均不改变。

**Validation:** scheduler-core transaction 的 `KETER-009` closure 已记录 `just build`、`mdbook build docs`、`git diff --check` 与全树零符号审计通过；kernel format check 只命中未触碰的 generated whitespace drift。未运行 QEMU / LTP。本条只引用该证据，不把它计作 R1 进展或验收。

**Boundary:** 本 correction 不关闭或推进 R1，不改 weighted FairClock、competition membership、service lag、accounting remainder、deadline catch-up、coordinate rebase、wait-core state、IPI、trap entry 或用户 ABI。

## Open Items

- R1 weighted FairClock 是当前 active blocker；R1 exit 前不开始 R2。
- R2 / R3a / R3b 与 Stage 3 clean runtime 尚未执行。
- `EEVDF-022` 保持 Neutralized，但不能替代 strict deadline catch-up / arithmetic closure。

## Closure

事务仍在进行中。Stage 3 已停止并重开 correction protocol；R1-R3b 和 clean runtime 未完成，不得进入阶段 4。
