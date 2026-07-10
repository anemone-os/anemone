# Sched EEVDF-lite 迁移实施计划

**状态：** Active
**最后更新：** 2026-07-10
**父 RFC：** [RFC-20260622-sched-eevdf-lite](./index.md)
**不变量：** [不变量需求](./invariants.md)

本文按可提交、可验证的 gate 拆分。当前版本以 sched-split 为实现前提：scheduler core 已经拥有 scheduler-private `ScheduleMode` 和语义化 wrapper；本计划只在这些入口之后补 method-first scheduler class transaction surface、typed pending resched request、runtime accounting、EEVDF placement 和 default class 切换。实现开始后必须建立 transaction devlog；本 RFC 记录 accepted contract、计划 gate 和停止边界。

## 迁移原则

- 不重新设计 sched-split。`ScheduleMode`、token-bound `schedule_wait_sleep()`、`schedule_preempt()` deferred、stale-safe wake placement 和 wait-core `PrePark/Parked` contract 均视为下层已接受前提。
- 删除 `SchedEvent` / `on_event` 作为 accepted contract 的方向。路径语义由 `Scheduler` trait 方法名和 `RunQueue` facade 调用点表达。
- `Scheduler` trait 方法是 class-local atomic transaction：一个方法可以包含 class-private accounting、placement、penalty、clamp 和统计更新，但 scheduler core 不能拆开组合这些步骤。
- `RunQueue` / scheduler core 负责 owner CPU/noirq 事务、class dispatch、`ntasks`、`on_runq`、idle fallback 和 transaction 之间的全局线性化。
- `ScheduleMode` 只属于 scheduler core entry permission；scheduler class 不能保存、匹配或暴露 wait-core private identity。
- processor pending request 使用 `PendingResched` flags；`ReschedCause::{Tick, RunnableArrival}` 合并而不是覆盖。`PendingResched` 可按值传入 `requeue_preempted_current()`，但 restore pending request 只属于执行 `take_pending_resched()` 的 scheduler-core caller。
- 默认 normal scheduler 最终为 `Eevdf`；除 idle task 外，ordinary user task、bootstrap task 和 kthread 第一版都进入 EEVDF normal class。
- RR 只作为 transaction surface 行为保持对照、debug 或 bisect class；default switch 后不得仍是 production placement path。
- `Task::cpuid()`、owner CPU runqueue 和 `SchedEntity::on_runq()` 的所有权不变。
- `Task::nice()` 是唯一 nice truth；EEVDF entity 不保存另一份 nice，也不在第一版保存 `cached_weight`。
- clone 只能继承 nice；新 task 必须创建 fresh normal `SchedEntity`，不得复制父 task 的 EEVDF runtime state。
- EEVDF 的 `account_current(now)` 是 class-private helper。trait 只暴露 current execution accounting 的生命周期点。
- `switch.rs::switch_out()` 中现有 `Task::on_switch_out()` hook 只保留 task / CPU usage 等 context-switch bookkeeping，不作为 fair scheduler accounting truth。
- wake placement 必须复用现有 stale-safe wake 路径，不允许为公平调度绕过 wait-core revalidation。
- wake clamp exactly-once：普通 wake 只在 `WakeEnqueueResult::Enqueued` 后通过 `enqueue_woken()` 执行；`ParkPending` 后的 scheduler 收口使用 `handoff_woken_current()` 执行；no-switch abort 和 `requeue_aborted_wait_current()` 不走 wake clamp。
- `sched_yield()` 第一版使用 bounded yield penalty。
- 第一版使用线性 `Eevdf` class 和 O(n) pick/dequeue；树索引只作为后续优化 gate。
- base slice、wake clamp window、yield penalty window 和 anomaly threshold 进入 kconfig parameters；nice weight table 固定 Linux 表，不做 selector。
- agent 验证只承诺 build、source audit 和 focused smoke；用户侧 iozone、LTP、long fairness log、baseline 分析作为 implementation feedback，不作为 agent 必跑项。
- feedback 只能验证受控假设和优化路线，不能削弱目标、不变量或验收边界；probe 计划写在本文的 `Probe / Vertical Slice Gates`，执行结果进入 transaction devlog，不新建通用 feedback/probe 状态文件。

## 阶段 0：文档协议关闭与 sched-split 接缝审计

前置条件：

- 本 RFC 已经完成 method-first 方向收敛。
- `index.md` 已明确 EEVDF-lite 是 default normal scheduler 目标，而不是 iozone workaround。
- `invariants.md` 已明确 fixed owner CPU、on-runqueue 所有权、sched-split 分层、method-first transaction surface、非 deadline-only、公平记账和 wait-core 边界。

交付：

- 本文件成为 implementation canonical source。
- `tracking-issues.md` 按本轮纠偏收口：
  - `EEVDF-016` 改为 method-first transaction surface blocker，并在文档纠偏完成后 neutralized。
  - `EEVDF-018` 通过 no-switch abort、`requeue_aborted_wait_current()` 和 `handoff_woken_current()` 三分法 neutralized。
  - `EEVDF-019` 保持 neutralized，但改为 `PendingResched` flags，不再写事件映射。
  - `EEVDF-013` 保持 neutralized，但表述改为 method-first transaction surface。
- 保留真正阻塞实现顺序的 open Keter：
  - `rq_vtime` / eligibility formula。
  - EEVDF private `account_current(now)` 幂等 accounting。
  - wake placement exactly-once / parked handoff。
  - switch-in / exec_start ordering。
  - default class switch gate matrix。
  - virtual time arithmetic。
- `EEVDF-021` 通过 canonical eventual-progress 证明 neutralized：bootstrap task、`kthreadd` 和普通 kthread 第一版直接进入 normal EEVDF，不引入隐式 RR 例外；实现期 source audit 只验证分类和 fresh entity 入口。
- 明确第一版策略常量：
  - fixed Linux nice weight table。
  - Kconfig: base slice。
  - Kconfig: wake clamp window。
  - Kconfig: yield penalty window。
  - Kconfig: anomaly threshold。
- 明确验证责任分层：
  - agent-required: build、source audit、focused scheduler smoke / debug test（若低成本可用）、whitespace check。
  - user-run feedback: iozone、long fairness log、LTP/user-test profile、post-sched-split baseline 和 deferred-count / workload trace。

审计：

- 读取并确认当前路径：
  - `anemone-kernel/src/sched/mod.rs`
  - `anemone-kernel/src/sched/processor.rs`
  - `anemone-kernel/src/sched/switch.rs`
  - `anemone-kernel/src/sched/wait.rs`
  - `anemone-kernel/src/sched/class/mod.rs`
  - `anemone-kernel/src/sched/class/rr.rs`
  - `anemone-kernel/src/sched/class/idle.rs`
  - `anemone-kernel/src/task/sched.rs`
  - `anemone-kernel/src/task/mod.rs`
  - `anemone-kernel/src/task/api/priority.rs`
- 审计当前 sched-split wrapper：
  - `schedule_preempt()`
  - `schedule_wait_sleep()`
  - `schedule_runnable()`，Checkpoint 1B 应改为 yield 语义命名。
  - `schedule_idle()`
  - `schedule_zombie_never_return()`
  - `schedule_wait_with_timeout()`
- 审计所有 `local_requeue_current()` call site，确认 Checkpoint 1B 必须拆为 yield / preempt / abort-park / parked handoff 等 method-first path。
- 审计 `task_enqueue()` / `local_enqueue()` / `remote_enqueue()` 调用点，确认它们都是 new task publication；若发现非 fresh task publication，停止并重新分类。
- 审计 `wake_enqueue()` / `local_wake_enqueue()` / `remote_wake_enqueue()` 的 `WakeEnqueueResult` 分支。
- 审计所有 `SchedClassPrv::RoundRobin(())` 初始化点，区分 ordinary task、bootstrap task、kthread 和 debug/bisect 保留点。
- 审计 `Task::nice()` / `set_nice()` 写入路径，包括 `setpriority()` 和 clone inheritance；clone 只能继承 nice，不能复制父 task 的 `SchedEntity`。
- 审计 `Instant::now()` 在 scheduler noirq / tick path 的可用性；若时间读取可能分配、睡眠、触发复杂锁或重入 scheduler，停止并回到 RFC review。

反馈假设：

- 假设 EEVDF-lite 的第一版可以复用 sched-split wrapper，不需要改变 `TaskSchedState`、`WakeToken` 或 wait-core lifecycle。
- 假设 method-first transaction surface 足以表达 EEVDF 所需 accounting、placement、wake clamp、yield penalty、tick decision 和 switch-in。
- 假设线性 `Eevdf` + per-runqueue `rq_vtime` 足以表达第一版 eligibility 与 fallback。
- 假设 scheduler constants 可以通过现有 kconfig parameter 生成路径表达。
- 失败信号：method-first surface 无法覆盖某条 current switch-out / requeue path、wake clamp 必须读取 wait-core private identity、`PendingResched` flags 无法在 trap / idle / IPI / tick 路径间保持、时间读取不适合 scheduler path、或常量必须依赖运行态动态 policy；此时停止阶段，回写 `index.md` / `invariants.md` / `tracking-issues.md`。

模块边界预检：

- 文档层只允许调整本 RFC 目录、RFC 导航和必要的 workflow 索引；不得把开发者私有草稿路径写成公共 canonical 链接。
- 若 review 发现 EEVDF 需要改变 wait-core、IPI、task topology、trap entry 或 user-visible `sched_*` ABI，先回到 RFC workflow 判断是否扩大 RFC 或拆 follow-up。

write set：

- `docs/src/rfcs/sched-eevdf-lite/index.md`
- `docs/src/rfcs/sched-eevdf-lite/invariants.md`
- `docs/src/rfcs/sched-eevdf-lite/implementation.md`
- `docs/src/rfcs/sched-eevdf-lite/tracking-issues.md`
- `docs/src/rfcs.md`
- `docs/src/SUMMARY.md`

验证：

- `git diff --check`
- 若 mdBook 可用，运行 `mdbook build docs`。
- 文档 accepted contract 中不得残留 `SchedEvent` / `on_event` / `EnqueueReason` / `RequeueReason` / `SwitchOutReason`。这些词只允许出现在 rejected alternative、tracking closure 或 source audit 命令中。

退出条件：

- 本 RFC 可进入 implementation acceptance review；若接受进入实现，先建立 transaction devlog。
- `rq_vtime`、runtime accounting、wake placement、method-first transaction surface、常量配置和验证责任分层均有明确 gate，不再以“实现时再说”形式悬空。

## 阶段 1：Scheduler Trait 生命周期事务、Typed Resched 与 RR 行为保持

前置条件：

- 阶段 0 文档协议关闭。
- 若进入实现，公开 RFC 已成为 implementation canonical source，transaction devlog 已建立。

阶段 1 保持一个概念阶段，但实现必须拆成两个 checkpoint。Checkpoint 1A 先关闭 class-local trait / facade / entity split 和 RR/Idle 行为保持；Checkpoint 1B 再关闭 typed pending request、schedule entry、trap / IPI 和 switch-in source audit。Checkpoint 1A 通过后不得直接进入阶段 2；阶段 2 的前置条件是 Checkpoint 1B 也关闭。

### Checkpoint 1A：Trait / RunQueue / Entity Split 与 RR / Idle 机械适配

交付：

- 将 scheduler class trait 扩展为 method-first class-local atomic transaction surface。第一版最小 surface 包括：
  - `enqueue_new(task)`
  - `enqueue_woken(task)`
  - `dequeue(task)`
  - `requeue_yielded_current(task, now)`
  - `requeue_preempted_current(task, now, pending)`
  - `handoff_woken_current(task, now)`
  - `requeue_aborted_wait_current(task, now)`
  - `put_prev_blocked(task, now)`
  - `put_prev_exiting(task, now)`
  - `pick_next_task()`
  - `set_next_task(task, now)`
  - `task_tick(task, now) -> TickAction`
  - `decide_preempt_current(current, candidate, now) -> PreemptDecision`
- Checkpoint 1A 可以先引入 `PendingResched` / `ReschedCause` 值类型以稳定 `requeue_preempted_current(task, now, pending)` 的最终 trait shape，但不得把 processor pending state、trap tail、idle tail 或 IPI producer 在本 checkpoint 内切换到新 plumbing；这些属于 Checkpoint 1B。
- `Scheduler` trait 不提供 generic `enqueue_runnable()` 默认实现；会改变 queue membership 的 transaction 必须由每个 class 显式实现。
- `Idle` 显式实现不支持的 enqueue / requeue / dequeue transaction 为 panic / unreachable。`RunQueue` 仍应在正常路径提前拒绝 idle enqueue/requeue；Idle 的 fail-closed 是第二道防线。
- `task_tick()` 是可变生命周期 transaction，可以更新 class-local state，但不得访问 processor percpu 或直接调用 scheduler core。
- `enqueue_new()` / `enqueue_woken()` 不接收 wall-clock `now`；第一版 placement 必须由 class state、`rq_vtime` 和 bounded clamp window 表达。若实现期发现 new / wake placement 必须依赖当前时间，必须停止阶段并回到 RFC review。
- `decide_preempt_current()` 在 `enqueue_new()` / `enqueue_woken()` placement 完成后调用，不接收 New/Wake source；placement 差异必须已经体现在 candidate 的 class state 中。它可以把 current accounting 推进到 `now`，但不得 enqueue / dequeue current 或 candidate。
- RR 和 Idle 先机械适配新 trait，RR 行为保持：
  - queue order 不变。
  - tick 仍按当前 RR 行为请求 resched。
  - dequeue 找不到 task 仍暴露 bug。
- 保持现有 `local_wake_enqueue()` / `wake_enqueue()` stale-safe placement 语义，不因为新增 method-first transaction 放宽 `TaskSchedState` 检查。
- 若为了保持 Checkpoint 1A 的行为等价，需要在 scheduler owner 内保留临时兼容 wrapper，例如旧 `RunQueue::enqueue()` / `pick_next()` / `on_tick()` 形状，它们必须只转发到新 method-first transaction，不得成为新的长期 class contract。跨模块 schedule entry 与 resched producer 的泛名清理属于 Checkpoint 1B。

建议实现形状：

- `sched/class/mod.rs` 保留 `Scheduler` trait、`TickAction`、`PreemptDecision` 等 class contract。
- `sched/class/runqueue.rs` 承载 `RunQueue` facade、class dispatch、`ntasks`、`on_runq` 维护和 idle fallback。
- `sched/class/entity.rs` 承载 `SchedEntity` / `SchedClassPrv`。阶段 1 保持 RR/Idle 形状与 `Copy` 行为不变；阶段 2 再加入 EEVDF payload。
- `pick_next_task()` 不接收 `now`，也不调用 `set_next_task()`；它只负责选择并移出 class queue。`RunQueue` / scheduler core 在 pick 后显式调用 `set_next_task(task, now)`，再进入现有 `Task::on_switch_in()` / `set_current_task()` 顺序。
- Checkpoint 1A 只要求 switch-in 顺序具备 `set_next_task(task, now)` 的单一落点；bootstrap first task、idle fallback、block、yield 和 zombie 等所有入口的完整 source audit 由 Checkpoint 1B 关闭。

审计：

- 搜索 `ScheduleMode`，确认它仍在 scheduler core 私有，不被 class mod / EEVDF mod 保存或解释。
- 搜索 `SchedEvent|on_event|EnqueueReason|RequeueReason|SwitchOutReason`，确认实现代码没有 catch-all event bus。
- 审计 `sched/class/mod.rs`、`sched/class/runqueue.rs` 和 `sched/class/entity.rs` 的边界，确认拆分仍在同一 scheduler owner 内，不扩大 wait-core API、task topology 或 public scheduler policy。
- 审计 `local_sched_tick()`：只能调用 `RunQueue::task_tick()` / class tick transaction，不得从 scheduler class 内访问 current processor percpu。
- 审计 RR / Idle 适配后 queue order、tick resched 和 missing dequeue panic 行为保持。

反馈假设：

- 假设 method-first trait / `RunQueue` facade 可以在不先改变 trap/IPI pending producer 的情况下机械适配 RR / Idle。
- 假设 `PendingResched` 作为值参数可以先稳定 trait shape；processor-private pending state 的 producer / consumer 切换留到 Checkpoint 1B 后不会导致 trait 二次重写。
- 失败信号：RR / Idle 行为保持 smoke 失败、class facade 必须读取 scheduler-private `ScheduleMode`、某条 current lifecycle 无法表达为 method-first transaction、或 `PendingResched` 值参数必须反向驱动 processor state；此时停止阶段，回写 `implementation.md` / `invariants.md`。

模块边界预检：

- Checkpoint 1A 建议主动做同一 scheduler owner 内 split-only checkpoint：`runqueue.rs` 与 `entity.rs`。该拆分行为保持、边界收紧，不扩大 public API、wait-core API 或 task topology。
- 不使用 `api.rs` 这类按抽象层命名的拆分；`Scheduler` trait 作为 class 模块门面留在 `sched/class/mod.rs`。
- 若需要改变 `TaskSchedState`、wait-core helper、trap entry 或 IPI payload semantics，必须停止 1A 并转入 Checkpoint 1B 或申请 RFC scope 扩展。

write set：

- `anemone-kernel/src/sched/class/mod.rs`
- `anemone-kernel/src/sched/class/runqueue.rs`
- `anemone-kernel/src/sched/class/entity.rs`
- `anemone-kernel/src/sched/class/rr.rs`
- `anemone-kernel/src/sched/class/idle.rs`
- `anemone-kernel/src/sched/processor.rs` 仅限 `RunQueue` facade 调用点同步。
- `anemone-kernel/src/sched/mod.rs` 仅限必要的 facade 调用点同步；不得在 1A 改 trap/IPI pending plumbing。
- `anemone-kernel/src/task/sched.rs` 仅在 helper 签名需要同步时触碰。

可观测性：

- trait / facade 适配阶段不新增 hot-path 日志。
- 如果新增 assertion，优先用 release `assert!` 保护轻量 correctness invariant；昂贵队列扫描仍可用 `debug_assert!`。

验证：

- `just build`
- focused smoke：RR / Idle 行为保持，系统能启动。
- Source audit：
  - class module 不引用 scheduler-private `ScheduleMode` 作为算法状态。
  - scheduler implementation 不引入 `SchedEvent` / `on_event` / `EnqueueReason` / `RequeueReason` / `SwitchOutReason`。
  - `RunQueue` / `SchedEntity` 拆分未扩大 owner boundary。
  - 临时兼容 wrapper 若存在，只服务 1A 行为保持，并在 Checkpoint 1B 有明确删除或收口点。

退出条件：

- `Scheduler` trait、`RunQueue` facade 和 `SchedEntity` / `SchedClassPrv` 文件边界已稳定。
- RR / Idle 在新 trait 下行为保持。
- current task 的公平状态具备入队前 lifecycle transaction 位置；Checkpoint 1B 可以把 schedule entry 和 pending producer 切到这些位置，而不再重塑 class trait。

### Checkpoint 1B：Typed Pending、Schedule Entry、Trap / IPI Plumbing 与 `EEVDF-005`

交付：

- processor 的 `need_resched: bool` 升级为 `PendingResched` flags：
  - `request_resched(ReschedCause::Tick)`
  - `request_resched(ReschedCause::RunnableArrival)`
  - `take_pending_resched() -> PendingResched`
  - `restore_pending_resched(PendingResched)`
- `task_tick()` 返回 `TickAction::RequestResched` 时，processor/core 设置 `ReschedCause::Tick`。
- `decide_preempt_current()` 返回 `PreemptDecision::RequestResched` 时，processor/core 设置 `ReschedCause::RunnableArrival`。
- pending request 合并而不是 last-writer-wins；tick 和 runnable arrival 可同时存在。`DeferredPreempt` 必须让执行 `take_pending_resched()` 的 caller 恢复同一组 pending bits。
- `schedule_preempt(pending: PendingResched)` 接收 typed pending flags；current runnable 时调用 `requeue_preempted_current(task, now, pending)`，`Waiting/PrePark` 时只返回 deferred，不在 scheduler 内部恢复 pending。
- `schedule_runnable()` 改为 yield 语义命名，例如 `schedule_yield()` 或 `schedule_current_yield()`；`ScheduleMode::Runnable` 拆为 `Yield` 和 `Idle`。
- `schedule_idle()` 保持 idle 专用入口。idle task 保持 fallback singleton，不进入 `requeue_*_current()`。
- `task_enqueue()` / `local_enqueue()` / `remote_enqueue()` 命名族清理为 `enqueue_new_task` 语义，例如 `enqueue_new_task()`、`local_enqueue_new_task()`、`remote_enqueue_new_task()`。`init_routines::local_enqueue_first()` 同步改为 first/new task publication 语义命名。
- `local_requeue_current()` 泛名入口消失。若需要共享 owner/current/on_runq 检查，可保留私有 helper，但所有跨模块 call site 必须通过语义化 facade。
- no-switch abort 不调用 class transaction。
- wait sleep 进入 scheduler 前已经发现 wait round 完成时，走 no-switch abort，不调用 `requeue_aborted_wait_current()`。
- `ParkPending` 后由 scheduler 收口时调用 `handoff_woken_current()`，做 exactly-once wake clamp。
- `requeue_aborted_wait_current()` 只用于 park 已进入 requeue 收口但没有 wake reward 的 abort-park 路径；不得用于 `ParkPending` handoff。
- `Instant::now()` 由 scheduler core / `RunQueue` 在一个调度事务中读取一次，并只传入需要 current execution accounting 或 preempt decision 的 class transaction。
- local arrival 的 `decide_preempt_current()` 在本地 owner CPU/noirq placement transaction 内调用。remote new-task / wake arrival 的 placement 若通过 IPI 发生，`decide_preempt_current()` 也必须在目标 owner CPU 的 IPI/local placement transaction 内调用；source CPU 只发送请求或接收 stale-safe placement 结果，不读取目标 CPU current。
- 为保持 RR 行为可以暂时在 owner CPU placement 后保守请求 `ReschedCause::RunnableArrival`，但该路径必须被标为 RR 适配期保守策略；进入 EEVDF placement 前必须改为 placement 后的 `decide_preempt_current()` 决策，不能让无条件 remote resched 成为长期 class contract。
- switch-in 线性化顺序固定为：`pick_next_task()` 选择并移出 class queue / 清 `on_runq`，scheduler core 调用 `set_next_task(task, now)`，随后允许执行地址空间切换准备，例如当前 `switch_mapping(prev, next)`，再进入 `Task::on_switch_in()`、`set_current_task()` 和 architecture switch。`exec_start` 从 `set_next_task()` 开始计入即将运行的 execution segment；如果实现期认为 mapping 准备时间必须排除在公平执行段外，必须停下回到 RFC review。bootstrap first task、idle fallback、block、yield 和 zombie 后的 next selection 都必须走同一顺序；no-switch abort 和 deferred preempt 不得调用 `set_next_task()`。

审计：

- 搜索 `local_requeue_current(`，确认无跨模块泛名入口；yield / preempt / abort-park / parked handoff 已分流。
- 搜索 `task_enqueue(` / `local_enqueue(` / `remote_enqueue(`，确认旧泛名消失；new task publication 和 wait completion 仍走不同 placement 入口。
- 搜索 `switch_out()` 和 `Task::on_switch_out()`，确认 EEVDF 公平状态不依赖该 task hook 才更新。
- 审计所有 `mark_need_resched()` / `fetch_clear_need_resched()` 调用点，确认 tick、IPI / runnable arrival 和 deferred-preempt carry 都保留 `PendingResched` flags；remote arrival 的 placement 后 preempt decision 在 owner CPU 线性化，不能在 source CPU 比较目标 current。
- 审计 `RunQueue::pick_next_task()`：pick 后清除 `on_runq`，随后显式 `set_next_task(task, now)`；当前 `switch_mapping(prev, next)` 或等价 mapping 准备必须位于 `set_next_task()` 之后、`Task::on_switch_in()` 之前；再进入 `Task::on_switch_in()`、`set_current_task()` 和 architecture switch。未切换路径不调用 `set_next_task()`。
- 审计 `WakeEnqueueResult::{Stale, AlreadyCurrent, ParkPending, AlreadyQueued, Enqueued}`，确认只有 `Enqueued` 触发 `enqueue_woken()`，只有 parked handoff 触发 `handoff_woken_current()`。

反馈假设：

- 假设 sched-split 的 `schedule_inner()` decision 加 `PendingResched` flags 足以区分 yield、tick preempt、runnable-arrival preempt、abort-park requeue、parked handoff、block / wait park、zombie exit 和 deferred preempt。
- 假设 Checkpoint 1A 的 final trait shape 不需要因 trap/IPI producer 接入而二次重写。
- 失败信号：`PendingResched` 无法在 trap / idle / IPI / tick 路径间保持，remote arrival 必须在 source CPU 比较目标 CPU current，某条 current switch-out 路径无法映射到 method-first transaction、wait-core park path 被迫暴露 private identity 给 class、`DeferredPreempt` 被错误当成 switch-out、或 `EEVDF-005` switch-in 顺序无法 source-audit；此时停止阶段，回写 `implementation.md` / `invariants.md`。

模块边界预检：

- 若需要改变 `TaskSchedState` 或 wait-core helper，必须先申请 RFC scope 扩展。
- 若需要改变 IPI payload ABI、trap tail 所属权或 task topology，必须记录 scope 扩展理由和验证计划；不能在 Checkpoint 1B 内顺手扩大到 wait-core 或 arch context-switch 重构。

write set：

- `anemone-kernel/src/sched/class/mod.rs`
- `anemone-kernel/src/sched/class/runqueue.rs`
- `anemone-kernel/src/sched/class/entity.rs`
- `anemone-kernel/src/sched/class/rr.rs`
- `anemone-kernel/src/sched/class/idle.rs`
- `anemone-kernel/src/sched/processor.rs`
- `anemone-kernel/src/sched/mod.rs`
- `anemone-kernel/src/sched/switch.rs` 仅限必要注释或保留 task hook 边界说明。
- `anemone-kernel/src/task/sched.rs` 仅在 helper 签名需要同步时触碰。
- `anemone-kernel/src/exception/ipi.rs` 仅限 runnable-arrival resched request plumbing 和 owner CPU placement 后 preempt decision 线性化。
- `anemone-kernel/src/arch/riscv64/exception/trap/{utrap.rs,ktrap.rs}` 仅限 typed pending resched plumbing。
- `anemone-kernel/src/arch/loongarch64/exception/trap/{utrap.rs,ktrap.rs}` 仅限 typed pending resched plumbing。

可观测性：

- typed pending / entry plumbing 阶段不新增 hot-path 日志。
- 如果新增 assertion，优先用 release `assert!` 保护轻量 correctness invariant；昂贵队列扫描仍可用 `debug_assert!`。

验证：

- `just build`
- focused smoke：系统能启动，`yield_now()` / `sched_yield()` 路径不退化。
- Source audit：
  - class module 不引用 scheduler-private `ScheduleMode` 作为算法状态。
  - scheduler implementation 不引入 `SchedEvent` / `on_event` / `EnqueueReason` / `RequeueReason` / `SwitchOutReason`。
  - `PendingResched` 覆盖 tick、IPI / runnable arrival 和 caller-owned deferred-preempt restore。
  - remote runnable arrival 不在 source CPU 比较目标 current；owner CPU placement 后决策或显式 RR 保守 resched 路径可审计。
  - runnable current requeue 前存在 method-first class transaction。
  - `DeferredPreempt` 不触发 switch-out accounting。
  - wake placement transaction 不放宽 stale-safe revalidation。
  - old generic names `task_enqueue`、`local_enqueue`、`remote_enqueue`、`local_requeue_current` 不再作为跨模块入口存在。

Tracking issue 关闭审查：

- `EEVDF-005` 必须在本 checkpoint 结束时审查；只有 pick / set-next / mapping 准备 / task switch-in hook / current-task 更新顺序已写入实现并通过 source audit，且所有特殊入口不绕过 `set_next_task()` 语义时，才能移入 Neutralized。
- 若 `EEVDF-005` 不能关闭，阶段 2 不得开始；修正应回写本节或 [不变量需求](./invariants.md)，而不是把它留到阶段 4 扫尾。

退出条件：

- 所有 scheduler class 调用点都通过 method-first transaction / `RunQueue` facade。
- trap-tail resched request 调用点都传递 `PendingResched` flags；idle loop 只把非空 pending 作为 `schedule_idle()` 触发；`DeferredPreempt` 由执行 `take_pending_resched()` 并进入 `schedule_preempt(pending)` 的 caller 恢复原 pending set。
- 当前 task 的公平状态具备入队前 lifecycle transaction 位置；阶段 2 可以在这些位置接入 EEVDF `account_current(now)`、wake clamp 和 eligibility，而不改 wait-core。
- `EEVDF-005` 的 switch-in 顺序已具备 source-audit 落点：所有真正切换到 next task 的路径都经过 `set_next_task(task, now)`，no-switch abort 和 deferred preempt 不会误开新的 execution segment。

## 阶段 2：`Eevdf` Class Scaffold 与 P1/P2/P3 算法 Checkpoints

前置条件：

- Checkpoint 1A 的 method-first trait / `RunQueue` facade / `SchedEntity` split 已完成，RR / Idle 行为保持。
- Checkpoint 1B 的 typed pending、schedule entry、trap / IPI plumbing 与 `EEVDF-005` source audit 已关闭；Checkpoint 1A 单独完成不足以进入阶段 2。
- 阶段 1B 已经提供 current accounting、wake handoff、abort-park requeue、tick decision 和 placement 后 preempt decision 的 lifecycle transaction 位置。
- `EEVDF-001`、`EEVDF-002`、`EEVDF-004` 和 `EEVDF-020` 不要求在进入阶段 2 前关闭；阶段 2 的职责就是用最小 EEVDF 实现和 Gate P1/P2/P3 闭合这些问题。任一 gate 不能关闭时，阶段必须停在 default class switch 之前。

阶段 2 保持一个概念阶段，但实现必须拆成四个 checkpoint。Checkpoint 2A 只建立可编译的 payload / class scaffold；2B 关闭 Gate P1 runtime accounting；2C 关闭 Gate P2 `rq_vtime` / arithmetic / bounded yield；2D 关闭 Gate P3 wake clamp / parked handoff。阶段 3 只能在 2B、2C、2D 全部关闭后开始。

### Checkpoint 2A：Payload / Class Compile Scaffold

交付：

- 新增 `sched/class/eevdf.rs`，正式 class 名称为 `Eevdf`，与 `RoundRobin` / `Idle` 对齐。
- 将 `SchedEntity` 改为 class-specific payload，保留 `on_runq` shared truth：

```rust
struct SchedEntity {
    on_runq: bool,
    class: SchedClassPrv,
}

enum SchedClassPrv {
    Eevdf(EevdfEntity),
    RoundRobin(()),
    Idle(()),
}
```

- `SchedEntity` 不再强行保持 `Copy`。
- 新增 `SchedEntity::new_eevdf()` / `new_idle()` 或等价窄 constructor，用于构造 EEVDF class payload 与 idle payload。Checkpoint 2A 只引入可被定向测试或显式调用的 EEVDF entity constructor，不把 ordinary task、bootstrap task、kthread 和 clone child 的默认 normal constructor 切到 EEVDF；默认 normal constructor 的语义翻转属于阶段 3。
- clone child 不得再使用父 task 的 `current_task.sched_entity()`；在阶段 2 的 EEVDF 定向路径中，它必须通过 fresh EEVDF entity 初始化，在阶段 3 的默认 normal 切换中则通过 fresh normal entity 初始化。
- `EevdfEntity` 至少包含字段位置：
  - `vruntime`
  - `deadline`
  - `slice`
  - `exec_start`
  - initialized / valid 标记
  - anomaly / last fallback 诊断字段或等价统计入口
- 实现 nice-to-weight 的 compile scaffold：
  - 使用 `Task::nice()` 作为唯一 nice 真相源。
  - 不在 EEVDF entity 中长期复制 nice。
  - 不在第一版保存 `cached_weight`。
  - weight visibility 的 owner-local 语义由 Checkpoint 2C 关闭；2A 不把 `setpriority()` 语义补丁伪装成 scaffold。
- 实现第一版线性 `Eevdf` 容器骨架：
  - `Vec<Arc<Task>>` 或等价线性容器。
  - duplicate enqueue 检查。
  - O(n) dequeue。
  - O(n) pick / dequeue call shape 可编译。
  - eligibility、fallback anomaly、accounting、yield penalty 和 wake clamp 只保留字段 / helper 位置，不在 2A 声称语义闭合。
- 策略常量接入 kconfig schema / generated plumbing：
  - base slice。
  - wake clamp window。
  - yield penalty window。
  - anomaly threshold。
  - nice weight table 固定 Linux 表，不做 selector。
- 2A 只建立配置路径。base slice、yield penalty window 和 anomaly threshold 的语义消费由 2C 关闭；wake clamp window 的语义消费由 2D 关闭。

审计：

- 搜索 EEVDF entity 字段，确认 `Task::nice()` 没有被复制成第二份长期 truth。
- 搜索 `current_task.sched_entity()` / `sched_entity()` copy，确认 clone 和 new-task publication 不会复制父 task 的 EEVDF state。
- 搜索 ordinary task、bootstrap task、kthread 和 clone child 的默认 normal constructor，确认 2A 没有提前切到 EEVDF。
- 审计所有 queue membership 修改骨架，确认 `on_runq` 与 queue state 在线性化事务内一致。
- 搜索 Kconfig defs，确认策略常量不是散落在代码里的 magic number。

反馈假设：

- 假设 EEVDF class payload 和线性 queue 骨架可以在不实现算法语义的情况下编译，并通过定向 constructor 与 RR default path 隔离。
- 失败信号：2A 为了编译被迫提前切换 default normal constructor、复制父 `SchedEntity`、把 `Task::nice()` 缓存成第二真相源、或把尚未关闭的 accounting / placement / wake clamp 语义沉淀为长期代码；此时停止阶段，回写 `implementation.md` / `tracking-issues.md`。

模块边界预检：

- `eevdf.rs` 应拥有 EEVDF queue、pick、accounting 和 private helper。
- `class/mod.rs` 只做 class contract 和窄 re-export，不承载 EEVDF 算法细节。
- `SchedEntity` class-specific payload 拆分属于同一 scheduler owner 内结构维护；不得让 idle/RR 被迫理解 EEVDF 字段。
- kconfig schema 修改属于构建配置边界；必须同步 `conf/.defconfig`、live root `kconfig`、`scripts/xtask/src/config/kconfig.rs` 和 generated defs 使用点。实现不得只依赖 `.defconfig` fallback；应运行 `just defconfig` 或等价步骤让默认构建实际消费新增 scheduler constants。

write set：

- `anemone-kernel/src/sched/class/eevdf.rs`
- `anemone-kernel/src/sched/class/mod.rs`
- `anemone-kernel/src/sched/class/runqueue.rs`
- `anemone-kernel/src/sched/class/entity.rs`
- `anemone-kernel/src/sched/processor.rs`
- `anemone-kernel/src/sched/mod.rs`
- `anemone-kernel/src/task/sched.rs`
- `anemone-kernel/src/task/mod.rs` 仅在 entity 初始化或 helper 需要时触碰。
- `anemone-kernel/src/task/api/clone/mod.rs`
- `conf/.defconfig`
- `kconfig`
- `scripts/xtask/src/config/kconfig.rs`
- `anemone-kernel/src/kconfig_defs.rs` 只由 xtask 生成，不手写。

可观测性：

- 2A 不新增 hot-path runtime 日志。
- queue membership 和 owner CPU 的轻量 correctness invariant 使用 `assert!`；昂贵队列扫描可用 `debug_assert!`。

验证：

- `just build`
- Source audit：Kconfig 常量接入 live root `kconfig`，class module 不泄漏 `ScheduleMode`，clone path 不复制父 `SchedEntity`；2A 的 EEVDF 定向路径使用 fresh EEVDF entity，阶段 3 前不得把普通 default normal constructor 偷偷切到 EEVDF。

退出条件：

- `Eevdf` class scaffold 可编译，尚未必须作为默认 normal class。
- `new_eevdf()` / EEVDF 定向 constructor 已可用；`new_normal()` 或等价默认 normal constructor 尚未翻转到 EEVDF，防止阶段 2 与阶段 3 的 default switch 边界重叠。
- Kconfig schema / generated plumbing 已可由默认构建消费；各常量的算法语义仍由 2C / 2D 关闭。
- 2B 可以在不重塑 payload / trait shape 的情况下实现 `account_current(now)`。

### Checkpoint 2B：Gate P1 - `account_current(now)` 与入队前执行段结算

交付：

- 实现 EEVDF private `account_current(now)`：
  - `set_next_task()` 记录 `exec_start`。
  - `account_current(now)` 是唯一推进当前执行段的 EEVDF helper。
  - `task_tick()` 可调用 `account_current(now)`，但必须刷新 `exec_start`，避免 switch-out / requeue 双记。
  - `requeue_yielded_current()`、`requeue_preempted_current()`、`handoff_woken_current()`、`requeue_aborted_wait_current()`、`put_prev_blocked()`、`put_prev_exiting()` 通过同一个 helper 结算当前执行段。
  - `deadline` 更新基于推进后的 `vruntime`，但具体 eligibility / yield 公式由 2C 关闭。
- `DeferredPreempt` 不结束 current execution segment，不触发 `account_current(now)`。
- `switch.rs::switch_out()` 中现有 `Task::on_switch_out()` hook 继续只负责 task / CPU usage bookkeeping，不成为 fair scheduler accounting truth。

审计：

- 审计 `account_current(now)` call sites，确认同一 `delta_exec` 不会双记，`DeferredPreempt` 不会提前结算。
- 搜索 `switch_out()` 和 `Task::on_switch_out()`，确认 EEVDF 公平状态不依赖该 task hook 才更新。
- 审计 runnable requeue、parked handoff requeue、abort-park requeue、wait park switch 和 exit switch，确认 class accounting transaction 先于需要入队或切走的路径。

反馈假设：

- 假设 EEVDF runtime accounting 可以表达为一个 class-private 幂等 helper，并由 method-first transaction 在 runnable requeue、parked handoff requeue、abort-park requeue、wait park switch 或 exit switch 前调用。
- 失败信号：某个 schedule path 在 class transaction 前重新入队，`account_current(now)` 无法幂等，或 class accounting 必须从 `switch.rs::switch_out()` 中的 task hook 才能正确运行。

write set：

- `anemone-kernel/src/sched/class/eevdf.rs`
- `anemone-kernel/src/sched/class/runqueue.rs`
- `anemone-kernel/src/sched/class/mod.rs`
- `anemone-kernel/src/sched/processor.rs`
- `anemone-kernel/src/sched/mod.rs`
- `anemone-kernel/src/sched/switch.rs` 仅限必要注释或保留 task hook 边界说明。

验证：

- `just build`
- Source audit 证明 class accounting transaction 先于 runnable requeue，`DeferredPreempt` 不 accounting。
- 若有低成本 focused smoke，覆盖 tick/switch 不双记。

Tracking issue 关闭审查：

- `EEVDF-002` 必须在本 checkpoint 结束时审查；只有 `account_current(now)` 的唯一入口、调用点、幂等规则和 `exec_start` 刷新规则已由 source audit / focused smoke 证明时，才能移入 Neutralized。
- 若反馈要求改变 task hook ownership、switch-out 边界或 accounting 不变量，先回写 [不变量需求](./invariants.md) 和本节，再继续阶段 2；不得把未闭合的 accounting 边界留到 default class switch。

退出条件：

- accounting transaction ordering 与 EEVDF private helper 已证明，`EEVDF-002` 可以关闭；否则阶段 2 停止在 default class switch 之前。

### Checkpoint 2C：Gate P2 - `rq_vtime`、Arithmetic、Eligibility 与 Bounded Yield

交付：

- 实现 `rq_vtime`：
  - 公式为 monotonic min-vruntime floor：visible runnable set 包含 ready queue 和当前正在运行的 EEVDF task，`rq_vtime = max(rq_vtime, min_visible_vruntime)`；visible set 为空时保持不变。
  - enqueue / dequeue / pick / `account_current(now)` 后通过 helper 用当前 visible set 推进 `rq_vtime`。
  - current task 被 pick 出 queue 后仍参与公平时钟，但不参与 queue membership 或 pick scan。
  - runnable set 变化不得让 `rq_vtime` 回退；new task placement 使用当前 `rq_vtime`。
  - no-eligible fallback 只在 non-empty queue 中没有 eligible task 时允许，必须记录 anomaly，并把 `rq_vtime` 推进到 fallback task 的 `vruntime`。
- 实现 virtual time arithmetic：
  - `Vruntime` / `Deadline` / `rq_vtime` 长期存储为 normalized nanoseconds 的 `u64` scalar；nice 0 下 `1ns` actual runtime 对应 `1` virtual ns。
  - 不引入额外 fixed-point fractional scale；`delta_exec_ns * NICE_0_WEIGHT / weight` 和 slice/deadline 乘除用 `u128` 中间值。
  - 正 `delta_exec` 的 `delta_vruntime` 至少为 `1`。
  - overflow / 超出 `u64::MAX` 时 saturate 到 `u64::MAX` 并记录 arithmetic anomaly；不 panic，不把 `Result` 扩散到 trait / `RunQueue` surface。
- 实现 first-version eligible pick：
  - O(n) eligible pick 选择最小 deadline。
  - eligibility 使用 `task.vruntime <= rq_vtime`。
  - no eligible fallback 到最小 `vruntime`，记录 anomaly，并推进 `rq_vtime`。
  - pick 不退化为单个 deadline-only 结构。
- 实现 tick preemption decision：
  - `account_current(now)` 后，当前任务 `vruntime >= deadline` 时请求 resched。
  - 存在 eligible 且 deadline 严格早于 current deadline 的 queued runnable task 时请求 resched。
  - deadline 相等时保持 current；non-eligible task 不得只凭更早 deadline 抢占 current；否则不每 tick 强制轮转。
- 实现 `decide_preempt_current()`：
  - 在 owner CPU 的 `enqueue_new()` / `enqueue_woken()` placement 后比较 current 与 candidate。
  - 不接收 New/Wake source。
  - 先 `account_current(current, now)`；只有 candidate eligible 且 deadline 严格早于 current deadline 时返回 `PreemptDecision::RequestResched`，由 processor/core 设置 `ReschedCause::RunnableArrival`。
- 实现 new task placement：new task 无有效 `vruntime` 时通过 `enqueue_new()` 初始化为 `vruntime = rq_vtime`，并按当前 nice weight 与 base slice 计算 `deadline`。
- 实现 deadline renewal：deadline 只在初始化或 `vruntime >= deadline` 时自然续期；普通 requeue 不无条件重算 deadline。
- 保持 2C / 2D wake 边界：
  - `enqueue_woken()` 在 2C 不执行 wake clamp；未初始化 entity 只做安全初始化，已初始化 entity 保留既有 virtual-time state。
  - 真实 wake clamp / parked handoff 归属 Checkpoint 2D；2C 不消费 wake clamp window。
- 实现 bounded yield penalty：
  - `requeue_yielded_current()` 先 `account_current(now)`。
  - 只后推 deadline 到至少 `rq_vtime + yield_penalty_window_vruntime(weight)`。
  - 不改 nice / weight。
  - 不修改 `vruntime`，不把 task 推到不可恢复的最差位置。
- 实现 nice-to-weight 语义消费：
  - `setpriority()` / clone nice inheritance 后，下一次 owner CPU `account_current()` / enqueue / pick / preempt decision 读取最新 nice。
  - 已存在 deadline 不因 renice 立即重算；若当前 `setpriority()` 路径无法保证后续 owner CPU 可观察最新 nice，2C 必须在 `task/api/priority.rs` 或 owner-local helper 中补齐规则；该问题不能留到 default class switch 后再处理。
  - nice 是 task-owned weight truth 的例外，不等同 scheduler policy / class migration；2C 不得新增远端直接修改 `SchedEntity` class 或 EEVDF payload 的路径。
- 消费 2A 已接入的 Kconfig constants：
  - base slice。
  - yield penalty window。
  - anomaly threshold。
- wake clamp window 只由 2D 消费。

审计：

- 搜索 `deadline` 排序逻辑，确认 pick 不是单个 deadline-only 结构。
- 搜索 anomaly 更新点，确认 fallback 和 arithmetic saturation 不会被静默吞掉；anomaly 字段必须标注为 EEVDF-lite 本地诊断概念，不参与调度决策。
- 搜索 Kconfig defs 和生成使用点，确认 base slice、yield penalty window 和 anomaly threshold 不是散落在代码里的 magic number。
- 搜索 `nice()` / `set_nice` / `setpriority()`，确认 nice 权重方向可被 owner CPU pick/accounting 观察。

反馈假设：

- 假设线性 scan 足以在第一版表达 eligibility、deadline pick 和 vruntime fallback，无需树索引。
- 假设 nice 变化无需立即跨 CPU 重排 runqueue；下一次 owner CPU accounting / enqueue / pick 可以消费最新权重。
- 假设 runtime scheduler policy / class switch 不属于本 RFC；若未来支持，必须另走 owner CPU `RunQueue` command / IPI 事务，而不是在 2C 权重可见性补丁中顺手加入 class migration。
- 假设第一版 bounded yield penalty 可避免立即选回，同时不造成永久饥饿。
- 失败信号：fallback anomaly 在稳定 CPU-bound workload 持续增长、出现 deadline-only 行为、nice 权重方向不可见、new placement 必须读取 wall-clock `now`、yield smoke 失败、2C 偷吃 wake clamp、或 virtual time arithmetic 无法 saturating fail closed；此时停止阶段，回写 `invariants.md` / `tracking-issues.md`。

write set：

- `anemone-kernel/src/sched/class/eevdf.rs`
- `anemone-kernel/src/sched/class/runqueue.rs`
- `anemone-kernel/src/sched/class/mod.rs`
- `anemone-kernel/src/sched/processor.rs`
- `anemone-kernel/src/sched/mod.rs`
- `anemone-kernel/src/task/api/priority.rs` 仅在 weight visibility helper 或 `setpriority()` owner-local update 规则需要时触碰。
- `conf/.defconfig`
- `kconfig`
- `scripts/xtask/src/config/kconfig.rs`
- `anemone-kernel/src/kconfig_defs.rs` 只由 xtask 生成，不手写。

可观测性：

- anomaly 至少提供受限计数和 last reason，可选 rate-limited log；覆盖 no-eligible fallback 和 arithmetic saturation。稳定 CPU-bound smoke 在 warm-up 后连续观察窗口仍增长 fallback anomaly 时，必须停止 default class switch 并回写 `rq_vtime` / eligibility 公式。
- 若日志在 hot path，必须受阈值限制，不能让 benchmark 结果主要反映日志成本。

验证：

- `just build`
- focused scheduler smoke / debug test（若低成本可用）：
  - eligible pick 选择最小 deadline。
  - no eligible fallback 选择最小 vruntime 并记录 anomaly。
  - nice 权重方向影响 `vruntime` 推进。
  - bounded yield penalty 让其它 runnable task 获得运行机会，yielding task 不永久饿死。
- Source audit：无 deadline-only pick，Kconfig 常量接入 live root `kconfig`，clone path 不复制父 `SchedEntity`，阶段 3 前不得把普通 default normal constructor 偷偷切到 EEVDF。

Tracking issue 关闭审查：

- `EEVDF-001` 必须在本 checkpoint 结束时审查；只有已接受的 `rq_vtime` 公式、更新点、fallback 允许条件和 anomaly 语义由实现、source audit 和 focused smoke（若低成本可用）证明后，才能移入 Neutralized。
- `EEVDF-020` 必须与 `EEVDF-001` 同步审查；只有已接受的 virtual time 类型、单位、scale、overflow / saturating 规则和 fail-closed 行为由实现、source audit 和 focused smoke（若低成本可用）证明后，才能移入 Neutralized。
- 若公式或 arithmetic 反馈改变 fairness / eligibility contract，先回写 [RFC index](./index.md) / [不变量需求](./invariants.md)，再更新 tracking issue；不得只在 transaction devlog 中留下实现事实。

退出条件：

- formula、arithmetic、eligible pick、yield penalty 和 weight visibility 足够稳定，`EEVDF-001` / `EEVDF-020` 可以关闭；否则阶段 2 停止在 default class switch 之前。
- 线性 queue 语义正确，树索引优化不阻塞第一版。

### Checkpoint 2D：Gate P3 - Wake Clamp 与 Parked Handoff

交付：

- 实现 wake / requeue placement semantics：
  - ordinary wake 只在 `WakeEnqueueResult::Enqueued` 后通过 `enqueue_woken()` 做 wake clamp。
  - `ParkPending` 不立即 clamp；scheduler 收口 requeue 时通过 `handoff_woken_current()` 做 wake clamp。
  - `AlreadyQueued` 不二次 clamp。
  - no-switch abort 不调用 class。
  - `requeue_aborted_wait_current()` 不走 wake clamp，不套 yield penalty。
  - block / wait / zombie 不入队，只结算 current。
- 消费 2A 已接入的 wake clamp window Kconfig constant。
- 保持现有 stale-safe wake path；不得为公平调度绕过 wait-core revalidation 或读取 wait-core private identity。

审计：

- 审计 wake clamp 只在 `enqueue_woken()` 和 `handoff_woken_current()` exactly once 执行，不在 stale wake、remote precheck failure、already-current、already-queued、no-switch abort 或 `requeue_aborted_wait_current()` 上修改 EEVDF entity。
- 审计 `WakeEnqueueResult::{Stale, AlreadyCurrent, ParkPending, AlreadyQueued, Enqueued}`，确认只有 `Enqueued` 触发 `enqueue_woken()`，只有 parked handoff 触发 `handoff_woken_current()`。
- 搜索 Kconfig defs 和生成使用点，确认 wake clamp window 不是散落在代码里的 magic number。

反馈假设：

- 假设第一版不实现完整 lag decay 时，只要 `enqueue_woken()` 与 `handoff_woken_current()` 对 wake clamp 做有界、一次性处理，就足以保证 wake-heavy workload 的基本进展与公平性。
- 失败信号：同一 wake round 重复 clamp，parked handoff 漏掉 clamp，abort path 获得 wake reward，长睡眠任务 wake 后长期无进展，wake-heavy task 长期获得超出公平边界的 CPU 份额，或 wake clamp 必须读取 wait-core private placement state；此时停止阶段，回写 `implementation.md` / `tracking-issues.md`。

write set：

- `anemone-kernel/src/sched/class/eevdf.rs`
- `anemone-kernel/src/sched/class/runqueue.rs`
- `anemone-kernel/src/sched/processor.rs`
- `anemone-kernel/src/sched/mod.rs`
- `conf/.defconfig`
- `kconfig`
- `scripts/xtask/src/config/kconfig.rs`
- `anemone-kernel/src/kconfig_defs.rs` 只由 xtask 生成，不手写。

验证：

- `just build`
- Source audit 覆盖普通 wake `Enqueued`、parked handoff、no-switch abort、abort-park requeue、stale、already queued、already current。
- 若有低成本 smoke，则覆盖 wake-heavy 与 wait-abort 路径。

Tracking issue 关闭审查：

- `EEVDF-004` 必须在本 checkpoint 结束时审查；只有 `enqueue_woken()` / `handoff_woken_current()` 的 exactly-once 边界已证明，且 abort / stale / already-current / already-queued 不会获得 wake reward 时，才能移入 Neutralized。
- 若失败来自 method boundary，回写阶段 1A / 1B 和 `EEVDF-004`；若失败来自 wait-core contract 变化，停止并路由回 sched-wait 相关 RFC，而不是在 EEVDF-lite 内补兼容旁路。

退出条件：

- wake clamp 与 parked handoff 的 exactly-once 边界足够清楚，`EEVDF-004` 可以关闭；否则阶段 3 停止。
- 2B、2C、2D 全部关闭后，阶段 2 才算完成；`Eevdf` class 可独立工作，尚未作为默认 normal class。

## 阶段 3：Default Class 切换与中性验证

前置条件：

- Checkpoint 2A 的 `Eevdf` class scaffold 与 EEVDF-specific constructor 已完成，且 default normal constructor 尚未提前翻转。
- Checkpoint 2B / Gate P1、2C / Gate P2 和 2D / Gate P3 均已关闭；`Eevdf` class 已通过对应 source proof / focused smoke（若低成本可用）。
- fallback anomaly 观察面已由 2C 建立。
- `EEVDF-001`、`EEVDF-002`、`EEVDF-004`、`EEVDF-017` 和 `EEVDF-020` 的 default switch 前置条件已关闭；任一问题只剩停止条件而非已关闭时，本阶段不得切换默认 class。
- `EEVDF-021` 已由 canonical eventual-progress 证明 neutralized；阶段 3 只验证初始化分类和无 production RR 特例。
- default class 切换的写入点已经审计完毕。
- 本阶段只负责把 default normal constructor / 初始化点翻转到 EEVDF，不再新增 placement、accounting、wake clamp 或 virtual-time contract。

交付：

- 除 idle task 外，ordinary task、bootstrap task 和 kthread 的默认 normal entity 从 RR 翻转到 `Eevdf`：`SchedEntity::new_normal()` 或等价默认 normal constructor 在本阶段开始返回 fresh EEVDF entity，不保留隐式 RR 例外、特殊优先级或单独 kthread class。
- idle task 保持 `Idle` class 和 fallback singleton 模型。
- RR 保留策略明确：
  - debug / bisect 对照；或
  - 后续删除。
  RR 不再是 production normal scheduler。
- 确认 wake / requeue placement 已使用阶段 2 闭合的 EEVDF semantics，本阶段不新增 placement contract：
  - new task 无有效 `vruntime` 时通过 `enqueue_new()` 放到 `rq_vtime` 附近。
  - ordinary wake 只在 `WakeEnqueueResult::Enqueued` 后通过 `enqueue_woken()` 做 wake clamp。
  - `ParkPending` 不立即 clamp；scheduler 收口 requeue 时通过 `handoff_woken_current()` 做 wake clamp。
  - `AlreadyQueued` 不二次 clamp。
  - no-switch abort 不调用 class。
  - `requeue_aborted_wait_current()` 不走 wake clamp，不套 yield penalty。
  - yield 使用 bounded penalty。
  - block / wait / zombie 不入队，只结算 current。
- 更新注释和内部文档，移除“EEVDF 是 TODO”的过期表述，保留后续 Linux-alignment / tree-index TODO。

审计：

- 搜索 `SchedClassPrv::RoundRobin`，确认保留点都不是 ordinary / bootstrap / kthread default 初始化遗漏。
- 搜索 idle 初始化，确认 idle task 不进入 EEVDF queue。
- 审计 task creation / clone / kthread spawn / bootstrap spawn 的 entity 初始化，确认所有 default normal 初始化点都通过默认 normal constructor，而不是散落的 `SchedClassPrv::Eevdf(...)` 手写构造。
- 审计 clone path：clone child 必须在阶段 3 通过 fresh normal entity 进入 `enqueue_new()` placement，只继承 nice，不继承父 EEVDF runtime state。
- 验证 `EEVDF-021` 的 source proof：
  - bootstrap task、`kthreadd`、ordinary kthread 和普通用户 task 都通过 fresh normal entity 进入 EEVDF。
  - idle task 是唯一 production `Idle` class；RR 只允许作为 debug / bisect 对照。
  - timer worker、OOM worker、`kthreadd` 等 service kthread 不通过名称、handle 或 lifecycle state 获得 EEVDF 外的特殊 placement。
  - wait-core progress 不依赖隐藏 scheduler-critical kthread；deferred disposal、IRQ-off allocation 和 long non-preemptible path 风险按原 owner / register 路由。
- 复核 wake clamp 只在 `enqueue_woken()` 和 `handoff_woken_current()` exactly once 执行，不在 stale wake、remote precheck failure、already-current、already-queued、no-switch abort 或 `requeue_aborted_wait_current()` 上修改 EEVDF entity。
- 审计 Checkpoint 2C 的 `setpriority()` / weight visibility 证据：若目标 task 当前在 runqueue 上，必须已经证明下一次 owner CPU pick/accounting 能观察最新 weight，或 2C 已补 owner-local requeue/update 规则。若该证据缺失，本阶段停止，不能在 default switch 中临时补算法语义。

证明边界与反馈路由：

- 假设 ordinary task default 切换不需要同时实现用户可见 `sched_setscheduler()` policy 切换。
- bootstrap task 和 kthread 第一版进入同一个 fair class 是 accepted design，不是等待实现反馈决定的假设；basic boot / focused scheduler smoke 只作为 sanity validation。
- 若 source audit 或实现事实证明某类 service kthread 需要 bounded latency、emergency priority 或单独 class，停止并回到 RFC review；不得在 default switch 中保留隐式 RR 例外。
- 假设 wake clamp 可以不实现 Linux delayed dequeue / lag decay，仍能在现有 workload 下避免明显 starvation。
- 失败信号：source audit 无法证明 bootstrap/kthread 直接 normal EEVDF 的 eventual progress；发现 2B / 2C / 2D 未闭合的 placement、accounting、wake clamp、virtual-time arithmetic 或 weight visibility 问题；wake clamp 导致稳定饿死或刷分；用户侧 feedback 显示异常但无法归类。此时停止阶段，回写 `implementation.md` 或 `index.md`。

模块边界预检：

- default class 初始化分散在 arch bootstrap、kthread 和 task creation 路径。修改时只做必要替换，不顺手重构 bootstrap / kthread ownership。
- 本阶段应优先翻转 `SchedEntity::new_normal()` 或等价默认 normal constructor，并把调用点收敛到该 helper；不得在 bootstrap / kthread / clone call site 手写 EEVDF runtime 字段。
- 若需要为 kthread 引入单独 scheduler class，超出本阶段默认目标，必须回到 RFC review。

write set：

- `anemone-kernel/src/sched/class/mod.rs`
- `anemone-kernel/src/sched/class/entity.rs`
- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/task/api/clone/mod.rs`
- `anemone-kernel/src/task/kthread/kthreadd.rs`
- `anemone-kernel/src/arch/riscv64/bootstrap.rs`
- `anemone-kernel/src/arch/loongarch64/bootstrap.rs`
- `anemone-kernel/src/sched/class/runqueue.rs`、`anemone-kernel/src/sched/class/eevdf.rs`、`anemone-kernel/src/sched/processor.rs`、`anemone-kernel/src/sched/mod.rs` 仅限 source audit、注释更新或发现 2B / 2C / 2D 漏闭合时的停阶段修复；若需要修改 placement、accounting、wake clamp、preempt decision 或 virtual-time contract，必须回到阶段 2 / RFC review，不能作为阶段 3 顺手修补。

可观测性：

- default switch 后保留 anomaly 计数。
- 记录 class 分类：ordinary task、bootstrap task、kthread、idle。
- 用户侧 iozone / LTP / long fairness log 可在 transaction devlog 中标为 user-run feedback；agent 不伪称运行。

验证：

- `just build`
- basic boot / focused scheduler smoke。
- `sched_yield()` smoke。
- 多 runnable CPU-bound fairness smoke（若低成本可用）。
- nice 权重定向 smoke（若低成本可用）。
- sleep/wake fairness smoke（若低成本可用）。
- Source audit：
  - no RR default production placement。
  - `EEVDF-021` direct-normal proof：bootstrap / kthread / ordinary task 均无 production RR 特例。
  - no deadline-only pick。
  - clone/new task 不复制父 `SchedEntity`。
  - wake clamp exactly once。
  - accounting before runnable requeue / parked handoff requeue。
  - `DeferredPreempt` 不触发 switch-out accounting。

Tracking issue 关闭审查：

- `EEVDF-017` 必须在本阶段结束时审查；只有阶段 1A / 1B、Checkpoint 2A / 2B / 2C / 2D 的 gate 均已关闭，且阶段 3 source audit 证明 default switch 没有 ordinary / bootstrap / kthread production RR 特例时，才能移入 Neutralized。
- 若阶段 3 发现任何 placement、accounting、wake clamp、virtual-time arithmetic 或 weight visibility 漏闭合，停止本阶段并回到对应阶段 2 checkpoint；不得在 default switch 中顺手补算法 contract。

退出条件：

- `Eevdf` 是 normal scheduler default；idle 仍由 Idle class 兜底。
- RR 不再作为 production normal path。
- `EEVDF-021` 的 source audit 与 canonical proof 一致：kthread direct EEVDF 只承诺 eventual progress，不承诺 bounded latency。
- 中性调度验证通过或未跑项明确标为 unrun。
- 用户侧反馈若提供，已正确归类；异常不能被自动归咎或自动排除。

## 阶段 4：实现收口、限制登记和后续优化排队

前置条件：

- 阶段 3 的 default class 切换完成。
- agent-required 验证 floor 已执行，user-run 项明确区分。

交付：

- 收口 transaction devlog：
  - method-first transaction surface 证据。
  - `PendingResched` flags 证据。
  - EEVDF private `account_current(now)` 边界。
  - `rq_vtime` / eligibility 公式。
  - wake clamp / parked handoff 策略。
  - bounded yield penalty。
  - default class 切换点。
  - RR 保留点分类。
  - agent-run / user-run / unrun 验证结果。
- 更新 RFC closeout：
  - 已完成能力。
  - 未完成 Linux EEVDF 对齐项。
  - 残余风险和后续 gate。
- 必要时更新 register/current limitations：
  - 若 wait-core / IRQ-off / no-preempt path 仍解释部分 starvation 或 hang，登记到对应已有 issue，而不是写成 EEVDF limitation。
  - 若 EEVDF-lite 明确接受 Linux EEVDF 差异，登记为当前限制或 RFC 风险。
- 排队后续优化：
  - tree / dual-index queue。
  - 更完整 lag / delayed dequeue / decay。
  - latency nice。
  - 与未来 `sched_*` syscall real policy 切换的集成。
  - SMP migration / load balance 单独 RFC。

审计：

- 搜索 `RoundRobin` / `SchedClassPrv::RoundRobin`，确认每个保留点都有理由。
- 搜索 `TODO.*eevdf` / `deadline-only` / anomaly log，确认过期 TODO 已更新。
- 搜索 hot-path logs，确认 anomaly 或 debug 日志不会长期污染 benchmark。
- 对照 `tracking-issues.md`，把已关闭项移到 Neutralized，未关闭项保留为后续 gate。

反馈假设：

- 假设第一版 EEVDF-lite 可以在不引入树索引的情况下满足当前中性 fairness 验收。
- 失败信号：任务规模导致 O(n) 本身成为主要性能瓶颈，或 fallback anomaly 常态化但仍被文档接受；此时停止 closeout，回到 RFC review 或开后续 gate。

模块边界预检：

- closeout 阶段只更新 RFC / transaction / register / profile 等必要文件；不得补写 feature code。
- 若要调整 LTP/user-test profile，必须区分 agent-run 与 user-run 验证，并按用户授权触碰本地 profile。

write set：

- EEVDF RFC / transaction docs。
- `docs/src/register/current-limitations.md` 或 `docs/src/register/open-issues.md`，仅在需要公开登记时触碰。
- `anemone-apps/user-test/ltp/profile` 仅在用户授权的验证 profile 调整中触碰。

验证：

- `just build`
- `git diff --check`
- 若公开 docs / mdBook 导航改变，运行 `mdbook build docs`。
- 用户授权时运行 targeted rv64 user-test / iozone profile。

退出条件：

- 第一版 EEVDF-lite 支持矩阵、未支持矩阵和验证证据全部可追踪。
- 后续 tree index、完整 Linux EEVDF、sched syscall real policy、SMP migration 不混入第一版 closeout。

## 旁路审计清单

- `rg -n "SchedClassPrv|SchedEntity::new|RoundRobin|Idle|Eevdf|eevdf" anemone-kernel/src`
- `rg -n "ScheduleMode|schedule_preempt|schedule_wait_sleep|schedule_yield|schedule_current_yield|schedule_idle|schedule_zombie" anemone-kernel/src/sched`
- `rg -n "SchedEvent|on_event|EnqueueReason|RequeueReason|SwitchOutReason" anemone-kernel/src/sched`
- `rg -n "local_requeue_current|local_enqueue|remote_enqueue|task_enqueue|local_wake_enqueue|wake_enqueue|remote_wake_enqueue" anemone-kernel/src/sched anemone-kernel/src/task`
- `rg -n "PendingResched|ReschedCause|request_resched|take_pending_resched|restore_pending_resched|mark_need_resched|fetch_clear_need_resched" anemone-kernel/src`
- `rg -n "switch_out\\(|on_switch_out|on_switch_in|account_current|local_sched_tick" anemone-kernel/src/sched anemone-kernel/src/task`
- `rg -n "TaskSchedState|ParkState|WakeEnqueueResult|is_sched_runnable|on_runq" anemone-kernel/src/sched anemone-kernel/src/task`
- `rg -n "nice\\(|set_nice|setpriority|getpriority|kernel_setpriority" anemone-kernel/src/task anemone-kernel/src/sched`
- `rg -n "Instant::now|Duration|SYSTEM_HZ|kconfig_defs" anemone-kernel/src/sched anemone-kernel/src/time scripts/xtask/src/config conf/.defconfig`
- `rg -n "deadline|vruntime|rq_vtime|anomaly|wake_clamp|yield_penalty|base_slice" anemone-kernel/src/sched`

允许保留的旁路必须满足三点：

- 不改变 EEVDF normal default 目标。
- 有日志、计数、断言或文档说明边界。
- 有明确后续 gate 或 current limitation。

## 可观测性清单

- fallback anomaly：count、last reason、可选 rate-limited log。
- scheduler constants：base slice、wake clamp、yield penalty、weight table 来源、system HZ。
- runtime accounting：可在 debug build 或 smoke 中观察 `delta_exec`、`vruntime`、`deadline`、`exec_start` 更新。
- default class switch：ordinary task、bootstrap task、kthread、idle 的 class 分类。
- bootstrap / kthread progress proof：记录 direct normal EEVDF 分类、无 production RR 特例，以及 bounded latency 不属于本 RFC。
- wake placement：普通 `enqueue_woken()` / `handoff_woken_current()` / `requeue_aborted_wait_current()` 的 source proof 或定向计数。
- fairness smoke：每个 worker 的实际运行计数或时间份额。
- nice smoke：不同 nice task 的相对份额。
- yield smoke：yielding task 与 non-yielding task 都有进展。
- user-run feedback：iozone、LTP、long fairness log、baseline 和 deferred-count trace，必须标明为 user-run。

## 停止边界

继续追查：

- EEVDF pick 退化为 deadline-only。
- fallback anomaly 在稳定 workload 中持续增长。
- `account_current(now)` 双记或漏记。
- runnable current 在更新 `vruntime` 前入队。
- `DeferredPreempt` 被错误当作 switch-out。
- wake placement 绕过 stale-safe wait-core revalidation。
- parked handoff 未 exactly-once clamp。
- no-switch abort 或 `requeue_aborted_wait_current()` 错走 wake clamp 或 yield penalty。
- production default 仍创建 RR task。
- nice 变化长期不影响 fair accounting。
- hot-path anomaly logging 主导 benchmark 结果。

停止并记录为后续 gate：

- tree / RB-tree / dual-index queue 优化。
- Linux 完整 delayed dequeue / lag decay。
- latency nice。
- realtime / deadline / idle policy 的真实调度类。
- 用户态 `sched_*` syscall 动态策略切换。
- SMP task migration 和 load balancing。
- wait-core preempt residual、IRQ-off allocation 或长期不可抢占内核路径的独立修复。

## Probe / Vertical Slice Gates

默认不要为 probe / feedback 新建通用 `feedback.md`、`probe.md` 或 `experiments.md`。计划写在本节；执行结果写入 transaction devlog。只有证据包过长时，才在 `backgrounds/` 下增加具体命名的证据文件，并从本节链接。

阶段 2 拆分后，本节的 P1 / P2 / P3 不再只是旁路 probe；它们分别是 Checkpoint 2B / 2C / 2D 的验证 contract。若本节与阶段 2 主段出现顺序冲突，以阶段 2 主段的 checkpoint 顺序为准，并回写本节。

### Gate P1 - `account_current(now)` 与入队前执行段结算

**假设：** EEVDF runtime accounting 可以表达为一个 class-private 幂等 `account_current(now)` helper，并由 method-first transaction 在 runnable requeue、parked handoff requeue、abort-park requeue、wait park switch 或 exit switch 前调用；`switch.rs::switch_out()` 的 task hook 仍只负责 context-switch bookkeeping。

**保护目标 / 不变量：** runnable current task 不得以 stale `vruntime` / `deadline` 重新入队；tick 和 switch-out / requeue 不得重复计算同一段执行时间。

**最小 write set：** `anemone-kernel/src/sched/mod.rs`、`anemone-kernel/src/sched/processor.rs`、`anemone-kernel/src/sched/class/mod.rs`、`anemone-kernel/src/sched/class/runqueue.rs`、`anemone-kernel/src/sched/switch.rs`。

**非目标：** 不重新设计 task CPU usage accounting、wait-core state、trap entry 或 context switch assembly。

**最低验证：** `just build`；source audit 证明 class accounting transaction 先于 runnable requeue，`DeferredPreempt` 不 accounting；若有低成本 focused smoke，覆盖 tick/switch 不双记。

**失败信号：** 某个 schedule path 在 class transaction 前重新入队，`account_current(now)` 无法幂等，或 class accounting 必须从 `switch.rs::switch_out()` 中的 task hook 才能正确运行。

**回写：** 执行事实写 transaction devlog；若 hook ordering 改变 accepted invariants，更新 `invariants.md`；若 task hook ownership 改变，更新本计划和 tracking issues。

**退出：** accounting transaction ordering 与 EEVDF private helper 已证明，`EEVDF-002` 可以关闭；否则阶段 2 停止在 default class switch 之前。

**证据：** None for draft。

### Gate P2 - `rq_vtime`、eligibility 与 bounded yield

**假设：** 简化的 per-runqueue `rq_vtime` 足以在第一版 EEVDF-lite 中提供 eligibility gating，不需要 Linux 完整 lag/dequeue 模型；已接受的第一版公式是 monotonic min-vruntime floor，visible set 包含 ready queue 和 current；bounded yield penalty 可以避免 yielding task 立即选回，同时不造成 starvation。

**保护目标 / 不变量：** 短 slice task 可以获得更好响应性，但不能绕过公平份额；yielding task 给其它 runnable task 运行机会，但不能被永久惩罚。

**最小 write set：** `anemone-kernel/src/sched/class/eevdf.rs`、`anemone-kernel/src/sched/class/mod.rs`、`anemone-kernel/src/sched/class/runqueue.rs`，以及 `task/api/priority.rs`、Kconfig 生成路径和可用的 focused scheduler smoke tests / debug-only test hooks。

**非目标：** 不在本 gate 实现 delayed dequeue、lag decay、cgroups、latency nice 或 tree indexes。

**最低验证：** focused smoke 覆盖 eligible pick、no-eligible fallback、weighted vruntime progression、arithmetic saturation anomaly observation 和 yield progress。稳定 CPU-bound smoke 在 warm-up 后不应持续增长 fallback anomaly。

**失败信号：** 稳定 CPU-bound workload 持续 fallback、出现 deadline-only 行为、nice weight 方向不可见、yield 长期立即选回自身、yielding task 饥饿，或 2C 实现提前消费 2D wake clamp。

**回写：** 公式和证据写 transaction devlog；若公式改变 eligibility contract，更新 `index.md` / `invariants.md` 并 neutralize 或修订 `EEVDF-001`。

**退出：** formula 和 yield penalty 足够稳定，`EEVDF-001` / `EEVDF-020` 可以关闭；否则阶段 2 停止在 default class switch 之前。

**证据：** None for draft。

### Gate P3 - 无 lag decay 的 wake clamp 与 parked handoff

**假设：** 第一版不实现完整 lag decay 时，只要 `enqueue_woken()` 与 `handoff_woken_current()` 对 wake clamp 做有界、一次性处理，就足以保证 wake-heavy workload 的基本进展与公平性。

**保护目标 / 不变量：** wake clamp 只能通过 `enqueue_woken()` 和 `handoff_woken_current()` 两个 class-local transaction 发生；`Stale`、`AlreadyQueued`、`AlreadyCurrent`、no-switch abort 和 `requeue_aborted_wait_current()` 都不得获得 wake reward。

**最小 write set：** `anemone-kernel/src/sched/class/eevdf.rs`、`anemone-kernel/src/sched/class/runqueue.rs`、`anemone-kernel/src/sched/processor.rs`、`anemone-kernel/src/sched/mod.rs`，以及 wake clamp Kconfig 生成路径、sleep/wake smoke test 或 workload hook。

**非目标：** 不实现完整 Linux lag decay、latency nice、source-specific wake hacks 或 wait-core private placement state。

**最低验证：** source audit 覆盖普通 wake `Enqueued`、parked handoff、no-switch abort、abort-park requeue、stale、already queued、already current；若有低成本 smoke，则覆盖 wake-heavy 与 wait-abort 路径。

**失败信号：** 同一 wake round 重复 clamp；parked handoff 漏掉 clamp；abort path 获得 wake reward；长睡眠任务 wake 后长期无进展；wake-heavy task 长期获得超出公平边界的 CPU 份额。

**回写：** 若失败是方法边界错误，按 1A / 1B 归属回写阶段 1 和 `EEVDF-004`；若失败是 wake clamp 公式错误，回写 Checkpoint 2C / 2D；若失败暴露 wait-core contract 变化，停止并路由回 sched-wait 相关 RFC。

**退出：** wake clamp 与 parked handoff 的一次性边界足够清楚，可进入 default class switch；否则阶段 3 停止。

**证据：** None for draft。

## 实现期反馈记录

- 2026-06-22：文档层反馈重写；实施计划从过细 8 阶段压缩为 4 个主阶段，并把 runtime accounting position、`rq_vtime` formula、wake clamp 三个高风险点改为 probe / vertical slice gate；目标和不变量保持不变。
- 2026-07-06：sched-split 后原地 v2 重写；接受 scheduler-private wrapper 作为下层前提，并补入 runtime accounting、wake placement、bounded yield penalty、class-specific `SchedEntity` payload、Kconfig 常量边界和 agent/user 验证责任分层。
- 2026-07-07：补 typed pending resched request 设计：`need_resched` 不再作为 bool 压扁 tick / runnable-arrival source，deferred preempt 必须恢复同一组 pending bits；本轮进一步统一为 `PendingResched` flags。
- 2026-07-07：纠正 v2 草案中的 event-first 偏差；删除 `SchedEvent` / `EnqueueReason` / `RequeueReason` / `SwitchOutReason` 作为 accepted contract 的设计，改为 method-first 的 scheduler class lifecycle transaction surface。`PendingResched` 保持 processor / scheduler-core 私有 pending request；EEVDF 的 `account_current(now)` 收归为 class-private helper；wake handoff / abort wait 边界改由 `enqueue_woken()`、`handoff_woken_current()` 和 `requeue_aborted_wait_current()` 等方法名表达。
- 2026-07-07：文档层 review 后修正 gate 顺序：当时将阶段 1 整体职责收窄为 method-first surface、typed pending 和 lifecycle transaction 位置；`rq_vtime`、EEVDF private accounting、wake placement exactly-once 和 virtual-time arithmetic 由阶段 2 / Gate P1-P3 在 default switch 前关闭。同时补充 remote runnable arrival 的 owner CPU placement 后 preempt decision 线性化要求，避免 source CPU 读取目标 CPU current。
- 2026-07-08：收窄 scheduler class surface：`pick_next_task()` 不接收 `now`，`set_next_task(task, now)` 不接收 bootstrap `first` 参数，`enqueue_new()` / `enqueue_woken()` 不接收 wall-clock `now`；若实现期发现 new / wake placement 必须依赖当前时间，停止并回到 RFC review。`EEVDF-005` 接受的 switch-in 顺序固定为 pick / set-next / mapping 准备 / task switch-in hook / current publication / architecture switch，no-switch abort 和 deferred preempt 不得调用 `set_next_task()`。
- 2026-07-08：阶段边界审查后修正实施计划：阶段 2 只引入 EEVDF-specific constructor 和算法 gate，不把 default normal constructor 提前翻到 EEVDF；阶段 3 才执行 default normal switch。`task/api/priority.rs` 的 weight visibility 修复归入阶段 2 条件 write set；阶段 3 的 scheduler core / EEVDF 文件改为 audit-only 或阶段 2 漏闭合时的停止信号。同时补充 `switch_mapping(prev, next)` 相对 `set_next_task()` 的 source-audit 位置，避免 switch-in execution segment 起点含义悬空。
- 2026-07-09：阶段 1 保持一个概念阶段，但拆为 Checkpoint 1A / 1B：1A 关闭 trait / `RunQueue` / entity split 和 RR/Idle 机械适配，1B 关闭 typed pending、schedule entry、trap / IPI plumbing 与 `EEVDF-005` source audit。阶段 2 前置条件改为 1B 关闭，避免一个 gate 同时吞下所有 cross-layer plumbing。
- 2026-07-09：阶段 2 保持一个概念阶段，但拆为 Checkpoint 2A / 2B / 2C / 2D：2A 只做 payload / class compile scaffold，2B 关闭 P1 accounting，2C 关闭 P2 `rq_vtime` / arithmetic / bounded yield，2D 关闭 P3 wake clamp / parked handoff。阶段 3 前置条件改为 2B / 2C / 2D 全部关闭，避免一次性算法落地。
- 2026-07-09：Checkpoint 1B source audit 后澄清 wait abort / handoff wording：no-switch abort 不调用 class transaction，`ParkPending` 由 `handoff_woken_current()` 收口，`requeue_aborted_wait_current()` 只保留给无 wake reward 的 abort-park requeue 路径；目标、不变量、阶段顺序和 write set 不变。
- 2026-07-09：Checkpoint 1B 后反馈指出 `schedule_preempt(pending)` 内部 restore caller 传入的 `PendingResched` value 会混淆 flags value 与 processor pending slot ownership。接受 caller-owned restore：执行 `take_pending_resched()` 的 trap tail caller 在 `SchedulePreemptResult::Deferred` 时恢复原 pending set；`schedule_preempt()` 只返回 deferred，不写 processor pending state。目标、不变量和阶段顺序不变。
- 2026-07-10：Checkpoint 2C 前设计共识闭合：virtual-time state 以 normalized ns `u64` 存储、`u128` 中间计算、saturating helper 记录 anomaly；`rq_vtime` 使用 monotonic min-vruntime floor，visible set 包含 queue 和 current；eligibility 为 `vruntime <= rq_vtime`；new placement 使用 `vruntime = rq_vtime`；deadline 仅初始化或 `vruntime >= deadline` 时自然续期；tick / runnable-arrival preempt 只接受 eligible 且 deadline 更早的 candidate；yield 只后推 deadline，不改 `vruntime`；nice visibility 只保证后续 owner CPU 观察最新 `Task::nice()`；2C 不实现 wake clamp。
- 2026-07-10：Checkpoint 2C 关闭后接受 class shape 反馈：跨 class precedence 在 `sched/class/mod.rs` 以 high-to-low class order 集中保存为唯一真相，各 `Scheduler` implementation 只关联自己的 class identity；`RunQueue` 的 pick 与 preempt comparison 都消费该 class-domain order，不再保存 `class_rank()` 或独立 pick 顺序。`EevdfEntity`、`SchedClassPrv` 和通用 payload constructor 收窄到 scheduler class owner，删除没有模块外消费者的 EEVDF entity accessor。该纠正保持 `Eevdf > RoundRobin > Idle` 行为、EEVDF 算法和 ABI 不变；nice 值域、nice-to-weight 表及其 owner boundary 明确延期，不在本反馈中修改。

## Write Set 扩展记录

- 2026-07-10：用户批准 Checkpoint 2C 后 class-shape correction 扩展到 `sched/class/{mod,entity,runqueue,eevdf,rr,idle}.rs`、本 implementation feedback 和 transaction devlog。扩展只用于 class precedence metadata 与 class-private entity visibility；不触碰 task / priority owner、nice / weight 架构、wait-core、2D wake clamp 或 default normal constructor。

## 结构维护记录

- 2026-07-07：阶段 1 建议主动做同一 scheduler owner 内 split-only checkpoint：`sched/class/runqueue.rs` 承载 `RunQueue` facade，`sched/class/entity.rs` 承载 `SchedEntity` / `SchedClassPrv`，`Scheduler` trait 留在 `sched/class/mod.rs`。拆分依据是 scheduler 业务职责，而不是 `api.rs` 这类抽象层命名。
- 2026-07-09：上述 split-only checkpoint 收归为阶段 1A；trap / IPI、typed pending producer/consumer 和 schedule entry plumbing 收归为阶段 1B。
- 2026-07-10：2C 后结构反馈要求 class precedence truth 归还 scheduler class metadata，并收窄 EEVDF payload / class-private enum 的可见面；该维护不改变 lifecycle transaction surface、class 顺序或调度语义。
