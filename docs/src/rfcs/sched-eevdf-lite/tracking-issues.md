# sched EEVDF-lite tracking issues

**状态：** Closed with unresolved Keter
**最后更新：** 2026-07-12
**父 RFC：** [RFC-20260622-sched-eevdf-lite](./index.md)
**事务日志：** [2026-07-09-sched-eevdf-lite](../../devlog/transactions/2026-07-09-sched-eevdf-lite.md)
**来源：** sched-split-aware v2 重写 / method-first scheduler class 纠偏 / 2026-07-07 文档层 review / 2026-07-11 Stage 3 runtime feedback / 2026-07-12 R1 runtime failure

本文保留 design review 后确认的 sched EEVDF-lite 草案缺陷、证明缺口、边界冲突或会影响未来实现顺序、review gate、停止边界和验收判断的设计问题。父 RFC 已延期关闭，但未解决问题不会随之 Neutralized；`EEVDF-001` / `EEVDF-018` / `EEVDF-004` / `EEVDF-020` 继续保持 Keter，直到未来重新打开 RFC 并以新批准的证据关闭。

普通实现进度、TODO、benchmark 数字、用户侧长日志和阶段性交付项不写入本文；它们属于 [RFC index](./index.md) 的背景、非目标、风险，或 [迁移实施计划](./implementation.md) 的阶段内容。受控实现反馈不新建通用 feedback 文件；计划写在 [迁移实施计划](./implementation.md#probe--vertical-slice-gates)，执行结果进入 transaction devlog。若反馈暴露目标、不变量、owner boundary 或接受边界需要改变，必须回写 RFC canonical 文本和本文对应 issue。

分级沿用 Anemone review 口径：

- **Apollyon**：当前必须修复的错误结果、数据损坏、安全问题、崩溃或严重不可恢复状态。
- **Keter**：会阻塞后续实现方向或导致核心抽象不可复审，必须修正或明确改边界。
- **Euclid**：值得修正，但通常不阻塞第一版实现。
- **Safe**：记录即可，除非顺手修正。

## Apollyon

- 暂无。

## Keter

### EEVDF-001：eligibility 必须使用 weighted FairClock

**状态：** Keter

**触发证据：** [Stage 3 eligibility 与整体吞吐回归证据](./backgrounds/stage3-eligibility-regression-20260711.md) 记录了相同 signal profile 中 `1,338,814` 次 min-floor `self_only_eligible` self-pick；同一 snapshot 的 weighted-fair-clock counterfactual 显示 `552,494` 次已有 eligible peer。R1 随后把 actual path 替换为 weighted FairClock，但用户运行仍有 `1,233,143` 次 yield self-pick 与 `1,232,735` 次 weighted `self_only_eligible`。此前基于 source audit / focused KUnit 的 Neutralized 结论没有覆盖 default EEVDF 下的 runtime failure，R1 也未关闭主要吞吐因果归属。

**问题：** monotonic minimum-`vruntime` floor 把本应参与竞争的 peer 排除在 eligible set 外，并形成 same-task yield feedback。现有 `NoEligibleTask` anomaly 不会触发，因为 yielding task 本身仍 eligible。继续把该 floor 当作公平时钟会把核心算法和后续 lag / placement 设计带到错误方向。

**修复落点：** [RFC index](./index.md) / [不变量需求](./invariants.md) 已改为从 `C = ready union class-active current` 派生 weighted FairClock；[Gate R1](./implementation.md#gate-r1---direct-weighted-fairclock-repair) 先做单变量 eligibility intervention。禁止 forced handoff、skip-current、penalty tuning 或 testcase-specific yield 旁路。

**关闭条件：** R1 已命中“百万级重复 self-pick 仍主导”的失败信号，因此本 issue 在 Closed RFC 中保持未解决。未来只有先重新打开 RFC review、重新分类该 trajectory、批准新的 intervention 与 runtime acceptance，并用证据证明 eligibility 与主要失速反馈闭合后才可 Neutralized；不能只凭公式替换关闭。

### EEVDF-018：competition membership 必须区分 continuous transfer 与 true leave / join

**状态：** Keter

**触发证据：** weighted FairClock 需要一个没有 pick / set-next 空洞的完整 competition set；现有实现直到 `set_next_task()` 才安装 class current，且原 2D 把 `ParkPending` handoff 当成 wake placement。此前 Neutralized 只证明了 method 名称分流，没有证明 FairClock membership 或 service-lag lifecycle。

**问题：** yield、preempt 和 `ParkPending` handoff 都没有离开 competition set；把其中任一路径当成 true leave / join 会保存或恢复不存在的 lag，并使 ready / active snapshot 出现双重或缺失 truth。

**修复落点：** R2 建立 ready / active 互斥 membership，`pick_next_task()` 在 class 内完成 ready-to-active transfer，true block / wake 才执行 leave / join。当前 `Processor` 已知先 enqueue、后在 `decide_preempt_current()` accounting，不能满足最终同 snapshot 合同；R2 必须先做 1A / 1B method-contract review，并记录已批准的 write-set 扩展，不能在旧 surface 间制造第二套状态。

**关闭条件：** 本 RFC 关闭时 R2 未执行。未来重新打开后，须由重新批准的 source audit 与 focused KUnit 证明 ready / active 互斥、continuous paths 不保存 / 恢复 lag、true leave / join exactly once，generic `dequeue()` 已删除或分类为窄 transaction，且没有依赖 wait-core private identity 的第二真相源。

### EEVDF-004：true wake placement 必须恢复 service lag，`ParkPending` 不得获得 wake reward

**状态：** Keter

**触发证据：** 原 Neutralized 结论把 ordinary wake 与 parked-current handoff 都收敛为围绕 monotonic floor 的 bounded clamp。Stage 3 feedback 已否定该 floor；competition membership 分析进一步证明 parked current 从未离开 `C`，因此 handoff clamp 本身就是错误 lifecycle 分类。

**问题：** true block / wake 需要保存并恢复 weight-scaled service lag；`ParkPending`、abort、preempt 和 yield 是 continuous membership。继续复用同一个 wake clamp 会让 continuous path 获得凭空 credit，也无法证明 unequal-weight sleep/wake 的债权守恒。

**修复落点：** R2 使用 bounded exact-rational saved service lag、带 `W0` 补偿的 join placement 和有方向约束的整数误差；ordinary `WakeEnqueueResult::Enqueued` 消费 saved lag 一次，`ParkPending` handoff 只做 active-to-ready transfer。R1 的 `legacy_placement_floor` 仅为隔离变量的临时 bridge，R2 必须删除。

**关闭条件：** 本 RFC 关闭时 R2 未执行。未来重新打开后，exact-rational leave / join、unequal-weight non-integer round trip、`W0 == 0`、credit / debt clamp、ParkPending no-reward 和 stale / already-* 不消费 saved lag 均须有 KUnit / source proof；任何 checked representation fallback 命中都会让 gate 失败。

### EEVDF-020：virtual-time arithmetic、accounting 与坐标表示必须闭合

**状态：** Keter

**触发证据：** 原 Neutralized 只证明 `u128` 中间计算、每段最小推进和 `u64` saturation 可观测。新的 FairClock / lag contract 暴露出三项未闭合边界：每段 `max(1, floor(...))` 不具备 accounting 分段不变性；deadline 重置不保留跨多个 request 的 phase；`u64` saturation 后无法继续证明 deadline、lag 或 progress。

**问题：** transaction 频率不能改变公平账本，arithmetic anomaly 也不能成为长期算法状态。saved lag 还需要 checked exact-rational 表示，不能先量化或把 `u128` magnitude 强转为 `i128`。

**修复落点：** R2 关闭 exact-rational saved lag 与 representation failure；R3a 关闭 fixed-weight remainder、strict request catch-up 和 block / wake accounting continuity；R3b 关闭 proactive common coordinate rebase。dynamic renice 的 strong lag conservation 仍是独立 follow-up RFC / gate，不属于本次 R1-R3b 或阶段 4 收口，也不阻塞 R1-R3b。

**关闭条件：** 本 RFC 关闭时 R2 / R3a / R3b 均未执行，本 issue 保持未解决 Keter。未来重新打开后，三门或其替代 gate 的证据须全部满足，真实 workload 不命中 arithmetic fallback，common rebase 前后 eligibility、lag、deadline 差和 pick 结果等价。

## Euclid

- 暂无 active Euclid。

## Safe

- 暂无 active Safe。

## Neutralized

### EEVDF-022：deadline 续期不得吞掉 current request completion

**状态：** Neutralized

**问题：** `account_current(now)` 在推进 `vruntime` 后立即把 expired deadline 续为 `vruntime + slice`，但原实现只把 arithmetic saturation 带出 helper；`task_tick()` 随后重新检查 `vruntime >= deadline` 时，正常非 saturation 路径已经必然为假。`decide_preempt_current()` 也会在 runnable-arrival accounting 中吞掉同一 completion。renewal 还位于短路 `||` 的末项，前序 arithmetic saturation 为真时会跳过续期副作用。

**修复落点：** deadline renewal 返回正交的 `renewed` / `saturated`，`account_current(now)` 返回 class-private `AccountOutcome`。tick 在 completion 且存在其它 EEVDF runnable peer 时请求 resched；runnable-arrival 在 completion 或 candidate eligible 且 deadline 更早时请求 resched。已经承诺 switch / requeue / block / exit 的 transaction 显式丢弃 outcome；wake normalization 只返回 arithmetic saturation，不制造 running completion。实现没有新增 entity pending flag、processor-global 副本、shared trait 方法或新 `ReschedCause`。

**Source audit / validation:** `task_tick()` 与 `decide_preempt_current()` 分别经过可直接 KUnit 的 class-private production decision helper；所有 `account_current()` caller 都显式消费或丢弃 outcome，arithmetic 与 renewal 不再通过 effectful short-circuit 组合。rv64 端到端日志中 113 项 KUnit 全部通过，包含 completion + peer、completion + no peer、runnable-arrival completion、saturation renewal 和 wake normalization 分层；equal-weight、nice direction、bounded yield、sleep/wake 四组阶段 3 workload 均通过，测试区间无 `EEVDF anomaly`。read-write LTP 的 nonzero failure multiset 与修复前基线完全一致。

**Review / 结论：** 首轮独立 review 的唯一 Euclid 是测试未锁住 production decision consumer；修正后复审无 Apollyon / Keter / Euclid。本 issue neutralized，只证明 request-completion outcome 不会被续期吞掉。R3a 新增的 strict multi-request catch-up / phase preservation 属于 active `EEVDF-020` arithmetic gate，不把本 issue 的历史修复反向解释为完整 deadline closure。

### EEVDF-017：default class switch 必须被 blocker / gate 矩阵约束

**状态：** Neutralized

**历史修复落点：** `anemone-kernel/src/sched/class/entity.rs` 的唯一 default normal constructor 曾在阶段 3 翻转为 fresh `EevdfEntity`，无调用者的 directed `new_eevdf()` 已删除。R1 runtime acceptance 失败后，该 constructor 已恢复为 fresh RR entity；EEVDF implementation 保留为实验代码。

**Source audit / validation:** ordinary clone child、rv64 / loongarch64 bootstrap task、`kthreadd` 和 ordinary kthread 继续调用 `SchedEntity::new_normal()`；idle task 仍使用 `new_idle()`。当前 `new_normal()` 创建 RR，且默认 `user-test` / pretest rootfs 不再自动运行或安装 `eevdf-test`。

**结论：** 本 issue 的 Neutralized 只记录阶段 3 default switch 曾按 single-constructor contract 完成；它不再表示当前 production default 是 EEVDF。恢复 RR 是 RFC runtime failure 后的显式 closeout，不把四个未解决 Keter 伪装为已修复。

### EEVDF-002：runtime accounting 必须有单一幂等边界

**状态：** Neutralized

**修复落点：**

- `anemone-kernel/src/sched/class/eevdf.rs` 中的 EEVDF private `account_current(now)` 是唯一推进当前执行段的 helper。
- `set_next_task(task, now)` 只记录下一段 `exec_start`；`task_tick()`、`requeue_yielded_current()`、`requeue_preempted_current()`、`handoff_woken_current()`、`put_prev_blocked()` 和 `put_prev_exiting()` 均先调用同一个 helper。
- `anemone-kernel/src/sched/switch.rs` 明确 `Task::on_switch_out()` 只保留 task / CPU usage bookkeeping，不作为 EEVDF fair accounting truth。

**Source audit:** `DeferredPreempt` 与 wait no-switch abort 都在 `schedule_inner()` 中提前返回，不调用 `switch_out()`、`local_pick_next()`、`set_next_task()` 或任何 EEVDF class transaction；runnable requeue、parked handoff、wait park switch 和 exit switch 都在 `RunQueue` 设置 `on_runq = true` 或真正切走前完成 class transaction。`account_current(now)` 在成功推进后刷新 `exec_start = now`，tick 后的 switch-out / requeue 只结算 tick 之后的新执行段。

**结论：** Checkpoint 2B / Gate P1 的单一 accounting owner、call-site ordering 和 `exec_start` 刷新仍成立，本 issue 保持 Neutralized。fixed-weight remainder、block/wake continuity、deadline catch-up 和 coordinate representation 属于 active `EEVDF-020` / R2 / R3a / R3b；不得用本 issue 的 owner closure 代替这些 arithmetic evidence。

### EEVDF-003：schedule caller / pending resched reason 必须可传递

**状态：** Neutralized

**修复落点：**

- sched-split 的 scheduler-private wrapper 已取代裸 `schedule()` caller taxonomy。
- EEVDF-lite 的 class-visible 语义折回 method-first scheduler class transaction surface。

**结论：** EEVDF-lite 不重新引入公开 `ScheduleCaller` taxonomy，也不通过 catch-all event taxonomy 表达路径语义。

### EEVDF-006：weight source 只能作为内部 provisional contract

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 和 [不变量需求](./invariants.md) 明确 `Nice` newtype 约束值域，Task 内部受约束的原子 nice 表示是唯一长期 truth；`Task::set_nice(Nice)` 是已发布 task 唯一带明确退出条件的写入方法。
- EEVDF entity 不保存另一份 nice，也不在第一版保存 `cached_weight`。

**结论：** 第一版消费固定 Linux nice weight 表，但不承诺完整 priority syscall ABI 即时性。

### EEVDF-007：yield 需要独立算法语义

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) / [不变量需求](./invariants.md) 决策为 bounded yield penalty。

**反馈相关：** 具体 yield penalty 公式和 smoke 先归入 Gate R1 的 weighted-FairClock intervention；若 yield 反馈显示长期立即选回或饥饿，按 R1 failure signal 停止并回写 correction gate，而不是重新打开 event taxonomy。若异常实际来自 wake reward / no-reward 边界，路由到 Gate R2。

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
- Checkpoint 2A 建立 schema / generated plumbing；base slice、yield penalty window 和 anomaly threshold 的历史语义消费曾由 2C 覆盖，当前 correction 由 R1 / R3a 分别验证；wake clamp window 的旧语义由 2D 覆盖，当前由 R2 改义为 true-sleep service-credit bound。

**结论：** nice weight table 第一版固定 Linux 表，不提供 selector；未来替换权重表另走 follow-up。

### EEVDF-010：fallback anomaly 观察阈值

**状态：** Neutralized

**修复落点：**

- [RFC index](./index.md) 和 [迁移实施计划](./implementation.md) 明确 fallback anomaly 必须可观测。

**反馈相关：** 未来若重新打开 RFC，稳定 CPU-bound smoke 在 warm-up 后连续观察窗口仍增长 anomaly 时，必须按 anomaly 来源路由到新批准的 FairClock / membership / representation / accounting / coordinate gate；在归类前停止推进，不得执行 default switch。

**结论：** 每次 anomaly 记录通过 `kerrln!` 输出 reason 和累计次数；anomaly threshold 不再作为独立 active issue，只控制连续 fallback 的额外 streak 摘要，并继续作为 `EEVDF-001` / R1-R3b correction gates 的观察面。

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

**历史修复落点：** [RFC index](./index.md) 和 [迁移实施计划](./implementation.md) 曾决策直接以 EEVDF-lite 作为默认 normal scheduler 为目标。

**结论：** 未另建空调度框架；method-first scheduler class transaction surface 与 RR 行为保持适配作为通用基础保留。EEVDF default 目标已随 RFC 延期关闭，未来是否恢复必须重新 review。

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
- class-visible 语义通过 `enqueue_new()`、`enqueue_woken()`、`requeue_yielded_current()`、`requeue_preempted_current()`、`handoff_woken_current()`、`put_prev_blocked()`、`put_prev_exiting()`、`pick_next_task()`、`set_next_task()`、`task_tick()` 和 `decide_preempt_current()` 等 method-first transaction 表达。

**结论：** 阶段 1 source audit 禁止 scheduler implementation 引入 `SchedEvent` / `on_event` / event bus。

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

**历史修复落点：** RFC 曾要求 bootstrap task、`kthreadd` 和普通 kthread 直接使用 normal EEVDF，并只承诺有限 runnable 集合中的 eventual scheduler progress；阶段 3 曾验证 fresh normal entity 分类和无 production RR 特例。

**反馈相关：** 未来若重新打开 RFC，且 source audit 或实现事实证明 timer worker、OOM worker、`kthreadd` 或其它 service kthread 需要 bounded latency、emergency priority 或单独 scheduler class，必须停止 default switch 并回到 RFC review。

**结论：** 该 Neutralized 只保留历史 contract 决策，不描述当前分类。当前这些内核线程通过 `new_normal()` 进入 RR；未来若再次切换到 EEVDF，eventual-progress 与 service-thread latency 边界必须随重新打开的 RFC 一并复审。
