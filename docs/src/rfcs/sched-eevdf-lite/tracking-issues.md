# sched EEVDF-lite tracking issues

**状态：** Active
**最后更新：** 2026-07-09
**父 RFC：** [RFC-20260622-sched-eevdf-lite](./index.md)
**事务日志：** [2026-07-09-sched-eevdf-lite](../../devlog/transactions/2026-07-09-sched-eevdf-lite.md)
**来源：** sched-split-aware v2 重写 / method-first scheduler class 纠偏 / 2026-07-07 文档层 review

本文只跟踪 design review 后确认的 sched EEVDF-lite 草案缺陷、证明缺口、边界冲突或会影响实现顺序、review gate、停止边界和验收判断的设计问题。

普通实现进度、TODO、benchmark 数字、用户侧长日志和阶段性交付项不写入本文；它们属于 [RFC index](./index.md) 的背景、非目标、风险，或 [迁移实施计划](./implementation.md) 的阶段内容。受控实现反馈不新建通用 feedback 文件；计划写在 [迁移实施计划](./implementation.md#probe--vertical-slice-gates)，执行结果进入 transaction devlog。若反馈暴露目标、不变量、owner boundary 或接受边界需要改变，必须回写 RFC canonical 文本和本文对应 issue。

分级沿用 Anemone review 口径：

- **Apollyon**：当前必须修复的错误结果、数据损坏、安全问题、崩溃或严重不可恢复状态。
- **Keter**：会阻塞后续实现方向或导致核心抽象不可复审，必须修正或明确改边界。
- **Euclid**：值得修正，但通常不阻塞第一版实现。
- **Safe**：记录即可，除非顺手修正。

## Apollyon

- 暂无。

## Keter

### EEVDF-001：`rq_vtime` / eligibility 公式必须闭合

**状态：** Active

**问题：** `rq_vtime` 不能只是最小 `vruntime` 或不可审查的临时统计；公式必须说明 current task 已被 pick 出 runqueue 后是否仍参与公平时钟、enqueue / dequeue / pick / `account_current(now)` 更新点、fallback 条件和异常观察面。

**修复落点：**

- [RFC index](./index.md) 的 fairness / eligibility 接受边界。
- [不变量需求](./invariants.md) 的公平性与 `rq_vtime` 不变量。
- [迁移实施计划](./implementation.md) Checkpoint 2C / Gate P2。

**反馈相关：** Checkpoint 2C / Gate P2 的 focused smoke 需要覆盖 eligible pick、no-eligible fallback、weighted vruntime progression、anomaly observation 和 yield progress。稳定 CPU-bound workload 在 warm-up 后 fallback anomaly 持续增长时，视为公式未闭合；执行事实写 transaction devlog，若公式改变 eligibility contract，回写 `index.md` / `invariants.md` 并再 neutralize 本 issue。

**关闭条件：** 公式、更新点、fallback 允许条件和 anomaly 语义已进入 canonical 文本或 Checkpoint 2C / Gate P2 证据，且默认 class 切换前该 checkpoint 已关闭。

### EEVDF-004：wake placement 必须 exactly-once 覆盖 parked handoff 分支

**状态：** Active

**问题：** 普通 wake 只有 `WakeEnqueueResult::Enqueued` 后才通过 `enqueue_woken()` 执行 wake clamp；`ParkPending` 不能立即 clamp，必须在 scheduler 收口 requeue 时通过 `handoff_woken_current()` exactly once clamp；`Stale`、`AlreadyCurrent`、`AlreadyQueued`、no-switch abort 和 `requeue_aborted_wait_current()` 都不得重复或错误 clamp。

**修复落点：**

- [RFC index](./index.md) 的 wake placement exactly-once 章节。
- [不变量需求](./invariants.md) 的 wake placement 不变量。
- [迁移实施计划](./implementation.md) Checkpoint 2D / Gate P3。

**反馈相关：** 阶段 1A 关闭 method surface，阶段 1B 关闭 schedule entry / wake placement 分流位置；具体 EEVDF wake clamp 由 Checkpoint 2D / Gate P3 关闭。2D 的 source audit 需要覆盖普通 wake `Enqueued`、parked handoff、no-switch abort、abort-park requeue、stale、already queued、already current。若失败是方法边界错误，按 1A / 1B 归属回写本计划和本 issue；若失败是 wait-core contract 变化，停止并路由回 sched-wait 相关 RFC。

**关闭条件：** `enqueue_woken()` / `handoff_woken_current()` 的 exactly-once 边界已证明，abort/stale/already-current/already-queued 不会获得 wake reward；未关闭前不得进入默认 class 切换。

### EEVDF-017：default class switch 必须被 blocker / gate 矩阵约束

**状态：** Active

**问题：** 阶段 3 切换默认 class 前，所有仍影响 accepted contract、实现顺序或验收边界的 Keter 必须已经 neutralized，不能只留下“停止条件”。

**修复落点：**

- [迁移实施计划](./implementation.md) 阶段 2 / 阶段 3 前置条件。

**反馈相关：** 本 issue 不拥有独立 probe；它消费阶段 1A / 1B review gate，以及 Checkpoint 2A / 2B / 2C / 2D。`EEVDF-021` 已通过 canonical eventual-progress 证明 neutralized；阶段 3 只验证 direct normal 分类和无 production RR 特例。若任一 checkpoint 的反馈要求改变目标、不变量或默认 class 接受边界，必须先回写 canonical 文本，本 issue 保持 active。

**关闭条件：** 最低矩阵全部关闭：阶段 1A 关闭 method-first surface、`RunQueue` / entity split 和 RR / Idle 行为保持；阶段 1B 关闭 typed pending、schedule entry plumbing 和 `EEVDF-005`；Checkpoint 2A 关闭 payload / class compile scaffold 且未提前切 default normal；Checkpoint 2B / Gate P1 关闭 `EEVDF-002`；Checkpoint 2C / Gate P2 关闭 `EEVDF-001` 与 `EEVDF-020`；Checkpoint 2D / Gate P3 关闭 `EEVDF-004`；阶段 3 source audit 证明 default switch 没有 ordinary / bootstrap / kthread production RR 特例。

### EEVDF-020：virtual time arithmetic 表示必须闭合

**状态：** Active

**问题：** Gate P2 不只需要 `rq_vtime` 公式，还必须确定 `Vruntime` / `Deadline` 的类型、单位、fixed-point scaling、overflow / saturating 策略、`delta_exec * NICE_0_WEIGHT / weight` 的精度边界，以及 deadline / slice 计算的 fail-closed 行为。

**修复落点：**

- [RFC index](./index.md) 的 runtime / deadline 公式描述。
- [不变量需求](./invariants.md) 的 fairness 与 arithmetic fail-closed 要求。
- [迁移实施计划](./implementation.md) Checkpoint 2C / Gate P2。

**反馈相关：** Checkpoint 2C / Gate P2 的验证必须能观察 weighted vruntime progression 和 overflow / precision 边界；若 arithmetic 需要改变 formula contract，回写 `index.md` / `invariants.md` 并与 `EEVDF-001` 一起关闭。

**关闭条件：** 类型、单位、scale、overflow / saturating 规则和 fail-closed 行为已明确；未关闭前不得进入默认 class 切换。

## Euclid

- 暂无 active Euclid。

## Safe

- 暂无 active Safe。

## Neutralized

### EEVDF-002：runtime accounting 必须有单一幂等边界

**状态：** Neutralized

**修复落点：**

- `anemone-kernel/src/sched/class/eevdf.rs` 中的 EEVDF private `account_current(now)` 是唯一推进当前执行段的 helper。
- `set_next_task(task, now)` 只记录下一段 `exec_start`；`task_tick()`、`requeue_yielded_current()`、`requeue_preempted_current()`、`handoff_woken_current()`、`requeue_aborted_wait_current()`、`put_prev_blocked()` 和 `put_prev_exiting()` 均先调用同一个 helper。
- `anemone-kernel/src/sched/switch.rs` 明确 `Task::on_switch_out()` 只保留 task / CPU usage bookkeeping，不作为 EEVDF fair accounting truth。

**Source audit:** `DeferredPreempt` 在 `schedule_inner()` 中提前返回，不调用 `switch_out()`、`local_pick_next()`、`set_next_task()` 或任何 EEVDF class transaction；runnable requeue、parked handoff、abort-park requeue、wait park switch 和 exit switch 都在 `RunQueue` 设置 `on_runq = true` 或真正切走前完成 class transaction。`account_current(now)` 在成功推进后刷新 `exec_start = now`，tick 后的 switch-out / requeue 只结算 tick 之后的新执行段。

**结论：** Checkpoint 2B / Gate P1 已关闭。2B 使用单调 actual-runtime scalar 证明 accounting 边界；weighted virtual-time arithmetic、`rq_vtime` 更新、deadline / slice fail-closed 规则和 bounded yield 仍归属 Checkpoint 2C / `EEVDF-001` / `EEVDF-020`，不因本条 neutralized 而提前关闭。

### EEVDF-003：schedule caller / pending resched reason 必须可传递

**状态：** Neutralized

**修复落点：**

- sched-split 的 scheduler-private wrapper 已取代裸 `schedule()` caller taxonomy。
- EEVDF-lite 的 class-visible 语义折回 method-first scheduler class transaction surface。

**结论：** EEVDF-lite 不重新引入公开 `ScheduleCaller` taxonomy，也不通过 catch-all event taxonomy 表达路径语义。

### EEVDF-006：weight source 只能作为内部 provisional contract

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 和 [不变量需求](./invariants.md) 明确 `Task::nice()` / `set_nice()` 是唯一 nice truth。
- EEVDF entity 不保存另一份 nice，也不在第一版保存 `cached_weight`。

**结论：** 第一版消费固定 Linux nice weight 表，但不承诺完整 priority syscall ABI 即时性。

### EEVDF-007：yield 需要独立算法语义

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) / [不变量需求](./invariants.md) 决策为 bounded yield penalty。

**反馈相关：** 具体 yield penalty 公式和 smoke 归入 Checkpoint 2C / Gate P2；若 yield 反馈显示长期立即选回或饥饿，回写 2C，而不是重新打开 event taxonomy。若异常实际来自 wake reward / no-reward 边界，路由到 Checkpoint 2D / Gate P3。

**结论：** yield 不再与 tick preempt 或 generic runnable requeue 混用。

### EEVDF-008：`SchedEntity` 扩展后的形状

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) / [不变量需求](./invariants.md) 决策为 class-specific payload。
- 阶段 1A 的 `entity.rs` 拆分保持 RR/Idle 形状与 `Copy` 行为，Checkpoint 2A 再接 EEVDF payload。

**结论：** `on_runq` 保持 shared physical truth；EEVDF state 进入 `SchedClassPrv::Eevdf(EevdfEntity)` 或等价结构，idle/RR 不理解 EEVDF 字段。

### EEVDF-009：weight 表与调度常量配置边界

**状态：** Neutralized

**修复落点：**

- [迁移实施计划](./implementation.md) 要求 base slice、wake clamp window、yield penalty window 和 anomaly threshold 进入 Kconfig。
- 实现必须同步 `conf/.defconfig`、live root `kconfig`、`scripts/xtask/src/config/kconfig.rs` 与 generated defs 使用点。
- Checkpoint 2A 建立 schema / generated plumbing；base slice、yield penalty window 和 anomaly threshold 的语义消费由 2C 关闭，wake clamp window 的语义消费由 2D 关闭。

**结论：** nice weight table 第一版固定 Linux 表，不提供 selector；未来替换权重表另走 follow-up。

### EEVDF-010：fallback anomaly 观察阈值

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 和 [迁移实施计划](./implementation.md) 明确 fallback anomaly 必须可观测。

**反馈相关：** 稳定 CPU-bound smoke 在 warm-up 后连续观察窗口仍增长 anomaly 时，反馈归入 Checkpoint 2C / Gate P2，视为 eligibility 公式未闭合，必须停止默认 class 切换。

**结论：** anomaly threshold 不再作为独立 active issue；它是 `EEVDF-001` / Checkpoint 2C / Gate P2 的观察面。

### EEVDF-011：O(n) runqueue 性能

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 和 [迁移实施计划](./implementation.md) 接受第一版使用线性 `Eevdf` queue / O(n) scan。

**反馈相关：** 若实现后任务规模导致 O(n) 本身成为主要瓶颈，进入后续 tree / dual-index optimization gate，不阻塞第一版语义闭合。

**结论：** 树索引、RB-tree 或双索引结构作为后续优化 gate，不阻塞第一版。

### EEVDF-012：`iozone` 数字不作为硬目标

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 明确 `iozone` 是历史动机和用户侧观察性反馈，不是 RFC 接受条件。

**反馈相关：** 用户侧 iozone、LTP、long fairness log、baseline 和 deferred-count trace 只能作为 implementation feedback。若反馈显示 wait-preempt residual，路由回对应 RFC；若反馈显示 EEVDF placement / accounting / eligibility 问题，回写本 RFC 的对应 gate。

**结论：** agent 不承担 baseline / iozone 长日志分析；吞吐数字不成为硬验收。

### EEVDF-013：是否先做空调度框架

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 和 [迁移实施计划](./implementation.md) 决策为直接以 EEVDF-lite 作为默认 normal scheduler 为目标。

**结论：** method-first scheduler class transaction surface 只服务 EEVDF-lite 和 RR 行为保持适配，不做空框架优先。

### EEVDF-014：是否第一版使用树结构

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的备选方案和 [迁移实施计划](./implementation.md) 接受第一版线性 scan。

**结论：** `BTreeMap`、RB-tree 或双索引结构作为后续优化 gate。

### EEVDF-015：是否把 wait-core / IRQ-off 问题并入本 RFC

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 的非目标和接受边界明确 EEVDF-lite 只处理 normal runnable task 到达调度点后的公平选择、runtime accounting、wake placement、yield penalty 和 tick preemption decision。

**反馈相关：** runtime log 若显示 wait-core stale wake、preempt deferred fairness gap、source-owner nested wait、IRQ-off allocation 或 long non-preemptible kernel path，反馈路由回对应 owner，不降低本 RFC 的 eligibility / accounting / placement 不变量。

**结论：** wait-core / IRQ-off residual 不作为 EEVDF-lite 的兜底目标。

### EEVDF-016：Scheduler trait 必须是 method-first class-local transaction surface

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md)、[不变量需求](./invariants.md) 和 [迁移实施计划](./implementation.md) 明确 `SchedEvent` / `on_event` / catch-all event bus 不再是 accepted contract。
- class-visible 语义通过 `enqueue_new()`、`enqueue_woken()`、`requeue_yielded_current()`、`requeue_preempted_current()`、`handoff_woken_current()`、`requeue_aborted_wait_current()`、`put_prev_blocked()`、`put_prev_exiting()`、`pick_next_task()`、`set_next_task()`、`task_tick()` 和 `decide_preempt_current()` 等 method-first transaction 表达。

**结论：** 阶段 1 source audit 禁止 scheduler implementation 引入 `SchedEvent` / `on_event` / event bus。

### EEVDF-018：`AbortWaitSleep` 不是一个单一 requeue event

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md)、[不变量需求](./invariants.md) 和 [迁移实施计划](./implementation.md) 把 no-switch abort、abort-park requeue 和 parked wake handoff 拆成不同 method-first path。

**反馈相关：** 具体 wake reward / no-reward 验证归入 Checkpoint 2D / Gate P3；若 abort path 获得 wake reward，回写 `EEVDF-004`。

**结论：** no-switch abort 不调用 scheduler class；`requeue_aborted_wait_current()` 不做 wake clamp / yield penalty；`handoff_woken_current()` 做 exactly-once wake clamp。

### EEVDF-005：switch-in 记账线性化点必须明确

**状态：** Neutralized

**修复落点：**

- 阶段 1A 在 `RunQueue::pick_next_task()` / `RunQueue::set_next_task(task, now)` 上建立 class switch-in transaction surface。
- 阶段 1B 在 scheduler loop 和 processor facade 中固定顺序：`local_pick_next()` 先调用 `RunQueue::pick_next_task()` 清 `on_runq`，再调用 `RunQueue::set_next_task(task, Instant::now())`；scheduler loop 随后执行 `switch_mapping(prev, next)`，再进入 `switch_to(next)`；`switch_to()` 内执行 `Task::on_switch_in()`、`set_current_task(Some(task))` 和 architecture context switch。

**Source audit:** `schedule_inner()` 只有需要真正切换的分支才走 `switch_out()`，随后回到 scheduler loop 选择 next task；`AbortWaitSleep` no-switch abort 和 `DeferredPreempt` 都提前返回，不调用 `switch_out()`、`local_pick_next()` 或 `set_next_task()`。idle fallback、yield、preempt、blocked 和 zombie 后的 next selection 都复用同一 `local_pick_next()` 路径。

**结论：** 所有真正切换到 next task 的路径都经过 `set_next_task(task, now)`，且该落点位于 mapping 准备、`Task::on_switch_in()`、`set_current_task()` 和 architecture switch 之前。no-switch abort 和 deferred preempt 不开启新的 execution segment。

### EEVDF-019：preempt reason 不能被当前 `need_resched` bool 静默压扁

**状态：** Neutralized

**修复落点：**

- [不变量需求](./invariants.md) 和 [迁移实施计划](./implementation.md) 决策为 processor / scheduler-core 私有 `PendingResched` flags。
- 单次请求原因是 `ReschedCause::{Tick, RunnableArrival}`，pending request 合并而不是覆盖。

**结论：** `DeferredPreempt` 必须让执行 `take_pending_resched()` 的 caller 恢复同一组 pending bits；scheduler class 不保存或 restore pending state，只在 `requeue_preempted_current(task, now, pending)` 中按值读取 flags。wake / new placement 语义不从 pending cause 推导，而由 `enqueue_woken()` / `enqueue_new()` 和 `decide_preempt_current()` 负责。

### EEVDF-021：bootstrap / kthread 进入 EEVDF 前必须证明 scheduler-critical progress

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 明确 bootstrap task、`kthreadd` 和普通 kthread 第一版直接使用 normal EEVDF，只承诺有限 runnable 集合中的 eventual scheduler progress。
- [不变量需求](./invariants.md) 增加 bootstrap / kthread progress 边界：不通过隐式 RR 例外、特殊优先级或单独 class 补齐证明。
- [迁移实施计划](./implementation.md) 阶段 3 只验证 fresh normal entity 分类和无 production RR 特例；basic boot / focused smoke 是 sanity validation，不是本 issue 的契约决策入口。

**反馈相关：** 若 source audit 或实现事实证明 timer worker、OOM worker、`kthreadd` 或其它 service kthread 需要 bounded latency、emergency priority 或单独 scheduler class，必须停止阶段 3 并回到 RFC review；不能在 default switch 中保留隐式 RR 例外。

**结论：** 这些内核线程第一版直接进入 EEVDF 已足够；本 RFC 的证明目标是 normal EEVDF eventual progress，不承诺 service-thread bounded latency。wait-core progress、deferred disposal、IRQ-off allocation 和 long non-preemptible path 风险按对应 owner / register 路由。
