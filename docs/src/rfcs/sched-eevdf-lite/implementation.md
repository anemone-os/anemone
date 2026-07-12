# Sched EEVDF-lite 迁移实施计划

**状态：** Closed - deferred after Stage 3/R1 runtime acceptance failure
**最后更新：** 2026-07-12
**父 RFC：** [RFC-20260622-sched-eevdf-lite](./index.md)
**不变量：** [不变量需求](./invariants.md)

本文保留本次迁移的历史阶段与未执行 gate。Stage 3 default switch 曾落地；R1 随后完成 weighted FairClock 公式替换，但用户运行仍观察到 `1,233,143` 次 yield self-pick 与 `1,232,735` 次 `self_only_eligible`，命中声明的 runtime failure signal。RFC 因此延期关闭，production default 已恢复为 RR，R2 / R3a / R3b 均未执行，当前没有 active gate。未来不得沿本文从 R2 直接续跑；必须先重新打开 RFC review并重新批准假设、顺序、write set、验证与停止条件。

## 迁移原则

- 不重新设计 sched-split。`ScheduleMode`、token-bound `schedule_wait_sleep()`、`schedule_preempt()` deferred、stale-safe wake placement 和 wait-core `PrePark/Parked` contract 均视为下层已接受前提。
- 删除 `SchedEvent` / `on_event` 作为 accepted contract 的方向。路径语义由 `Scheduler` trait 方法名和 `RunQueue` facade 调用点表达。
- `Scheduler` trait 方法是 class-local atomic transaction：一个方法可以包含 class-private accounting、placement、penalty、clamp 和统计更新，但 scheduler core 不能拆开组合这些步骤。
- `RunQueue` / scheduler core 负责 owner CPU/noirq 事务、class dispatch、`ntasks`、`on_runq`、idle fallback 和 transaction 之间的全局线性化。
- `ScheduleMode` 只属于 scheduler core entry permission；scheduler class 不能保存、匹配或暴露 wait-core private identity。
- processor pending request 使用 `PendingResched` flags；`ReschedCause::{Tick, RunnableArrival}` 合并而不是覆盖。`PendingResched` 可按值传入 `requeue_preempted_current()`，但 restore pending request 只属于执行 `take_pending_resched()` 的 scheduler-core caller。
- 本次迁移的历史目标是让默认 normal scheduler 最终成为 `Eevdf`；该目标未达成。当前 ordinary user task、bootstrap task 和 kthread 通过 fresh normal constructor 进入 RR。
- RR 是 Closed/deferred 期间的 production default；EEVDF 保留为实验 class。未来若重新申请 default switch，必须先满足重新批准的 runtime acceptance gates。
- `Task::cpuid()`、owner CPU runqueue 和 `SchedEntity::on_runq()` 的所有权不变。
- `Nice` newtype 统一约束 nice 值域和 weight-table index；`Task::nice()` 返回唯一 nice truth，EEVDF entity 不保存另一份 nice，也不在第一版保存 `cached_weight`。
- clone 只能继承 nice；新 task 必须创建 fresh normal `SchedEntity`，不得复制父 task 的 EEVDF runtime state。
- EEVDF 的 `account_current(now)` 是 class-private helper。trait 只暴露 current execution accounting 的生命周期点。
- `account_current(now)` 必须以 class-private outcome 保存 deadline 续期前确认的 request completion；decision transaction 立即消费，已经进入 switch 的 transaction 显式丢弃，不新增 entity flag、processor-global truth 或 shared trait 方法。
- `switch.rs::switch_out()` 中现有 `Task::on_switch_out()` hook 只保留 task / CPU usage 等 context-switch bookkeeping，不作为 fair scheduler accounting truth。
- wake placement 必须复用现有 stale-safe wake 路径，不允许为公平调度绕过 wait-core revalidation。
- competition set 为 ready queue 与 class-active current 的互斥并集；yield、preempt 和 `ParkPending` handoff 保持 continuous membership，true block / wake 才执行 leave / join。
- ordinary true wake 只在 `WakeEnqueueResult::Enqueued` 后通过 `enqueue_woken()` 消费 saved service lag；`ParkPending` 后的 scheduler 收口使用 `handoff_woken_current()` 做 active-to-ready transfer，不执行 wake reward。
- eligibility 使用 weighted FairClock，不再接受 monotonic minimum-`vruntime` floor。R1 的 `legacy_placement_floor` 只允许作为旧 placement 的临时 bridge，并在 R2 删除。
- `sched_yield()` 第一版使用 bounded yield penalty；没有 eligible peer 时允许合法 self-pick，不用 forced handoff / skip-current 修补 FairClock。
- 第一版使用线性 `Eevdf` class 和 O(n) pick/dequeue；树索引只作为后续优化 gate。
- base slice、true-sleep service-credit window、yield penalty window 和 anomaly threshold 进入 kconfig parameters；现有 wake-clamp 配置名由 R2 改义 / 迁移，nice weight table 固定 Linux 表，不做 selector。
- agent 验证承诺 build、source audit、focused KUnit / smoke 和文档检查；instrumented signal 与 clean-tree signal / read-write 对照由用户运行，作为 R1 与最终 Stage 3 的显式 gate，不再只是可选反馈。
- feedback 只能验证受控假设和优化路线，不能削弱目标、不变量或验收边界；probe 计划写在本文的 `Probe / Vertical Slice Gates`，执行结果进入 transaction devlog，不新建通用 feedback/probe 状态文件。

## 阶段 0：文档协议关闭与 sched-split 接缝审计

本节及后续阶段记录 2026-07-09 起的历史执行入口，不重写当时的 issue / gate 列表。2026-07-11 runtime feedback 已否定并撤销其中关于 `EEVDF-001` / `EEVDF-018` / `EEVDF-004` / `EEVDF-020` 的旧 closure；R1 又在 2026-07-12 命中 runtime failure。当前状态以 [Tracking Issues](./tracking-issues.md) 与 transaction closeout 为准，下面所有未执行 gate 均处于 deferred/inactive 状态。

前置条件：

- 本 RFC 已经完成 method-first 方向收敛。
- `index.md` 已明确 EEVDF-lite 是 default normal scheduler 目标，而不是 iozone workaround。
- `invariants.md` 已明确 fixed owner CPU、on-runqueue 所有权、sched-split 分层、method-first transaction surface、非 deadline-only、公平记账和 wait-core 边界。

交付：

- 本文件成为 implementation canonical source。
- `tracking-issues.md` 按本轮纠偏收口：
  - `EEVDF-016` 改为 method-first transaction surface blocker，并在文档纠偏完成后 neutralized。
  - `EEVDF-018` 通过 no-switch abort 与 `handoff_woken_current()` 的 scheduler-core 分流 neutralized。
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
  - `anemone-kernel/src/task/api/priority/`
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
- 审计 `Task::nice()` / published-task writer，包括 `setpriority()` 和 clone inheritance；clone 只能在发布前继承 nice，不能复制父 task 的 `SchedEntity`。
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

本节记录已完成的 method-first / typed-pending migration。R2 若证明原 enqueue / candidate-only preempt surface 无法线性化 full-set arrival，必须按 R2 gate 重新进入 method contract review；这不否定阶段 1 的 entry / pending ownership closure。

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
- wait sleep 进入 scheduler 前已经发现 wait round 完成时，走 no-switch abort，保持 current 继续执行且不改变 physical membership。
- 历史阶段 1B 曾在 `ParkPending` 收口时由 scheduler 调用 `handoff_woken_current()`，并把该路径视为 exactly-once wake clamp；2026-07-11 correction 已撤销这一 reward 语义，当前 R2 要求它只做 continuous active-to-ready transfer。
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

- 假设 sched-split 的 `schedule_inner()` decision 加 `PendingResched` flags 足以区分 yield、tick preempt、runnable-arrival preempt、parked handoff、block / wait park、zombie exit、no-switch abort 和 deferred preempt。
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
- 阶段 1B 已经提供 current accounting、wake handoff、tick decision 和 placement 后 preempt decision 的 lifecycle transaction 位置；no-switch abort 不进入 class transaction。
- `EEVDF-001`、`EEVDF-002`、`EEVDF-004` 和 `EEVDF-020` 不要求在进入阶段 2 前关闭；阶段 2 的职责就是用最小 EEVDF 实现和 Gate P1/P2/P3 闭合这些问题。任一 gate 不能关闭时，阶段必须停在 default class switch 之前。

阶段 2 保持一个概念阶段，但实现拆成四个 checkpoint。Checkpoint 2A 建立 payload / class scaffold；2B 关闭 Gate P1 accounting owner；2C / 2D 的原 P2 / P3 曾被关闭并允许 default switch，但 2026-07-11 runtime feedback 已撤销算法 closure。当前 2C correction 由 R1 / R2 / R3a / R3b 承担，其中 R2 同时纠正 2C 的 competition membership / full-set arrival 与 2D 的 true leave / join / handoff；这些 gate 全部关闭前，阶段 3 只能保持 stopped 状态。

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
  - `requeue_yielded_current()`、`requeue_preempted_current()`、`handoff_woken_current()`、`put_prev_blocked()`、`put_prev_exiting()` 通过同一个 helper 结算当前执行段。
  - `deadline` 更新基于推进后的 `vruntime`，但具体 eligibility / yield 公式由 2C 关闭。
- `DeferredPreempt` 不结束 current execution segment，不触发 `account_current(now)`。
- `switch.rs::switch_out()` 中现有 `Task::on_switch_out()` hook 继续只负责 task / CPU usage bookkeeping，不成为 fair scheduler accounting truth。

审计：

- 审计 `account_current(now)` call sites，确认同一 `delta_exec` 不会双记，`DeferredPreempt` 不会提前结算。
- 搜索 `switch_out()` 和 `Task::on_switch_out()`，确认 EEVDF 公平状态不依赖该 task hook 才更新。
- 审计 runnable requeue、parked handoff requeue、wait park switch 和 exit switch，确认 class accounting transaction 先于需要入队或切走的路径；no-switch abort 不结算或重建 execution segment。

反馈假设：

- 假设 EEVDF runtime accounting 可以表达为一个 class-private 幂等 helper，并由 method-first transaction 在 runnable requeue、parked handoff requeue、wait park switch 或 exit switch 前调用。
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

### Historical Checkpoint 2C：Gate P2 - `rq_vtime`、Arithmetic、Eligibility 与 Bounded Yield（已失效）

**历史状态：** 2026-07-11 runtime feedback 曾重开本 checkpoint。原 P2 的 min-floor / saturating-arithmetic closure 只保留在 transaction devlog 作为历史；R1 后续失败，R2 / R3a / R3b 未执行且已 inactive，不得按旧 P2 或旧 correction 顺序进入阶段 4。

交付：

- 实现 weighted FairClock：
  - competition set 为 `C = ready queue union class-active current`，ready / active 互斥。
  - 对固定 snapshot 聚合 `v0 = min(v_i)`、`W = sum(w_i)`、`A = sum((v_i - v0) * w_i)`，eligibility 使用 `A >= (v_i - v0) * W`。
  - FairClock aggregate 是 transaction-derived snapshot，不长期缓存；join / leave / reweight 允许改变 weighted average，不要求全局 monotonic。
  - valid non-empty positive-weight set 必有 eligible entity；no-eligible 使用 release assertion，checked aggregate invalid 才允许记录 arithmetic anomaly并 fail forward 到最小 `vruntime`。
  - R1 可以保留只服务旧 fresh / ordinary wake / `ParkPending` placement 的 `legacy_placement_floor`；eligibility / pick / yield / preempt 不得读取它，R2 必须删除。
- 实现 virtual time arithmetic：
  - `Vruntime` / `Deadline` 长期存储为 normalized nanoseconds 的 `u64` scalar；nice 0 下 `1ns` actual runtime 对应 `1` virtual ns。
  - fixed weight accounting 保存 division remainder，任意 segment split 与合并结算产生相同总 `vruntime`；不得每段强制最小推进 `1`。
  - deadline 按当前 request phase strict catch up；初始化、自然续期或没有 outstanding yield penalty 的 catch-up 完成后保持 `vruntime < deadline <= vruntime + request`，yield penalty 的有界暂态除外。
  - 乘加、FairClock 和 saved lag 使用 checked `u128` / exact-rational helper；arithmetic failure 记录 anomaly 并让 gate 失败。
  - `u64` saturation 不是最终状态；R3b 在 headroom 不足前做 common coordinate rebase。
- 实现 first-version eligible pick：
  - O(n) eligible pick 选择最小 deadline。
  - eligibility 使用 weighted FairClock；没有 request-completion outcome 时，deadline tie 的 preferred-entity 比较保持 current。
  - valid FairClock 下 current ineligible 且有 ready peer 时必须选其它 eligible entity。
  - checked aggregate invalid 才允许 fallback 到最小 `vruntime` 并记录 arithmetic anomaly。
  - pick 不退化为单个 deadline-only 结构。
- 实现 tick preemption decision：
  - `account_current(now)` 显式返回 deadline renewal 是否完成了 current request；续期后不得重新读取 `vruntime >= deadline` 反推旧状态。
  - request completed 且 EEVDF queue 中存在其它 runnable peer 时请求 resched；只有 current 时只续期，不制造空转调度。
  - 从完整 `C` 计算 preferred entity；存在其它 eligible 且 deadline 严格早于 current，或 current 已 ineligible 时请求 resched。
  - preferred-entity 比较遇到 deadline tie 时本身不请求 resched；request-completion outcome 且存在 peer 仍是独立的重新选择原因。除此之外不每 tick 强制轮转。
- 实现 `decide_preempt_current()`：
  - current accounting、new / wake placement、enqueue 与 preferred-entity decision 必须在 owner CPU 上共享同一 competition snapshot。
  - placement 后检查完整 `C`，不能只比较 current 与 new candidate；candidate 加入后既有 ready peer 也可能成为 winner。
  - 若现有 `enqueue_*()` / `decide_preempt_current()` surface 无法证明原子性，R2 先回到 method contract review，不在方法间缓存第二份 snapshot。
- 实现 new task placement：fresh entity 以 zero service lag 加入 competition set，并按当前 nice weight与 base slice 建立新 request；R1 允许暂读 `legacy_placement_floor` 隔离变量，R2 删除。
- 实现 deadline renewal：deadline 只在初始化或 `vruntime >= deadline` 时续期；renewal 返回独立的 `renewed` / arithmetic-failure 结果，所有 arithmetic 与 renewal 步骤显式顺序执行，不能把有副作用的 renewal 放入短路 `||`；R3a 改为 strict multi-request catch-up。
- 保持 2C / 2D wake 边界：
  - R1 不改旧 placement 行为；ordinary wake / `ParkPending` 暂时只读取 `legacy_placement_floor`。
  - true leave / join service lag、ParkPending continuous membership 和 full-set arrival transaction 归属 R2；R1 不提前消费。
- 实现 bounded yield penalty：
  - `requeue_yielded_current()` 先 `account_current(now)`。
  - 只后推 deadline 到至少 `ceil(V) + yield_penalty_window_vruntime(weight)`。
  - 不改 nice / weight。
  - 不修改 `vruntime` / service lag，不把 task 推到不可恢复的最差位置。
  - 不承诺存在 peer 就强制 handoff；没有 eligible peer 时 self-pick 是合法 owed-service 结果。
- 实现 nice-to-weight 语义消费：
  - `setpriority()` / clone nice inheritance 后，下一次 owner CPU `account_current()` / enqueue / pick / preempt decision 读取最新 nice。
  - 已存在 deadline 不因 renice 立即重算；若当前 `setpriority()` 路径无法保证后续 owner CPU 可观察最新 nice，2C 必须在 `task/api/priority/` 或 owner-local helper 中补齐规则；该问题不能留到 default class switch 后再处理。
  - nice 是 task-owned weight truth 的例外，不等同 scheduler policy / class migration；2C 不得新增远端直接修改 `SchedEntity` class 或 EEVDF payload 的路径。
- 消费 2A 已接入的 Kconfig constants：
  - base slice。
  - yield penalty window。
  - anomaly threshold。
- 现有 wake clamp window 在 R2 改义为 true-sleep service-credit bound；R1 只允许 legacy placement bridge 使用旧语义。

审计：

- 搜索 `deadline` 排序逻辑，确认 pick 不是单个 deadline-only 结构。
- 搜索 `account_current()`、deadline renewal、tick 与 runnable-arrival decision，确认 request completion 只通过立即返回的 class-private outcome 传播；不存在续期后重查旧 predicate、entity pending flag、processor-global 副本或 effectful short-circuit。
- 搜索 FairClock aggregate、eligibility 和 anomaly 更新点，确认 valid non-empty set 必有 eligible；checked arithmetic / representation failure 每次都通过 `kerrln!` 输出 reason / 累计次数且不会被静默吞掉。
- 搜索 Kconfig defs 和生成使用点，确认 base slice、yield penalty window 和 anomaly threshold 不是散落在代码里的 magic number。
- 搜索 `nice()` / `set_nice` / `setpriority()`，确认 nice 权重方向可被 owner CPU pick/accounting 观察。

反馈假设：

- 假设线性 scan 足以在第一版无分配地聚合 weighted FairClock 并表达 eligibility / deadline pick，无需树索引或长期 cached aggregate。
- 假设 nice 变化无需立即跨 CPU 重排 runqueue；下一次 owner CPU accounting / enqueue / pick 可以消费最新权重。
- 假设 runtime scheduler policy / class switch 不属于本 RFC；若未来支持，必须另走 owner CPU `RunQueue` command / IPI 事务，而不是在 2C 权重可见性补丁中顺手加入 class migration。
- 假设 bounded yield penalty 在正确 FairClock 上表达 preference；没有 eligible peer 时允许合法 self-pick，不承诺每次 yield 都 handoff。
- 失败信号：valid FairClock 出现 no-eligible、出现 deadline-only 行为、nice 权重方向不可见、R1 后 yield / same-task dispatch 仍在修复前数量级、任何真实 arithmetic failure、或需要用 forced handoff / penalty tuning 才能通过；此时停止当前 correction gate，回写 `invariants.md` / `tracking-issues.md` / transaction。

write set：

- `anemone-kernel/src/sched/class/eevdf.rs`
- `anemone-kernel/src/sched/class/runqueue.rs`
- `anemone-kernel/src/sched/class/mod.rs`
- `anemone-kernel/src/sched/processor.rs`
- `anemone-kernel/src/sched/mod.rs`
- `anemone-kernel/src/task/api/priority/` 仅在 weight visibility helper 或 `setpriority()` owner-local update 规则需要时触碰。
- `conf/.defconfig`
- `kconfig`
- `scripts/xtask/src/config/kconfig.rs`
- `anemone-kernel/src/kconfig_defs.rs` 只由 xtask 生成，不手写。

可观测性：

- anomaly 提供 count 和 last reason；每次 checked FairClock / arithmetic / representation failure 都通过 `kerrln!` 输出 reason 和累计次数。valid FairClock no-eligible 使用 release assertion。真实 workload 任何 arithmetic failure 都让当前 gate 失败。
- 普通调度路径不得打印该错误；如果 anomaly error 频繁到主导 benchmark，必须按算法失败停止并追查，不能通过降级、限流或隐藏日志继续 gate。

验证：

- `just build`
- focused scheduler smoke / debug test（若低成本可用）：
  - equal / unequal weight、动态 `v0` 与 eligibility boundary 的交叉乘结果正确。
  - non-empty positive-weight FairClock 至少有一个 eligible entity；invalid aggregate 与普通 no-peer 分类不混淆。
  - nice 权重方向影响 `vruntime` 推进和 FairClock。
  - current accounting 跨越旧 deadline 后，即使 peer deadline 不早于续期后的 current deadline，也能保留 request completion 并请求一次重新选择；没有 peer 时不请求空转调度。
  - runnable-arrival accounting 跨越旧 deadline 时不会吞掉 completion；arithmetic failure 不会短路 deadline renewal。
  - true-join deadline normalization 不制造 running request completion。
  - bounded yield penalty 不改变 `vruntime` / lag；eligible peer 有进展，yielding task 不永久饿死。
- Source audit：无 deadline-only / min-floor eligibility，FairClock snapshot 不长期缓存，clone path 不复制父 `SchedEntity` / remainder / lag；`legacy_placement_floor` 的行为消费只符合 R1 白名单，坐标维护 update 站点不得被用作 eligibility / pick / yield / preempt truth。
- User-run：R1 instrumented signal profile 保持 probe 守恒与 `mismatch/invalid/pending_overwrite/missing_* = 0`，并完成 new-actual / old-floor mirror check；clean tree runtime 留给 Stage 3 final gate。

Tracking issue 关闭审查：

- `EEVDF-001` 原计划只有在 R1 的 weighted FairClock 公式、actual pick、instrumented signal intervention 和 failure classification 全部有证据后才能移入 Neutralized；R1 已失败，该项保持 Keter。
- `EEVDF-022` 必须在阶段 3 反馈纠正结束时审查；只有 request completion outcome、tick / runnable-arrival 消费、switch-boundary 显式丢弃、wake normalization 分层和 saturation sequencing 均由实现、focused KUnit、source audit 与独立 review 证明后，才能移入 Neutralized。
- `EEVDF-020` 保持未解决 Keter；R2 exact-rational lag、R3a remainder / deadline catch-up 和 R3b coordinate rebase 均未执行，R1 不关闭 arithmetic issue。
- 若公式或 arithmetic 反馈改变 fairness / eligibility contract，先回写 [RFC index](./index.md) / [不变量需求](./invariants.md)，再更新 tracking issue；不得只在 transaction devlog 中留下实现事实。

退出条件：

- R1 原计划只关闭 `EEVDF-001` 的 FairClock / direct-causality 子门；R1 已失败，R2 / R3a / R3b 未执行，Checkpoint 2C correction 与阶段 3 已随 RFC 延期关闭而 inactive。
- 线性 queue 语义正确，树索引优化不阻塞第一版。

### Historical Checkpoint 2D：Gate P3 - Wake Clamp 与 Parked Handoff（已失效）

**历史状态：** 2026-07-11 membership feedback 曾重开本 checkpoint，并计划由 Gate R2 取代。R2 未执行且已 inactive。原 P3 证明了 stale-safe transaction 分流，却错误地把 `ParkPending` continuous handoff 当成 wake reward 点。

交付：

- 实现 competition membership 与 true leave / join：
  - `C = ready queue union class-active current`，两部分互斥；pick 在 class 内完成 ready-to-active transfer。
  - true block 在 final accounting 后、leave 前保存 bounded exact-rational service lag；ordinary true wake 只在 `WakeEnqueueResult::Enqueued` 后通过 `enqueue_woken()` 消费一次并 join。
  - `ParkPending` handoff、preempt 和 yield 保持 continuous membership，不保存 / 恢复 lag，不执行 wake reward。
  - `AlreadyQueued` / `AlreadyCurrent` / `Stale` 不消费 saved lag；no-switch abort 不调用 class。
  - exit 丢弃 lag；fresh 以 zero lag join。
- 将现有 wake clamp window 改义为 true-sleep maximum positive service credit，定义对应 negative debt bound；R2 删除 `legacy_placement_floor` 及旧 clamp 语义。
- 保持现有 stale-safe wake path；不得为公平调度绕过 wait-core revalidation 或读取 wait-core private identity。

审计：

- 审计 ready / active 互斥与每条 lifecycle path 的 join / leave / continuous 分类；selected entity 在 pick / set-next 之间不得离开抽象 `C`。
- 审计 `WakeEnqueueResult::{Stale, AlreadyCurrent, ParkPending, AlreadyQueued, Enqueued}`，确认只有 `Enqueued` 消费 saved lag；parked handoff 只做 active-to-ready transfer。
- 分类搜索 `legacy_placement_floor` 的行为消费与维护 update 站点，确认 R1 只有 fresh / ordinary wake / `ParkPending` 三类行为消费，且 R2 完整删除 bridge；同时搜索 Kconfig defs 和生成使用点，确认 service-lag bounds 不是散落 magic number。

反馈假设：

- 假设第一版不实现 time-based lag decay 时，bounded exact-rational service lag 足以保持 true sleep / wake 的 service debt，同时 continuous paths 不需要 placement reward。
- 失败信号：ready / active membership 出现空洞或重复；同一 true wake 重复消费 lag；`ParkPending` / abort / preempt / yield 获得 wake reward；unequal-weight round trip 越过误差合同；checked rational 表示失败；或正确 owner transaction 需要未获批 method / write-set 扩展。命中后停止 R2 并回到 RFC review。

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
- focused KUnit：ready / active 互斥；continuous requeue；raw rational lag 与 FairClock aggregate 精确相等；saved lag 等于 exact-rational clamp；unequal-weight non-integer round trip 只含声明的 placement error；`W0 == 0` 消费 saved lag 并归零；positive credit / negative debt 不越界。
- Source audit 覆盖 ordinary wake `Enqueued`、parked handoff、no-switch abort、stale、already queued、already current，以及 `legacy_placement_floor` 完整删除。
- focused runtime 覆盖 wake-heavy 与 wait-abort 路径，且不新增 arithmetic / membership anomaly。

Tracking issue 关闭审查：

- `EEVDF-018` 与 `EEVDF-004` 必须在 R2 结束时同步审查；只有 competition membership、true leave / join、ParkPending no-reward 和 saved-lag representation 都有证据时才能移入 Neutralized。
- `EEVDF-020` 只关闭 exact-rational lag / representation 子门，继续等待 R3a / R3b。
- 若失败来自 method boundary，回写阶段 1A / 1B、`EEVDF-018` 和获批 write-set；若失败来自 wait-core contract 变化，停止并路由回 sched-wait 相关 RFC，而不是在 EEVDF-lite 内补兼容旁路。

退出条件：

- ready / active membership、true leave / join 和 full-set arrival transaction 全部闭合，`EEVDF-018` / `EEVDF-004` 可以关闭；否则阶段 3 保持停止。
- R2 删除全部 `legacy_placement_floor` 旧语义；R3a / R3b 继续关闭 arithmetic，完成前阶段 2 correction 不算结束。

## 阶段 3：Default Class 切换与中性验证

**历史状态：** default constructor / source-classification 子阶段曾经落地；2026-07-11 用户 runtime 证明算法 gate 未闭合，R1 又在 2026-07-12 命中 runtime failure signal。default constructor 现已恢复 RR，阶段 3 不再等待自动恢复，且不会进入阶段 4。本节只保留历史目标与未来重新 review 时可参考的验证清单。

历史前置条件：

- Checkpoint 2A scaffold、Checkpoint 2B accounting owner、default constructor flip 和 `EEVDF-017` source-classification closure 曾完成；R1 failure closeout 已明确恢复 production RR。
- 原 P2 / P3 closure 已被 runtime evidence 撤销；`EEVDF-001`、`EEVDF-018`、`EEVDF-004`、`EEVDF-020` 当前为 Closed RFC 中未解决的 Keter。
- R1 先关闭 weighted FairClock 公式与 direct-causality intervention；R1 已失败并停止，未自动继续 R2。
- R2 关闭 ready / active membership、true leave / join service lag 和 full-set arrival transaction；R3a / R3b 再关闭 accounting / request 与 coordinate arithmetic。
- correction runtime / arithmetic anomaly 观察面必须在每门 gate 中成立；不得靠隐藏日志、降低阈值或保留旧 floor 通过。
- `EEVDF-021` 的 historical canonical eventual-progress 决策保持 Neutralized，但不再描述当前 RR 分类。
- R2 / R3a / R3b 未执行；RFC 已延期关闭，不进入阶段 4。

历史交付目标：

- 除 idle task 外，ordinary task、bootstrap task 和 kthread 的默认 normal entity 从 RR 翻转到 `Eevdf`：`SchedEntity::new_normal()` 或等价默认 normal constructor 在本阶段开始返回 fresh EEVDF entity，不保留隐式 RR 例外、特殊优先级或单独 kthread class。
- idle task 保持 `Idle` class 和 fallback singleton 模型。
- RR 保留策略明确：
  - debug / bisect 对照；或
  - 后续删除。
  RR 不再是 production normal scheduler。
- correction 后确认 wake / requeue placement 使用修订后的 EEVDF semantics：
  - fresh task 以 zero service lag join。
  - ordinary true wake 只在 `WakeEnqueueResult::Enqueued` 后消费 saved lag。
  - `ParkPending` handoff 保持 continuous membership，不执行 wake reward。
  - `AlreadyQueued` / `AlreadyCurrent` / `Stale` 不消费 saved lag。
  - no-switch abort 不调用 class。
  - yield 使用 weighted FairClock 上的 bounded penalty。
  - true block 保存 lag，exit 丢弃调度状态。
- 更新注释和内部文档，移除“EEVDF 是 TODO”的过期表述，保留后续 Linux-alignment / tree-index TODO。
- 新增独立用户态 `eevdf-test` app，通过公开 syscall 和共享用户内存黑盒覆盖 equal-weight progress、nice 权重方向、bounded yield 和 sleep/wake progress；不为测试新增 scheduler debug ABI、procfs hook、test-only syscall 或 hot-path debug 日志。
- `user-test` 在进入 competition root 前执行 `eevdf-test`；所有安装 `user-test` 的公共 rootfs manifest 同步安装该 app，避免共享测试入口依赖 checkout-local rootfs 配置。

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
- 复核只有 true wake `enqueue_woken()` 消费 saved lag；`handoff_woken_current()`、stale wake、remote precheck failure、already-current、already-queued 和 no-switch abort 都不执行 true join placement。
- 审计 Checkpoint 2C 的 `setpriority()` / weight visibility 证据：若目标 task 当前在 runqueue 上，必须已经证明下一次 owner CPU pick/accounting 能观察最新 weight，或 2C 已补 owner-local requeue/update 规则。若该证据缺失，本阶段停止，不能在 default switch 中临时补算法语义。

证明边界与反馈路由：

- 假设 ordinary task default 切换不需要同时实现用户可见 `sched_setscheduler()` policy 切换。
- bootstrap task 和 kthread 第一版进入同一个 fair class 是 accepted design，不是等待实现反馈决定的假设；basic boot / focused scheduler smoke 只作为 sanity validation。
- 若 source audit 或实现事实证明某类 service kthread 需要 bounded latency、emergency priority 或单独 class，停止并回到 RFC review；不得在 default switch 中保留隐式 RR 例外。
- 第一版不实现 Linux delayed dequeue / time-based lag decay，但必须实现 bounded saved service lag。
- 失败信号：source audit 无法证明 bootstrap/kthread direct EEVDF eventual progress；R1-R3b 任一 gate 未闭合；clean runtime 仍保留修复前整体回归；出现 arithmetic / membership anomaly；或用户侧 feedback 无法归类。此时停止阶段，按 gate 回写 `implementation.md` / `invariants.md` / tracking issue，不进入阶段 4。

模块边界预检：

- default class 初始化分散在 arch bootstrap、kthread 和 task creation 路径。修改时只做必要替换，不顺手重构 bootstrap / kthread ownership。
- 本阶段应优先翻转 `SchedEntity::new_normal()` 或等价默认 normal constructor，并把调用点收敛到该 helper；不得在 bootstrap / kthread / clone call site 手写 EEVDF runtime 字段。
- 若需要为 kthread 引入单独 scheduler class，超出本阶段默认目标，必须回到 RFC review。
- 用户态 smoke 保持独立 app owner：测试进程只使用 `anemone-rs` 提供的 `fork`、shared anonymous mapping、`sched_yield()`、`nanosleep()`、typed priority syscall 和 wait 接口，不把测试状态或阈值下沉到 scheduler core。`anemone-rs` 只增加 syscall 薄封装和 typed `PriorityWhich` / nice 返回值解码，不拥有 kernel nice 状态或调度策略。

write set：

- `anemone-kernel/src/sched/class/mod.rs`
- `anemone-kernel/src/sched/class/entity.rs`
- `anemone-kernel/src/task/mod.rs`
- `anemone-kernel/src/task/api/clone/mod.rs`
- `anemone-kernel/src/task/kthread/kthreadd.rs`
- `anemone-kernel/src/arch/riscv64/bootstrap.rs`
- `anemone-kernel/src/arch/loongarch64/bootstrap.rs`
- `anemone-kernel/src/sched/class/runqueue.rs`、`anemone-kernel/src/sched/class/eevdf.rs`、`anemone-kernel/src/sched/processor.rs`、`anemone-kernel/src/sched/mod.rs` 仅限 source audit、注释更新或发现 2B / 2C / 2D 漏闭合时的停阶段修复；若需要修改 placement、accounting、wake clamp、preempt decision 或 virtual-time contract，必须回到阶段 2 / RFC review，不能作为阶段 3 顺手修补。
- `anemone-apps/eevdf-test/{Cargo.toml,Cargo.lock,app.toml,src/main.rs}`
- `anemone-rs/src/sys/linux.rs` 仅限增加 raw `getpriority` / `setpriority` syscall wrapper。
- `anemone-rs/src/os/linux.rs` 仅限增加 typed `PriorityWhich`、`getpriority()` 返回值解码和 `setpriority()` 高层 wrapper。
- `anemone-apps/user-test/src/main.rs` 仅限在 competition root 之前执行 `eevdf-test`。
- `conf/rootfs/{minimal,pretest-rv64,pretest-la64}.toml` 仅限安装 `eevdf-test` app。
- `docs/src/rfcs/sched-eevdf-lite/{index,invariants,implementation,tracking-issues}.md` 与 `docs/src/devlog/transactions/2026-07-09-sched-eevdf-lite.md`，仅限记录获批扩张、阶段三证据、tracking closure 和 user-run / unrun 状态。

可观测性：

- default switch 后保留 FairClock / arithmetic / representation anomaly 计数；valid FairClock no-eligible 使用 release assertion。
- 记录 class 分类：ordinary task、bootstrap task、kthread、idle。
- `eevdf-test` 只在 case 边界输出一次开始、结果和计数摘要，不在 worker 热循环打印；kernel anomaly 继续只通过既有 `kerrln!` 观察。用户运行时把 live `console_log_level` 设为 `3`，本阶段不修改 tracked 默认日志级别。
- R1 validation branch 的 exact-yield / actual-vs-counterfactual probe 保持 bounded、一次性汇总和自校验；probe 不进入 production tree。
- 用户侧 instrumented signal 与 clean signal / read-write 对照写入 transaction devlog并标为 user-run；agent 不伪称运行。

验证：

- `just build`
- `just app build --arch riscv64 eevdf-test`
- `just app build --arch loongarch64 eevdf-test`
- `just fmt eevdf-test --check` 与 `just fmt user-test --check`；kernel formatter 若只命中既有 generated whitespace drift，必须单独标明，不把它伪称为本阶段 clean。
- basic boot / focused scheduler smoke。
- `sched_yield()` smoke。
- 多 runnable CPU-bound fairness smoke（若低成本可用）。
- nice 权重定向 smoke（若低成本可用）。
- sleep/wake fairness smoke（若低成本可用）。
- user-run instrumented signal：验证 R1 actual weighted eligibility、old-floor mirror、自校验守恒和 yield / same-task dispatch failure signal。
- user-run clean tree：signal / read-write case set 必须与基线一致；candidate 不得新增 FAIL / BROK / infra failure / timeout，也不得让 baseline PASS 退化，所有 result / failure-set diff 必须先分类。改善使 multiset 不同时，绝对耗时只比较全部共同完成且结果稳定的 case，或在结果集合稳定后重复 baseline / candidate；不能把改善本身判为 gate failure。无 probe 的绝对耗时承担 Stage 3 吞吐验收。
- 上述 runtime 由用户通过 rv64 端到端流程执行；agent 不伪称运行。结果尚未提供时，transaction 必须明确标为 `user-run pending` 或 `unrun`，且 Stage 3 不得关闭。
- Source audit：
  - no RR default production placement。
  - `EEVDF-021` direct-normal proof：bootstrap / kthread / ordinary task 均无 production RR 特例。
  - no deadline-only / min-floor eligibility。
  - clone/new task 不复制父 `SchedEntity`、remainder 或 saved lag。
  - ready / active 互斥，true wake restore exactly once，ParkPending no reward。
  - accounting before runnable requeue / parked handoff requeue。
  - fixed-weight remainder、deadline catch-up 和 common rebase 证明义务已关闭。
  - `DeferredPreempt` 不触发 switch-out accounting。

Tracking issue 关闭审查：

- `EEVDF-017` 的 default-constructor source classification 保持 Neutralized，不作为 R1-R3b 算法 closure 的替代证据。
- `EEVDF-001` / `EEVDF-018` / `EEVDF-004` / `EEVDF-020` 必须在未来重新批准的 agent-run / user-run 证据齐全后才能分别移入 Neutralized；当前四项仍为未解决 Keter，不能用阶段 3 source-classification closure 替代。
- 若阶段 3 仍发现 placement、accounting、lag、virtual-time 或 weight visibility 漏闭合，停止并回到对应 correction gate；不得顺手补算法 contract。

退出条件：

- `Eevdf` 是 normal scheduler default；idle 仍由 Idle class 兜底。
- RR 不再作为 production normal path。
- `EEVDF-021` 的 source audit 与 canonical proof 一致：kthread direct EEVDF 只承诺 eventual progress，不承诺 bounded latency。
- R1-R3b 全部通过，temporary bridge / validation probe 已从 production tree 删除。
- instrumented signal 与 clean signal / read-write 验收通过；未跑项不能用于关闭 Stage 3。
- 用户侧反馈已正确归类；残余异常不能被自动归咎、自动排除或改名为 limitation。

## 阶段 4：实现收口、限制登记和后续优化排队

**状态：** 未执行且不再 active。本次收口是 Stage 3/R1 runtime acceptance 失败后的 Closed/deferred 处置，不是本阶段原计划的 Completed closeout；以下内容仅保留为历史计划。

前置条件：

- 阶段 3 的 default class 切换完成。
- R1 / R2 / R3a / R3b correction gates 与 Stage 3 clean runtime 验收全部关闭。
- agent-required 验证 floor 已执行，user-run 项明确区分。

交付：

- 收口 transaction devlog：
  - method-first transaction surface 证据。
  - `PendingResched` flags 证据。
  - EEVDF private `account_current(now)` 边界。
  - weighted FairClock / eligibility 公式与 competition membership。
  - true leave / join service lag、ParkPending continuous handoff 和 full-set arrival decision。
  - accounting remainder、deadline catch-up 与 coordinate rebase。
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
  - delayed dequeue / time-based lag decay / stronger dynamic reweight。
  - latency nice。
  - 与未来 `sched_*` syscall real policy 切换的集成。
  - SMP migration / load balance 单独 RFC。

审计：

- 搜索 `RoundRobin` / `SchedClassPrv::RoundRobin`，确认每个保留点都有理由。
- 搜索 `TODO.*eevdf` / `deadline-only` / anomaly log，确认过期 TODO 已更新。
- 搜索 hot-path logs，确认普通路径无日志；若 anomaly error 持续出现并污染 benchmark，按停止条件处理，不能隐藏报告。
- 对照 `tracking-issues.md`，把已关闭项移到 Neutralized，未关闭项保留为后续 gate。

反馈假设：

- 假设第一版 EEVDF-lite 可以在不引入树索引的情况下满足当前中性 fairness 验收。
- 失败信号：任务规模导致 O(n) 本身成为主要性能瓶颈，valid FairClock / arithmetic anomaly 命中，或 clean-tree throughput 仍处于修复前回归范围却被文档接受；此时停止 closeout，回到 RFC review 或开后续 gate。

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
- 用户运行 clean rv64 signal / read-write 对照并记录 case / failure multiset、profile interval 与 anomaly；未运行不能关闭 Stage 3 / 4。

退出条件：

- 第一版 EEVDF-lite 支持矩阵、未支持矩阵和验证证据全部可追踪。
- 后续 tree index、完整 Linux EEVDF、sched syscall real policy、SMP migration 不混入第一版 closeout。

## 旁路审计清单

以下清单服务历史实现与未来重开审查；当前 RR default 本身不是旁路或停止信号。

- `rg -n "SchedClassPrv|SchedEntity::new|RoundRobin|Idle|Eevdf|eevdf" anemone-kernel/src`
- `rg -n "ScheduleMode|schedule_preempt|schedule_wait_sleep|schedule_yield|schedule_current_yield|schedule_idle|schedule_zombie" anemone-kernel/src/sched`
- `rg -n "SchedEvent|on_event|EnqueueReason|RequeueReason|SwitchOutReason" anemone-kernel/src/sched`
- `rg -n "local_requeue_current|local_enqueue|remote_enqueue|task_enqueue|local_wake_enqueue|wake_enqueue|remote_wake_enqueue" anemone-kernel/src/sched anemone-kernel/src/task`
- `rg -n "PendingResched|ReschedCause|request_resched|take_pending_resched|restore_pending_resched|mark_need_resched|fetch_clear_need_resched" anemone-kernel/src`
- `rg -n "switch_out\\(|on_switch_out|on_switch_in|account_current|local_sched_tick" anemone-kernel/src/sched anemone-kernel/src/task`
- `rg -n "TaskSchedState|ParkState|WakeEnqueueResult|is_sched_runnable|on_runq" anemone-kernel/src/sched anemone-kernel/src/task`
- `rg -n "nice\\(|inherit_nice_before_publish|set_nice|setpriority|getpriority" anemone-kernel/src/task anemone-kernel/src/sched`
- `rg -n "Instant::now|Duration|SYSTEM_HZ|kconfig_defs" anemone-kernel/src/sched anemone-kernel/src/time scripts/xtask/src/config conf/.defconfig`
- `rg -n "deadline|vruntime|rq_vtime|anomaly|wake_clamp|yield_penalty|base_slice" anemone-kernel/src/sched`
- `rg -n "legacy_placement_floor|remainder|saved.*lag|class.active|current" anemone-kernel/src/sched/class`

未来 EEVDF 实现允许保留的旁路必须满足三点：

- 不改变 EEVDF normal default 目标。
- 有日志、计数、断言或文档说明边界。
- 有明确后续 gate 或 current limitation。

## 可观测性清单

以下为未来重新打开 EEVDF runtime acceptance 时的观察面；当前不要求 RR 主线继续运行这些 gate。

- anomaly：checked FairClock / arithmetic / representation failure count、last reason 和每次记录的 `kerrln!`；valid FairClock no-eligible 由 release assertion 暴露。
- scheduler constants：base slice、true-sleep service-credit / debt bound、yield penalty、weight table 来源、system HZ。
- runtime accounting：可在 debug build 或 smoke 中观察 `delta_exec`、remainder、`vruntime`、`deadline`、`exec_start` 更新。
- FairClock：R1 validation branch 观察 actual weighted eligibility、old-floor counterfactual、yield / dispatch 关联和自校验；production 不保留热路径计数。
- default class switch：ordinary task、bootstrap task、kthread、idle 的 class 分类。
- bootstrap / kthread progress proof：记录 direct normal EEVDF 分类、无 production RR 特例，以及 bounded latency 不属于本 RFC。
- membership / placement：ready / active transfer、true `enqueue_woken()` lag restore、`handoff_woken_current()` continuous transfer，以及 no-switch abort 不进入 class 的 source proof 或定向计数。
- fairness smoke：每个 worker 的实际运行计数或时间份额。
- nice smoke：不同 nice task 的相对份额。
- yield smoke：yielding task 与 non-yielding task 都有进展。
- user-run gates：instrumented signal 与 clean signal / read-write 对照必须标明 user-run；iozone、long fairness log 和 deferred-count trace 继续作为补充反馈。

## 停止边界

未来重新打开 RFC 后继续追查：

- EEVDF pick 退化为 deadline-only。
- actual eligibility 仍读取 min-floor，或 valid FairClock 出现 no-eligible。
- R1 后 yield / same-task dispatch 仍处于修复前数量级。
- `account_current(now)` 双记或漏记。
- accounting split 改变总 `vruntime`，deadline catch-up 丢失 request phase。
- runnable current 在更新 `vruntime` 前入队。
- `DeferredPreempt` 被错误当作 switch-out。
- wake placement 绕过 stale-safe wait-core revalidation。
- ready / active membership 出现空洞或重复。
- parked handoff 或 no-switch abort 错走 true join / lag restore / yield penalty。
- checked arithmetic / rational representation 在真实 workload 命中，或坐标进入 saturation。
- 未获重新批准或未通过 runtime acceptance 就把 production default 切回 EEVDF。
- nice 变化长期不影响 fair accounting。
- anomaly error 出现并主导 benchmark，或 probe 自校验不守恒。

停止并记录为后续 gate：

- tree / RB-tree / dual-index queue 优化。
- Linux 完整 delayed dequeue / lag decay。
- latency nice。
- realtime / deadline / idle policy 的真实调度类。
- 用户态 `sched_*` syscall 动态策略切换。
- SMP task migration 和 load balancing。
- wait-core preempt residual、IRQ-off allocation 或长期不可抢占内核路径的独立修复。

## Probe / Vertical Slice Gates

本节为历史计划。R1 已执行并失败；R2、R3a、R3b 未执行且当前不具备启动授权。未来继续时必须先重新打开 RFC review，不能把本节文字直接当作 active work order。

默认不要为 probe / feedback 新建通用 `feedback.md`、`probe.md` 或 `experiments.md`。计划写在本节；执行结果写入 transaction devlog。只有证据包过长时，才在 `backgrounds/` 下增加具体命名的证据文件，并从本节链接。

P1 仍是已关闭的 accounting-owner gate。原 P2 / P3 已被 2026-07-11 runtime feedback 证伪，只保留历史说明；历史 correction 顺序原定为 R1 -> R2 -> R3a -> R3b -> Stage 3 clean-tree runtime。R1 未满足 Exit，后续 gate 因 RFC 关闭而 inactive；未来必须先重新 review 顺序，不能直接套用。

### Gate P1 - `account_current(now)` 与入队前执行段结算

**假设：** EEVDF runtime accounting 可以表达为一个 class-private 幂等 `account_current(now)` helper，并由 method-first transaction 在 runnable requeue、parked handoff requeue、wait park switch 或 exit switch 前调用；`switch.rs::switch_out()` 的 task hook 仍只负责 context-switch bookkeeping。

**保护目标 / 不变量：** runnable current task 不得以 stale `vruntime` / `deadline` 重新入队；tick 和 switch-out / requeue 不得重复计算同一段执行时间。

**最小 write set：** `anemone-kernel/src/sched/mod.rs`、`anemone-kernel/src/sched/processor.rs`、`anemone-kernel/src/sched/class/mod.rs`、`anemone-kernel/src/sched/class/runqueue.rs`、`anemone-kernel/src/sched/switch.rs`。

**非目标：** 不重新设计 task CPU usage accounting、wait-core state、trap entry 或 context switch assembly。

**最低验证：** `just build`；source audit 证明 class accounting transaction 先于 runnable requeue，`DeferredPreempt` 不 accounting；若有低成本 focused smoke，覆盖 tick/switch 不双记。

**失败信号：** 某个 schedule path 在 class transaction 前重新入队，`account_current(now)` 无法幂等，或 class accounting 必须从 `switch.rs::switch_out()` 中的 task hook 才能正确运行。

**回写：** 执行事实写 transaction devlog；若 hook ordering 改变 accepted invariants，更新 `invariants.md`；若 task hook ownership 改变，更新本计划和 tracking issues。

**退出：** accounting transaction ordering 与 EEVDF private helper 已证明，`EEVDF-002` 可以关闭；否则阶段 2 停止在 default class switch 之前。

**证据：** [事务日志中的 Checkpoint 2B closure](../../devlog/transactions/2026-07-09-sched-eevdf-lite.md)。

### Historical Gate P2 - min-floor eligibility（已失效）

原 P2 接受 monotonic minimum-`vruntime` floor 作为 eligibility clock。2026-07-11 的 exact-yield evidence 证明该假设错误，并且原 focused KUnit / source audit 没有覆盖 default EEVDF 下的 singleton-eligibility feedback。该 gate 不再可执行，也不能作为 `EEVDF-001` / `EEVDF-020` closure 依据；历史执行事实保留在 transaction devlog，纠正入口是 R1 / R2 / R3a / R3b，其中 R2 关闭 FairClock 所需的 competition membership 与 full-set arrival transaction。

### Historical Gate P3 - parked-handoff wake clamp（已失效）

原 P3 证明了 stale-safe wake transaction 的方法分流，但把 `ParkPending` handoff 当成 bounded wake-clamp 点。修订后的 competition membership 证明该路径从未离开 `C`，不能获得 wake reward。该 gate 不再可执行，也不能作为 `EEVDF-004` closure 依据；纠正入口是 R2。

### Gate R1 - Direct Weighted FairClock Repair

**Hypothesis:** 只把 actual eligibility / pick / yield 的时钟从 monotonic min-floor 替换为当前 `C` 的 weighted FairClock，就会让 min-floor 人工制造的 singleton eligible set 消失，并使同一 signal profile 的 yield、same-task dispatch 和 yield self-pick 脱离修复前数量级。该 intervention 同时验证 min-floor mechanism 对整体失速的主要因果贡献。

**Protected Goal / Invariant:** R1 执行时 eligibility 必须来自 weighted FairClock，且 intervention 期间 default normal 保持 EEVDF；测试 case set 不得缩小，现有 PASS 不得退化，也不得新增 FAIL / BROK / infra failure / timeout；nice direction、request-completion outcome、bounded yield 与 owner-CPU 边界不得削弱。不得用 forced handoff、skip-current、penalty / HZ / slice tuning、case-specific yield 旁路或隐藏 anomaly 让 gate 通过。

**Minimum Write Set:** production correction 默认只写 `anemone-kernel/src/sched/class/eevdf.rs` 及其 focused KUnit、本 RFC / tracking issue 与 transaction devlog。若 existing class-active association 不足以构造无空洞 snapshot，停止并上报 R2-style method / write-set expansion，不在 R1 偷改 membership。validation instrumentation 必须在独立临时分支 / commit 中维护；pre-fix evidence commit `d0d4196f` 不整体 cherry-pick 到产品分支。

**Validation-only Write Set:** 为复用同一观察语义，临时 validation branch 允许只加入 probe / profile 适配到以下既有代码文件（scheduler owner、用户 profile 和关机汇总点）：`anemone-apps/user-test/ltp/profile.txt`、`anemone-apps/user-test/src/main.rs`、`anemone-kernel/src/power.rs`、`anemone-kernel/src/sched/{class/eevdf.rs,class/runqueue.rs,mod.rs,processor.rs}`。这些改动只能记录 yield/dispatch correlation、关机汇总和同一 signal workload 选择，不得改变 dispatch、membership、pending、wait 或测试语义，也不得用 profile / group 修改绕过失败 case；production correction 不得依赖它们。旧 evidence commit 中与 signal observation 无关的 `groups/pipe.txt` testcase bypass 和 `class/entity.rs` RR constructor 注释不在本 gate 的 write set，不能随 probe 复用。

**Non-goals:** 不改 true leave / join、saved lag、`ParkPending` 分类、arrival transaction surface、accounting remainder、deadline catch-up、coordinate rebase、penalty 形式 / 常量或 dynamic renice。允许临时 `legacy_placement_floor` 继续只服务 fresh / ordinary wake / `ParkPending` 三类旧 placement；其行为消费不得参与 eligibility、pick、yield、lag 或 preempt，维护旧坐标的 update 站点也不得成为这些决策的 truth，R2 必须删除 bridge。

**Validation Floor:** `just build`、`git diff --check`；focused KUnit 覆盖 equal / unequal weight、动态 `v0`、eligibility boundary、non-empty set 必有 eligible 和 old-floor counterexample；source audit 分类列出 `legacy_placement_floor` 的行为消费与维护 update 站点。用户运行同一 instrumented signal profile，保持 actual correlation 守恒、`mismatch/invalid/pending_overwrite/missing_yielding/missing_pick = 0`，新增 new-actual weighted eligibility 与 old-floor counterfactual 的镜像核对，并记录同一 signal workload 的端到端 interval；公开修复前对照为 EEVDF `78s`、RR `57s`，各只有一份用户运行值，若单次 post-fix A/B 无法区分波动，必须在 probe baseline 与 candidate 上各重复一次再判定。case set 必须与基线一致；candidate 不得新增 FAIL / BROK / infra failure / timeout，也不得让 baseline PASS 退化。所有 result / failure-set diff 都必须记录并分类，未分类前不得用总 interval 关闭因果；若 timeout 消失或结果改善使 multiset 不同，timing 判断必须改用共同完成且结果稳定的 case subset，或等待结果集合稳定后重复 baseline / candidate，不能把改善本身当成 gate failure。

**Failure Signals:** actual eligibility 仍读取 legacy floor；valid FairClock 出现 no-eligible；probe 自校验非零；candidate 新增 FAIL / BROK / infra failure / timeout、baseline PASS 退化，或 result / failure-set change 无法分类；yield / same-task dispatch / self-pick 仍落在修复前计数数量级或仍由百万级重复 self-pick 主导；这些计数已回落但同一 signal workload 的 interval 在重复 A/B 中仍接近修复前 EEVDF `78s`、且没有相对 RR `57s` 对照稳定收窄；为了改善结果必须同时改变 placement、penalty 或 producer。命中任一项都停止 R1，只能声明 min-floor 公式缺陷已被替换，不能关闭其对整体失速的主要因果；在重新分类和 RFC 回写前不开始 R2。

**Write-back:** build / KUnit / source audit / user-run counts 与 failure-set diff 追加到 transaction devlog；公式或 protected invariant 变化回写 `index.md` / `invariants.md` 和 `EEVDF-001`；仍存在的 expected-behavior defect 才进入 register open issues，不能改名为 accepted limitation。

**Exit:** production actual path 的 eligibility / pick / yield / preempt 不再读取 min-floor，R1 runtime intervention 满足计数、自校验和同口径 signal 因果判定要求，`legacy_placement_floor` 的行为消费只剩白名单三类 placement 且维护 update 不驱动算法决策，`EEVDF-001` 才可按证据 Neutralized。随后才进入 R2；validation branch 的 probe/profile/power hook 必须删除，并以 `rg` source audit 确认 `sched_probe`、`EevdfProbe`、`dump_debug_probe` 等临时符号不在 production tree，不能进入产品提交。

**Outcome:** R1 实现与公式验证通过，但用户运行仍有 `1,393,625` 次 yield、`1,233,143` 次 yield self-pick 和 `1,232,735` 次 weighted `self_only_eligible`；百万级反馈仍主导，明确命中本 gate 的 Failure Signals。R1 未 Exit，`EEVDF-001` 保持 Keter，后续 gate 未获启动授权。详见 [Stage 3 eligibility 与整体吞吐回归证据（2026-07-11）](./backgrounds/stage3-eligibility-regression-20260711.md) 与 transaction closeout。

### Gate R2 - Competition Membership、Service Lag 与 Full-set Arrival

**Hypothesis:** ready / active 互斥 membership、true leave / join 的 bounded exact-rational service lag，以及 accounting + placement + enqueue + full-set preferred decision 的 owner transaction 可以在不实现 delayed dequeue / time-based lag decay 的前提下闭合第一版 sleep/wake 公平性。

**Current preflight:** 现有 `Processor` 路径先调用 `RunQueue::enqueue_new()` / `enqueue_woken()`，再调用 `decide_preempt_current()`；后者才推进 current accounting。因此当前 surface 已知不能满足“accounting 后再 placement、enqueue 与 full-set decision 同一 snapshot”的最终合同，R2 必须先完成 method-contract review，而不是把该扩展留作未证实的条件分支。

**Protected Goal / Invariant:** `C = ready union class-active current` 无空洞、无重复；true block / wake 才是 leave / join；yield、preempt 和 `ParkPending` handoff 保持 continuous membership；no-switch abort 不进入 class；generic `dequeue()` 不得默认等于 true block；saved lag 不缓存 current nice truth，wait-core private identity 不进入 scheduler algorithm。

**Minimum Write Set:** `anemone-kernel/src/sched/class/{mod,eevdf,entity,runqueue}.rs`、`anemone-kernel/src/sched/processor.rs` 及其对应 owner-CPU caller、focused KUnit、本 RFC / tracking issue 与 transaction devlog。若 method-contract review 证明还需要 `sched/mod.rs` 或其它 owner surface，必须先记录扩展原因、批准点和验证影响；不得在旧 surface 间制造第二套 snapshot。Kconfig / generated path 仅在 service-credit / debt bound 改名或改义时进入。wait-core 不在 write set。

**Non-goals:** 不改 FairClock 公式、yield policy、accounting remainder、deadline catch-up、coordinate rebase、dynamic renice、delayed dequeue、lag decay、scheduler policy syscall 或 SMP migration。不得为复用旧代码继续保留 `legacy_placement_floor`。

**Validation Floor:** `just build`、`git diff --check`；source audit 覆盖 ready / active transfer、true block / exit / wake、yield / preempt / ParkPending / abort、generic `dequeue()` 语义和 legacy-floor 行为消费与维护 update 站点，确认 bridge 完整删除。focused KUnit 用交叉乘证明 raw rational lag、exact clamp、unequal-weight non-integer round trip、positive / negative bound、`W0 == 0` 的 interior anchor、placement error contract，以及 current-ineligible / candidate-winner / existing-peer-winner / 无 request-completion outcome 的 deadline-tie full-set cases。focused wake / wait-abort runtime 不得新增 membership 或 arithmetic anomaly。

**Failure Signals:** pick / set-next 之间 entity 离开抽象 `C`；同一 entity 同时 ready / active；continuous path 保存 / 恢复 lag；同一 wake 重复消费 lag；checked rational representation failure；full-set decision 需要读取 wait-core private state；或需要未获批 owner / public API 扩展。命中后停止 R2 并回到 RFC review。

**Write-back:** execution facts、KUnit、source audit 和 runtime 结果追加 transaction；method surface / stage order / write set 变化更新本文件；membership、lag 或 owner invariant 变化更新 `index.md` / `invariants.md` 与 `EEVDF-018` / `EEVDF-004` / `EEVDF-020`。

**Exit:** `legacy_placement_floor` 完整删除；generic `dequeue()` 已删除或分类为窄 transaction；ready / active membership 与 full-set arrival transaction 闭合；true join service lag 及 representation failure 子门有证据；`EEVDF-018` / `EEVDF-004` 可 Neutralized，`EEVDF-020` 只关闭 R2 子门。随后进入 R3a。

**Evidence:** R1 transaction evidence；没有 R1 exit 不开始本 gate。

### Gate R3a - Segmentation-invariant Accounting 与 Request Catch-up

**Hypothesis:** entity-local division remainder 可以让 fixed-weight runtime accounting 对 transaction 分段不敏感；strict multi-request catch-up 可以保持 deadline phase，而不改变 FairClock、lag placement 或 slice / HZ policy。

**Protected Goal / Invariant:** `account_current(now)` 仍是唯一公平记账 owner；tick / requeue 不双记；`0 <= remainder < weight`；weight 变化时按 `floor(remainder * new_weight / old_weight)` 向零换基；初始化、自然续期或没有 outstanding yield penalty 的 catch-up 在无 arithmetic failure 时满足 `vruntime < deadline <= vruntime + request`，yield penalty 暂态除外；request completion 仍通过 transaction-local outcome 立即消费。

**Minimum Write Set:** `anemone-kernel/src/sched/class/{eevdf,entity}.rs`、focused KUnit、本 RFC / tracking issue 与 transaction devlog。若 fixed-weight remainder 需要跨 lifecycle 保存，只能进入同一 entity payload；不得增加 processor-global 副本或 shared trait field。

**Non-goals:** 不改 FairClock、membership、saved lag formula、yield、slice、HZ、service-lag bounds、coordinate rebase 或 dynamic renice strong semantics。current non-transactional renice 继续明确为 weak semantics；R3a 只负责 remainder denominator 的明确换基，完整 lag-preserving reweight 属于独立 follow-up RFC / gate，不属于本次 R1-R3b 或阶段 4 收口。

**Validation Floor:** `just build`、`git diff --check`；KUnit 比较任意 split / merged runtime、small-delta accumulation、multi-request overrun、request-completion outcome、block / wake remainder continuity 和 weight-change remainder rebase；source audit 证明所有 accounting caller 显式消费或丢弃 outcome。用户运行同一 signal profile，case set 必须与基线一致，candidate 不得新增 FAIL / BROK / infra failure / timeout，也不得让 baseline PASS 退化；所有 result / failure-set diff 必须记录并分类，结果改善不自动构成 gate failure，同时不得新增 arithmetic anomaly。

**Failure Signals:** transaction 分段改变总 `vruntime`；remainder 跨 weight 变化被无解释复用；deadline catch-up 重置当前 phase、溢出或在 penalty 已耗尽后破坏 `v < d <= v + q`；candidate 新增 FAIL / BROK / infra failure / timeout、baseline PASS 退化，或 result / failure-set change 无法分类；为了通过需要改 FairClock / lag / penalty。命中后停止并回写 `EEVDF-020`，不进入 R3b。

**Write-back:** execution facts 写 transaction；accounting / deadline invariant 变化更新 `invariants.md`；`EEVDF-022` 只补历史关联，active closure 仍归 `EEVDF-020`。

**Exit:** fixed-weight split invariance、remainder lifecycle、strict catch-up 和 outcome propagation 全部有证据，`EEVDF-020` 的 R3a 子门关闭；随后进入 R3b。

**Evidence:** R2 transaction evidence。

### Gate R3b - Common Coordinate Rebase

**Hypothesis:** 在上、下 headroom 不足前，对 owner CPU 上所有 competing entities 的 `vruntime` / `deadline` 做同一个可加或可减的公共平移，可以避免 `u64` saturation，同时保持 FairClock、service lag、deadline distance、eligibility 和 pick 结果不变；若当前坐标跨度无法放入预留的 interior window，必须先停止而不是饱和。

**Protected Goal / Invariant:** rebase 不改变外部可见调度选择，不触碰 sleeping exact-rational service lag 或 accounting remainder；真实 workload 不允许进入 saturation-after-the-fact 状态。

**Minimum Write Set:** `anemone-kernel/src/sched/class/{eevdf,entity}.rs`、focused KUnit、本 RFC / tracking issue 与 transaction devlog。若 rebase 需要跨 class 或 processor-global coordinate truth，停止并申请 owner-boundary review，不新增全局 offset 第二真相源。

**Non-goals:** 不改 FairClock、membership、lag formula、accounting、deadline request length、yield、renice 或 tree index。短 signal runtime 不要求实际触发 rebase。

**Validation Floor:** `just build`、`git diff --check`；focused KUnit 构造上下 headroom 边界，验证向上与向下公共平移、坐标跨度可容纳性，并证明平移前后 `V-v`、saved service amount、`deadline-vruntime`、eligibility、deadline order 和 pick 结果相同；source audit 证明 production arithmetic 在 saturation 前调用 rebase，空集合重置到预留 headroom 的固定 anchor，且不泄漏旧 absolute coordinate。

**Failure Signals:** 需要 saturate 后再修复；rebase 改变 eligible set / pick；sleeping entity 仍依赖 absolute `vruntime` / `deadline`；出现第二份 global coordinate truth；或 checked helper 在接受边界内仍可失败。命中后停止，`EEVDF-020` 保持 active，阶段 3 不恢复。

**Write-back:** KUnit / source audit 追加 transaction；coordinate representation 或 owner boundary 变化更新 `index.md` / `invariants.md` 与 `EEVDF-020`。

**Exit:** proactive rebase 与 headroom proof 闭合，R2 / R3a / R3b arithmetic 子门全部通过，真实 correction runtime 无 arithmetic fallback，`EEVDF-020` 可 Neutralized；删除临时 validation hooks，恢复 Stage 3 clean-tree runtime 验收。

**Evidence:** R3a transaction evidence。

## 实现期反馈记录

- 2026-06-22：文档层反馈重写；实施计划从过细 8 阶段压缩为 4 个主阶段，并把 runtime accounting position、`rq_vtime` formula、wake clamp 三个高风险点改为 probe / vertical slice gate；目标和不变量保持不变。
- 2026-07-06：sched-split 后原地 v2 重写；接受 scheduler-private wrapper 作为下层前提，并补入 runtime accounting、wake placement、bounded yield penalty、class-specific `SchedEntity` payload、Kconfig 常量边界和 agent/user 验证责任分层。
- 2026-07-07：补 typed pending resched request 设计：`need_resched` 不再作为 bool 压扁 tick / runnable-arrival source，deferred preempt 必须恢复同一组 pending bits；本轮进一步统一为 `PendingResched` flags。
- 2026-07-07：纠正 v2 草案中的 event-first 偏差；删除 `SchedEvent` / `EnqueueReason` / `RequeueReason` / `SwitchOutReason` 作为 accepted contract 的设计，改为 method-first 的 scheduler class lifecycle transaction surface。`PendingResched` 保持 processor / scheduler-core 私有 pending request；EEVDF 的 `account_current(now)` 收归为 class-private helper；wake handoff / abort wait 边界改由 `enqueue_woken()`、`handoff_woken_current()` 和 `requeue_aborted_wait_current()` 等方法名表达。
- 2026-07-07：文档层 review 后修正 gate 顺序：当时将阶段 1 整体职责收窄为 method-first surface、typed pending 和 lifecycle transaction 位置；`rq_vtime`、EEVDF private accounting、wake placement exactly-once 和 virtual-time arithmetic 由阶段 2 / Gate P1-P3 在 default switch 前关闭。同时补充 remote runnable arrival 的 owner CPU placement 后 preempt decision 线性化要求，避免 source CPU 读取目标 CPU current。
- 2026-07-08：收窄 scheduler class surface：`pick_next_task()` 不接收 `now`，`set_next_task(task, now)` 不接收 bootstrap `first` 参数，`enqueue_new()` / `enqueue_woken()` 不接收 wall-clock `now`；若实现期发现 new / wake placement 必须依赖当前时间，停止并回到 RFC review。`EEVDF-005` 接受的 switch-in 顺序固定为 pick / set-next / mapping 准备 / task switch-in hook / current publication / architecture switch，no-switch abort 和 deferred preempt 不得调用 `set_next_task()`。
- 2026-07-08：阶段边界审查后修正实施计划：阶段 2 只引入 EEVDF-specific constructor 和算法 gate，不把 default normal constructor 提前翻到 EEVDF；阶段 3 才执行 default normal switch。`task/api/priority/` 的 weight visibility 修复归入阶段 2 条件 write set；阶段 3 的 scheduler core / EEVDF 文件改为 audit-only 或阶段 2 漏闭合时的停止信号。同时补充 `switch_mapping(prev, next)` 相对 `set_next_task()` 的 source-audit 位置，避免 switch-in execution segment 起点含义悬空。
- 2026-07-09：阶段 1 保持一个概念阶段，但拆为 Checkpoint 1A / 1B：1A 关闭 trait / `RunQueue` / entity split 和 RR/Idle 机械适配，1B 关闭 typed pending、schedule entry、trap / IPI plumbing 与 `EEVDF-005` source audit。阶段 2 前置条件改为 1B 关闭，避免一个 gate 同时吞下所有 cross-layer plumbing。
- 2026-07-09：阶段 2 保持一个概念阶段，但拆为 Checkpoint 2A / 2B / 2C / 2D：2A 只做 payload / class compile scaffold，2B 关闭 P1 accounting，2C 关闭 P2 `rq_vtime` / arithmetic / bounded yield，2D 关闭 P3 wake clamp / parked handoff。阶段 3 前置条件改为 2B / 2C / 2D 全部关闭，避免一次性算法落地。
- 2026-07-09：Checkpoint 1B source audit 后澄清 wait abort / handoff wording：no-switch abort 不调用 class transaction，`ParkPending` 由 `handoff_woken_current()` 收口，`requeue_aborted_wait_current()` 只保留给无 wake reward 的 abort-park requeue 路径；目标、不变量、阶段顺序和 write set 不变。
- 2026-07-09：Checkpoint 1B 后反馈指出 `schedule_preempt(pending)` 内部 restore caller 传入的 `PendingResched` value 会混淆 flags value 与 processor pending slot ownership。接受 caller-owned restore：执行 `take_pending_resched()` 的 trap tail caller 在 `SchedulePreemptResult::Deferred` 时恢复原 pending set；`schedule_preempt()` 只返回 deferred，不写 processor pending state。目标、不变量和阶段顺序不变。
- 2026-07-10：Checkpoint 2C 前设计共识闭合：virtual-time state 以 normalized ns `u64` 存储、`u128` 中间计算、saturating helper 记录 anomaly；`rq_vtime` 使用 monotonic min-vruntime floor，visible set 包含 queue 和 current；eligibility 为 `vruntime <= rq_vtime`；new placement 使用 `vruntime = rq_vtime`；deadline 仅初始化或 `vruntime >= deadline` 时自然续期；tick / runnable-arrival preempt 只接受 eligible 且 deadline 更早的 candidate；yield 只后推 deadline，不改 `vruntime`；nice visibility 只保证后续 owner CPU 观察最新 `Task::nice()`；2C 不实现 wake clamp。
- 2026-07-10：Checkpoint 2C 关闭后接受 class shape 反馈：跨 class precedence 在 `sched/class/mod.rs` 以 high-to-low class order 集中保存为唯一真相，各 `Scheduler` implementation 只关联自己的 class identity；`RunQueue` 的 pick 与 preempt comparison 都消费该 class-domain order，不再保存 `class_rank()` 或独立 pick 顺序。`EevdfEntity`、`SchedClassPrv` 和通用 payload constructor 收窄到 scheduler class owner，删除没有模块外消费者的 EEVDF entity accessor。该纠正保持 `Eevdf > RoundRobin > Idle` 行为、EEVDF 算法和 ABI 不变；nice 值域、nice-to-weight 表及其 owner boundary 明确延期，不在本反馈中修改。
- 2026-07-10：阶段 3 前接受 Nice / priority 边界反馈纠正，不新增 Checkpoint 2E：引入 `Nice` / 受约束原子存储，拆分 `task/api/priority` syscall 模块，修复低代价且已确认的 target-selection / errno 语义，并把已发布 task 的临时非事务性 nice 写入集中到 `Task::set_nice(Nice)`。动态 renice 的 owner-CPU `RunQueue` command / IPI、立即 deadline/requeue 更新、`RLIMIT_NICE`、user namespace / LSM 强一致性仍延期；方法注释记录 deferred-accounting 边界及 owner-CPU transaction 对直接原子写入的替换条件，不为该临时边界增加逐次 renice 日志。该 feedback correction 关闭前不开始阶段 3，但不改变阶段编号、EEVDF 算法 gate 或 default-switch contract。
- 2026-07-10：阶段 3 source review 发现 `account_current(now)` 在续 deadline 后丢失 current request completion，tick 与 runnable-arrival decision 无法从归一化后的 snapshot 恢复该事实；同时 effectful renewal 位于短路 `||` 中，arithmetic saturation 可跳过续期。按阶段 3 停止条件回到 Checkpoint 2C，新增 `EEVDF-022` correction gate；accepted EEVDF formula、owner boundary 和 default constructor 不变。
- 2026-07-10：`EEVDF-022` correction 关闭：deadline renewal 与 accounting 分别返回正交的瞬时 outcome，tick / runnable-arrival 通过 class-private production decision helper 消费 completion，wake normalization 只返回 arithmetic saturation；113 项 KUnit、阶段 3 四组 workload、source audit 与独立复审均通过。该关闭只恢复 Checkpoint 2C contract，阶段 3 仍保持修复状态，不触发阶段 4 转换。
- 2026-07-12：Stage 3 runtime feedback 触发 canonical reopening。用户运行的 read-write 对照显示相同 case / failure multiset 下 EEVDF profile 约为 RR 的 3.3 至 3.5 倍；exact-yield probe 将额外 dispatch 收敛到 min-floor `self_only_eligible` feedback，weighted-FairClock counterfactual 证明其中 `552,494 / 1,338,814` 个 snapshots 已有 eligible peer，同时 `786,320` 个 snapshots 仍无 eligible peer。由此撤销原 P2 min-floor 与 P3 parked-handoff clamp closure，重开 `EEVDF-001` / `EEVDF-018` / `EEVDF-004` / `EEVDF-020`，并建立严格顺序 R1 -> R2 -> R3a -> R3b。目标和 default EEVDF 不变；forced handoff、penalty tuning、case-specific bypass 与 probe commit 合并均不接受。
- 2026-07-12：scheduler-core post-close source audit 纠正历史 abort-wait 分类。already-completed wait 在进入 scheduler 时由 `AbortWaitSleep` 直接返回，不切换、不重入队、不调用 class；wait 已进入 parked 收口后变为 runnable 的唯一 production 路径是 `handoff_woken_current()`。此前为假设中的第三条 abort-park 路径保留的 `requeue_aborted_wait_current()` 从 processor facade、`RunQueue` transaction、`Scheduler` trait 与全部 class implementation 删除；R1-R3b 顺序和 EEVDF 算法目标不变。

## Write Set 扩展记录

- 2026-07-10：用户批准 Checkpoint 2C 后 class-shape correction 扩展到 `sched/class/{mod,entity,runqueue,eevdf,rr,idle}.rs`、本 implementation feedback 和 transaction devlog。扩展只用于 class precedence metadata 与 class-private entity visibility；不触碰 task / priority owner、nice / weight 架构、wait-core、2D wake clamp 或 default normal constructor。
- 2026-07-10：用户批准阶段 3 前 Nice / priority feedback correction 扩展到 `sched/{mod,nice}.rs`、`sched/class/eevdf.rs`、`task/{mod,sched}.rs`、`task/api/clone/mod.rs`、`task/api/priority/{mod,target,getpriority,setpriority}.rs`、本 RFC canonical 文本和 transaction devlog。扩展不触碰 runqueue、processor、IPI payload、deadline / placement / accounting formula、Kconfig 或 default normal constructor；若实现要求这些边界，停止并回到 RFC review。
- 2026-07-10：用户批准阶段 3 default switch 扩展到独立 `anemone-apps/eevdf-test`、`anemone-apps/user-test/src/main.rs`、安装 `user-test` 的三份公共 rootfs manifest，以及本 RFC / transaction 文档。扩展只建立用户态黑盒 smoke 和公共 pretest 接入，不新增 scheduler debug ABI、procfs hook、test-only syscall、Kconfig policy 或 hot-path debug 日志；runtime smoke 由用户运行，agent 负责 app/kernel build、source audit、格式和文档验证。
- 2026-07-10：用户进一步批准阶段 3 用户态 smoke 扩展到 `anemone-rs/src/{sys,os}/linux.rs`，用于增加 nice 相关 `getpriority` / `setpriority` syscall 封装。低层保持 raw syscall 转发，高层拥有 typed selector 和 Linux raw return 到 nice 的解码；该扩张不改变 kernel ABI、priority permission、nice owner、renice transaction、EEVDF formula 或测试日志边界。

## 结构维护记录

- 2026-07-07：阶段 1 建议主动做同一 scheduler owner 内 split-only checkpoint：`sched/class/runqueue.rs` 承载 `RunQueue` facade，`sched/class/entity.rs` 承载 `SchedEntity` / `SchedClassPrv`，`Scheduler` trait 留在 `sched/class/mod.rs`。拆分依据是 scheduler 业务职责，而不是 `api.rs` 这类抽象层命名。
- 2026-07-09：上述 split-only checkpoint 收归为阶段 1A；trap / IPI、typed pending producer/consumer 和 schedule entry plumbing 收归为阶段 1B。
- 2026-07-10：2C 后结构反馈要求 class precedence truth 归还 scheduler class metadata，并收窄 EEVDF payload / class-private enum 的可见面；该维护不改变 lifecycle transaction surface、class 顺序或调度语义。
