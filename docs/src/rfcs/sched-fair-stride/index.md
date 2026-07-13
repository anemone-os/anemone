# RFC-20260713-sched-fair-stride

**状态：** Accepted / Implementing
**负责人：** doruche, Codex
**最后更新：** 2026-07-13
**领域：** scheduler / fair class / stride / nice / runqueue
**事务日志：** [2026-07-13-sched-fair-stride](../../devlog/transactions/2026-07-13-sched-fair-stride.md)
**开放问题：** None；已关闭问题与依据见 [Tracking Issues](./tracking-issues.md)。
**下一步：** Checkpoint 1 已关闭；按事务日志进入 Checkpoint 2 的 compile-time Fair default cutover gate。

## 摘要

本 RFC 定义 Anemone 第一版 `Fair` scheduler class，并选择经典 Stride 作为当前唯一 Fair backend。`Fair` 是稳定的 scheduler class 身份，`Stride` 是本次具体算法；生产代码以编译期别名把 `Stride` / `StrideEntity` 接到 `Fair` / `FairEntity`。未来只有在第二个 Fair backend 已经具备独立 RFC、实现和验收证据后，才增加编译期 `sched_fair_policy` selector；本 RFC 不引入运行时 backend enum、动态策略切换或混合 Fair entity。

Stride 以一个 scheduler timer tick 作为固定 service unit。每个 Fair task 维护单调 `pass`，每消费一个 tick 就按 Linux nice weight 的倒数推进；ready queue 使用按 `(pass, enqueue_seq)` 排序的最小堆。持续 runnable、fixed-nice 的有限任务集合中，最小 pass 选择提供确定性的比例公平和 eventual progress。sleep/wake、显式 yield 和跨 class 抢占使用单独的有界放置规则，不引入 EEVDF eligibility、deadline、lag 或 FairClock。

## 背景

当前 scheduler class graph 已落地 method-first `Scheduler` trait、`RunQueue` facade、class-specific `SchedEntity` payload、typed `PendingResched` 和 capability-gated entity mutation。当前 production graph 只有 `Realtime > Idle`；所有 fresh non-idle task 通过 `SchedEntity::new_default()`，再由编译期 `sched_default_policy = "rt_rr" | "rt_fifo"` 选择 RT policy。

RT class 已是独立、正式的 scheduler class，但第一版不支持已发布 task 的 class、policy 或 priority 修改。Fair class 沿用同一边界：本 RFC 只增加编译期 default policy 和固定 class payload，不实现运行期 class migration 或调度属性事务。

[旧 EEVDF-lite RFC](../sched-eevdf-lite/index.md) 已因 Stage 3 / R1 runtime acceptance failure 延期关闭。其失败说明 eligibility、competition membership、sleep/wake lag 和 yield self-pick 组合会迅速扩大协议和热路径风险。本 RFC 不修补或裁剪旧 EEVDF 实现，而是独立建立一个没有 eligibility/deadline 的 Stride Fair class。旧 EEVDF 源码继续作为归档材料，不进入本 RFC 的 production graph 或 write set。

本 RFC 已经达成以下共识：

- Stride 使用固定 quantum；一个 timer tick 就是一个 service unit。
- Fair nice-to-weight 使用 Linux 40 项固定 weight 表。
- 有其它 Fair peer 时，`sched_yield()` 和内核 `yield_now()` 都必须至少让出一次；两者当前共享同一 `ScheduleMode::Yield`，本 RFC 不人为区分来源。
- 当前只能在编译期选择唯一 default scheduling policy；运行期调度策略和属性修改属于后续 RFC，并必须由 owner-CPU transaction 原子完成。
- `sched_default_policy` 选择稳定 policy/class，值应为 `fair`、`rt_rr` 或 `rt_fifo`；不能用 `stride` 替代稳定的 `fair` 名称。
- 在第二个可用 Fair backend 出现前，不增加 `sched_fair_policy`。

## 目标

- 引入稳定的 `Fair` scheduler class identity，并把当前实现定义为 `Fair = Stride`。
- 将集中 class precedence 扩展为 `Realtime > Fair > Idle`。
- 为 Fair task 增加 class-private `StrideEntity`，只用 `Option<u128>` 保存 fresh/placed pass 状态。
- 使用固定 Linux nice weight 表，把 `Task::nice()` 作为 Fair weight 的唯一长期来源，不缓存第二份 nice 或 weight。
- 使用一个 timer tick 作为固定 service unit；current 每消费一个 tick，`pass` 推进一次。
- 使用最小堆按 `(pass, enqueue_seq)` 选择 runnable Fair task；相同 pass 下保持 FIFO tie-break。
- 为 fresh、wake、yield、preempt、handoff、block、exit、tick、pick 和 arrival decision 定义完整的 method-first transaction 语义。
- 用只服务 admission 的 `placement_floor` 防止 fresh/woken task 从陈旧低坐标获得无界 catch-up；它不得参与 eligibility 或 pick。
- 有 Fair peer 时，显式 yield 必须通过 pass/queue order 自然保证至少一个 peer 先运行，不增加 one-shot skip flag 或特殊 pick 分支。
- 将 `sched_default_policy` 扩展为 `fair | rt_rr | rt_fifo`，并把 repository default 切换到 `fair`；所有选择仍只在编译期生效。
- 保持 RT/FIFO、RT/RR selector build 和现有 RT class 语义不变。
- 通过理论/KUnit关闭 heap order、pass arithmetic、yield handoff、wake placement 和 fixed-set progress；通过用户运行 `fair-test` 与真实 LTP workload 验证 production 接线和无整体回归。

## 非目标

- 不实现运行期 scheduler class、policy、RT priority 或 Fair backend 修改。
- 不实现 `sched_setscheduler()`、`sched_setparam()`、`sched_setattr()` 或对应 getter/permission ABI。
- 不为现有 published-task weak nice setter增加 owner-CPU linearized renice transaction、queued remove-update-reinsert、current segment 原子切分、历史 pass 重算或动态公平证明；这些属于后续调度属性 RFC。
- 不实现实际纳秒 runtime accounting、CFS-like vruntime、EEVDF eligibility/deadline、sleep lag、latency nice 或 cgroup scheduling。
- 不保证频繁 sub-tick block/wake task 与持续 runnable task 之间的精确 CPU-time 比例；第一版公平证明只覆盖 fixed-nice、持续 runnable 竞争集合。
- 不增加 sleeper bonus。wake 只消除陈旧 credit，不把 task 放到当前 placement floor 之前。
- 不实现跨 CPU migration、load balance、scheduler domain、CPU hotplug 或 remote runqueue inspection。
- 不实现 RT bandwidth、Fair bandwidth、throttling 或跨 class share。
- 不修改 wait-core identity、stale-safe wake、pending-resched、trap/IPI 或 schedule entry contract。
- 不把 IRQ-off allocation 作为本 RFC 的算法选择 blocker；相关风险由 [ANE-20260622-IRQ-OFF-HEAP-ALLOCATION](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation) 跟踪，本 RFC 只沿用、不宣称修复。
- 不把第一版 `dequeue()` 优化为 intrusive/indexed heap；常见 pick/requeue 为 `O(log n)`，任意 task dequeue 可以先使用线性定位和 heap rebuild。
- 不把 archived `eevdf.rs` 重新接入 production，也不把 EEVDF 旧字段/测试迁移进 Stride。
- 不在只有一个 Fair backend 时增加 `sched_fair_policy`、runtime backend tag、delegating wrapper 或第二套 Fair trait。

## 文档地图

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)
- [Tracking Issues](./tracking-issues.md)

背景材料：

- None

## 方案

### 稳定 Fair 身份与当前 Backend

第一版目录形状为：

```text
sched/class/
  fair/
    mod.rs
    stride.rs
  rt.rs
  idle.rs
  entity.rs
  runqueue.rs
  mod.rs
```

`fair/mod.rs` 拥有稳定 Fair 语义和当前 backend 选择点：

```rust
mod stride;

pub(super) use stride::{Stride as Fair, StrideEntity as FairEntity};
```

`Stride` 实现现有 `Scheduler` trait，并令 `Scheduler::KIND == SchedClassKind::Fair`。共享层只看见：

```rust
SchedClassKind::Fair
SchedClassPrv::Fair(FairEntity)
RunQueue::fair: Fair
```

不得增加 `SchedClassKind::Stride`。未来切换 Fair backend 不应修改 class precedence、`RunQueue` dispatch、`SchedEntity` variant、scheduler core 或观察面。

当前只有 Stride backend，因此不增加 `sched_fair_policy`。未来若存在另一个已验收 backend，selector 必须在编译期选择唯一 `Fair` / `FairEntity` alias；不得让单个 kernel 在运行时持有多种 Fair backend 或为此增加逐 transaction 的动态分发。

### 编译期 Default Policy

现有 `sched_default_policy` 扩展为：

```toml
sched_default_policy = "fair" # fair | rt_rr | rt_fifo
```

`SchedEntity::new_default()` facade 拥有全局 selector 分发：

- `Fair`：构造 fresh `SchedClassPrv::Fair(FairEntity)`；
- `RtRr`：请求 RT owner 构造 fresh minimum-priority RT/RR payload；
- `RtFifo`：请求 RT owner 构造 fresh minimum-priority RT/FIFO payload。

共享 facade 只选择 class/policy，不实现 Stride arithmetic 或 RT policy state。Fair/Stride 与 RT 模块分别拥有 opaque payload factory 和 validation。ordinary task、clone child、bootstrap task、`kthreadd` 和普通 kthread 继续只调用 `new_default()`；idle task 继续只调用 `new_idle()`。

本 RFC 把 repository default 改为 `fair`，但保留 `rt_rr` / `rt_fifo` selector build 作为 RT 回归。每个 build 中，所有 fresh non-idle production task 只有一种编译期 default policy；不存在 published-task policy migration。

### Nice Weight

Fair class 复用 Linux `sched_prio_to_weight[]` 的 40 项固定表，覆盖 `Nice[-20, 19]`，其中 nice 0 weight 为 1024。weight 表属于稳定 Fair 语义，不属于 Stride heap mechanics；当前可以由 `fair/mod.rs` 提供 class-private lookup，未来其它 Fair backend 必须复用同一映射，除非独立 RFC 明确修改 Fair nice contract。

`Task::nice()` 是唯一长期 nice truth。`StrideEntity` 不保存 `cached_nice` 或 `cached_weight`。每个推进 pass 的 class transaction 只读取一次 typed nice：timer tick 与有 peer 的 charged yield 都使用该次 observation 计算 stride delta。现有 published-task `set_nice()` 只保持既有 weak observation；与 update 竞争的 charge 可以观察旧值或新值，首次观察到新值的 charge 使用新 delta，但本 RFC 不承诺观察延迟上界、renice 线性化、queued pass/key 追溯更新或 current segment 切分。

公平证明要求观察区间内 task nice 固定。`fair-test` 的 nice-direction worker 在 measurement barrier 前完成 `setpriority()` / `getpriority()`，测量区间不再修改 nice；该 case 只验证 fixed-nice direction 和 production integration，不验收动态 renice、精确 share 或线性化语义。

### Pass Arithmetic

第一版使用 normalized fixed-point pass：

```text
NICE_0_WEIGHT = 1024
PASS_SCALE    = 1 << 32

stride_delta(weight) =
    ceil(PASS_SCALE * NICE_0_WEIGHT / weight)
```

因此 nice 0 每个 service unit 精确推进 `PASS_SCALE`。所有合法 weight 都产生正 delta；高 weight 推进较慢，低 weight 推进较快。乘法、ceil division 和累加使用 `u128` checked arithmetic。`PASS_SCALE` 是内部数值精度，不是可调度策略，不进入 Kconfig。

`pass` 与 `placement_floor` 使用 `u128`。在 `SYSTEM_HZ` 的现有类型上界和最小 Linux weight 下，耗尽 `u128` 需要远超可运行系统寿命；实现仍必须使用 checked addition 暴露不变量破坏，不得 silent wrap、saturate 后继续或增加改变顺序的 fallback。

### Entity 与 Class State

建议形状：

```rust
struct StrideEntity {
    pass: Option<u128>,
}

struct Stride {
    ready: BinaryHeap<ReadyEntry>,
    current: Option<Weak<Task>>,
    placement_floor: u128,
    next_enqueue_seq: u128,
}

struct ReadyEntry {
    pass_snapshot: u128,
    enqueue_seq: u128,
    task: Arc<Task>,
}
```

`StrideEntity::new_fresh()` 只创建 `pass: None`、未入队 payload。`enqueue_new()` 是唯一允许的 `None -> Some(placement_floor)` 转换；它必须常开断言输入为 `None`。状态没有 `Some -> None` 转换，`enqueue_woken()` 与所有 queued/current/lifecycle/arrival transaction 都必须常开断言所需 entity 已为 `Some(pass)`。clone 只能继承 nice，不得复制 parent pass、queue snapshot、sequence、current identity 或 `on_runq`。

`current` 是 class-active execution segment 的 protocol identity，不是诊断字段。`set_next_task()` 建立它；yield、preempt、handoff、block 和 exit transaction 在重新入队或离开前清除它。`pick_next_task()` 要求没有旧 current。weak reference 只识别 active task，不拥有 task 生命周期。

`SchedEntity::on_runq` 继续由 `RunQueue` 唯一发布，但它只要求在完整 `RunQueue` transaction 的入口和出口与 class-local physical membership 一致，不是 class dispatch 每个中间步骤的同步镜像。enqueue/requeue 先由 Fair class 完成入堆，返回 `RunQueue` 后再发布 `on_runq = true`；pick/dequeue 先由 Fair class 移除 entry，返回 `RunQueue` 后再清除 `on_runq`。这些中间态始终位于同一 owner-CPU IRQ-off transaction 内，不允许 admission、callback 或其它观察者介入，也不得通过增加第二个 membership flag 来“修复”短暂差异。

### Ready Heap 与 Tie-Break

ready heap 的唯一行为排序是：

```text
(pass_snapshot ascending, enqueue_seq ascending)
```

Rust `BinaryHeap` 可以通过 reversed/custom order 实现 min-heap。`pass_snapshot` 是性能所需的稳定排序快照：queued task 的 entity pass 在出队前不得变化，pop/dequeue 时必须用常开断言验证 snapshot 与 entity pass 一致。heap entry 不得成为第二份可独立修改的 pass truth。`on_runq` 与 heap membership 的一致性在完整 `RunQueue` transaction 边界检查；class-local enqueue/pick/dequeue 中间态按上一节的发布顺序处理。

每次实际入堆分配新的单调 `enqueue_seq`。相同 pass 下，先入队者先运行；yielding current 重新入队时取得晚于现有 peer 的 sequence。sequence 使用 checked `u128`，不得 wrap 后破坏 order。

第一版不增加 entity heap index。pick/requeue 使用 heap 原生 `O(log n)`；`dequeue(task)` 可以线性定位目标 entry 后 rebuild heap。若未来真实 caller 或 profile 证明任意 dequeue 是热点，再以独立行为保持型优化 gate 引入 indexed/intrusive heap。

### Placement Floor 与 Wake

`placement_floor` 是 Stride class-owned admission clock。一个完整 lifecycle transaction 的最终 visible Fair set 是 ready heap 与 class-active current 的并集。floor 只能在 transaction 完成后按最终状态刷新：visible set 非空时，先常开断言其最小 pass 不低于旧 floor，再把 floor 推进到该最小值；visible set 为空时保留最后值。不得用 `max(old_floor, min_visible)` 静默掩盖低于 floor 的 entity。

中间的 pop、clear-current 或尚未完成的 requeue 不得刷新 floor。`pick_next_task()` 只 pop/验证且保持 floor 不变；现有 owner-CPU IRQ-off full-pick transaction 随后必须无 admission、无 callback 地调用 `set_next_task(selected)`，由 set-next 建立 current 后再按 selected/current 与 remaining heap 刷新。preempt、handoff 和 yield requeue 也必须先完成 pass 更新、clear-current 与入堆，再按最终 post-state 刷新；不增加 selected/in-flight 字段或新的 scheduler-core state。

放置规则：

```text
fresh enqueue: entity.pass = Some(placement_floor)
wake enqueue: entity.pass = Some(max(old_pass, placement_floor))
```

这样 new task 不从零追赶历史 service，sleeping task 也不能携带低于当前竞争坐标的陈旧 credit。高于 floor 的已有 debt 被保留。`placement_floor` 只允许被 fresh/wake placement 消费；它不得筛选 eligible task、改变 heap pick 或制造 no-eligible fallback。

### Tick 与比例公平

一个 scheduler timer tick 是一个完整 Stride service unit。`task_tick(current)`：

1. 读取 current 的固定区间 nice weight；
2. 把 current pass 推进一个 `stride_delta`；
3. 刷新 `placement_floor`；
4. ready heap 非空时返回 `RequestResched`，否则返回 `None` 并让 current 继续运行。

如果 kernel preemption 延迟 full pick，后续每个 timer tick 仍各自推进一次 pass。`PendingResched::Tick` 是合并 latch，不承担 service accounting truth；延迟消费不能丢失期间已经发生的 tick charge。

持续 runnable、fixed-nice 的有限集合中，每次从最小 pass 选择并按 weight 倒数推进。所有 delta 为正，因此任何未运行 task 的 pass 最终都会成为最小值；任务不会稳定饿死。长期 service-unit 比例近似 Linux weight 比例，误差来自固定点 ceil rounding，并由 `PASS_SCALE` 控制。

频繁 sub-tick block/wake task 不按实际运行纳秒收费。第一版明确不证明它与 CPU-bound task 的精确 CPU-time share；wake floor 只阻止无界陈旧 credit，不把 Stride 扩张成 runtime-accounted CFS/EEVDF。

### Yield

现有用户 `sched_yield()` 与内核 `yield_now()` 共享同一 class transaction。`requeue_yielded_current()` 在 ready heap 有 Fair peer 时只读取一次 `Task::nice()`，并执行：

```text
charged = current.pass + stride_delta(current.weight)
current.pass = max(charged, min_peer.pass_snapshot)
enqueue current with a fresh enqueue_seq
```

因此 peer 的 pass 严格更小，或 pass 相等但 sequence 更早，下一次 Fair pick 必然先选择某个既有 peer。该保证不依赖特殊 pick mode、skip-current flag、yield target 或 task 类型判断。

没有 Fair peer 时，不修改 current pass，只重新入队并由正常 pick 选回。若同时有 RT task，集中 precedence 先选择 RT；yield 后的 Fair heap order保持不变，RT 队列耗尽后既有 Fair peer 仍排在 yielding task 前。

yielding task 可能主动放弃积累的 service credit，这是显式 yield 的接受语义。保证是“至少一个既有 Fair peer 先运行”，不是“恰好只让出一个 peer”或“保持 yielding task 原有 lag”。

### 其它 Lifecycle Transaction

- `requeue_preempted_current()`：清除 current 后按现有 pass 入堆，最后按完整 post-state 刷新 floor，不额外收费。Tick transaction 已完成对应 charge；RunnableArrival cross-class preempt 不能伪造一个 Fair service unit。
- `handoff_woken_current()`：清除 current 后按现有 pass 入堆，最后按完整 post-state 刷新 floor，不额外收费。
- `put_prev_blocked()`：清除 current，刷新 placement floor，不入堆。
- `put_prev_exiting()`：清除 current，刷新 placement floor，不入堆。
- `pick_next_task()`：从 heap 取最小 entry，验证 snapshot，返回 task；不修改 pass，也不刷新 floor。
- `set_next_task()`：常开断言 selected task 为 `Some(pass)` 且已出 heap，建立唯一 class-active current，然后按完整 post-state 刷新 floor；不预收下一 tick。
- same-Fair `decide_preempt_current()`：常开断言 current 为 active、`Some(pass)` 且未 queued，candidate 为 queued、`Some(pass)`；普通 arrival 保持 current，下一 timer tick 在存在 peer 时产生 Tick resched request，实际 full pick 仍服从 scheduler core 的 deferred-preempt 语义。
- cross-class arrival：继续由集中 `Realtime > Fair > Idle` precedence 决定。

wait-core no-switch abort 不调用 class transaction。parked wake handoff 只走现有 `handoff_woken_current()`；Stride 不读取 wait identity、park state 或 wake token。

## 验证边界

理论和 focused KUnit 负责关闭：

- Linux nice table 完整性、正 weight 和 nice 方向；
- nice 0 stride 精确值、所有 delta 非零、ceil rounding 和 checked arithmetic；
- heap `(pass, sequence)` order、合法 queued snapshot/entity 一致性与 transaction-boundary membership；
- equal-weight RR-like order；
- unequal-weight fixed-set service ratio和 eventual progress；
- fresh/wake placement 与 empty-set floor persistence；
- `0, 100` pick/set-next gap 不推进 floor，随后 fresh/wake placement 不跳到 `100`；
- `current = 0, peer = 100` 的 preempt/handoff 只按最终 post-state 刷新，入堆后 floor 仍为 `0`；
- `pass: Option<u128>` 的合法初始化与 placed lifecycle，确认唯一初始化路径是 `enqueue_new()`；
- queued weak nice update 不修改 pass/key，后续 tick 与 charged yield 从各自单次 observation 计算 delta；
- tick delayed-pick 下每 tick 只收费一次；
- yield 有 peer时必然 handoff、无 peer时不惩罚；
- RT arrival 抢占 Fair、Fair arrival 抢占 Idle、same-Fair arrival 不立即抢占；
- lifecycle transaction 不重复收费、不遗留 current、不制造 duplicate membership。

source audit 负责关闭：

- `Fair` 是 stable class identity，production graph 无 `SchedClassKind::Stride`；
- Fair backend 只有一个编译期 alias，无 runtime tag/wrapper；
- `Task::nice()` 是唯一 nice truth，production Stride entity 无 cached nice/weight；
- `fair-test` nice-direction 只在 barrier 前设置 nice，measurement interval 保持 fixed-nice；
- heap entry pass 只作 queued immutable snapshot；
- duplicate new、fresh wake/set-next、未初始化 arrival 与 snapshot mismatch 都有常开断言；这些 expected-panic contract 不进入普通 KUnit，因为当前 KUnit 不支持 unwind；
- `on_runq` 只在完整 `RunQueue` transaction 边界与 class-local heap membership 一致，enqueue/requeue 与 pick/dequeue 的发布顺序没有被倒置或复制成第二份状态；
- ordinary task、clone、bootstrap 和 kthread 只使用 `new_default()`；
- default selector 只有 `fair | rt_rr | rt_fifo`，没有当前无意义的 `sched_fair_policy`；
- archived EEVDF 不进入 production module graph。

runtime 验证负责确认：

- agent 只负责 Fair default kernel 的 focused KUnit / pretest boot、build 与 source audit，不运行 Fair/RT-RR whole-profile 对比；
- 用户运行 `fair-test`，确认 equal-nice、fixed-nice direction、yield 和 sleep/wake 四组 workload 通过；nice-direction 的阈值不作为动态 renice、精确 share 或线性化 proof；
- 用户在同一 checkout、相同平台/config/rootfs/test image、相同 LTP profile/case-set 和相同测量区间下分别运行 `rt_rr` baseline 与 `fair`，并裁定 Fair 是否保持与 RT/RR 同一量级、没有稳定的多倍整体吞吐退化；
- RT/RR 与 RT/FIFO selector build 继续通过，RT class 没有被 Fair 接线破坏。

本 RFC 不用 runtime benchmark 替代理论公平证明，也不把单次耗时波动写成严格性能回归。RT/RR 对比由用户运行和裁定，agent 不得把未提供的结果写成通过，也不预设一个未经用户接受的百分比 SLA；若用户报告重复、稳定的多倍整体吞吐退化，必须停在当前 gate，按 tick charge、yield handoff、heap order、placement floor 或 integration owner 分类，不能靠调参掩盖。

## 接受边界

本 RFC 被接受意味着：

- `Fair`/`Stride` 身份、状态所有权、pass arithmetic、heap order、placement、yield、tick 和 lifecycle contract 已闭合；
- `sched_default_policy = "fair"` 的 owner 与 RT 回归边界已闭合；
- runtime acceptance 使用同 checkout RT/RR 作为用户侧 A/B baseline，agent/user 责任和结果回写边界已闭合；
- implementation 可以按 [迁移实施计划](./implementation.md) 的 checkpoint 推进，并在实现开始时建立公开 transaction devlog；
- fixed-tick、fixed-nice fairness 和 sub-tick sleeper 非精确 accounting 是明确边界，不得在实现中私自扩张为 runtime-accounted scheduler。

以下变化必须停止当前 gate并回到 RFC review：

- 需要修改 scheduler core、wait-core、pending-resched、trap/IPI 或 `Scheduler` trait 才能表达 Stride；
- 需要把 pass、weight、current 或 membership 复制到新的长期真相源；
- 需要运行时 backend dispatch、published-task class migration 或调度属性事务；
- 需要 eligibility、deadline、sleep lag 或其它 EEVDF/CFS 机制才能通过验收；
- 需要降低 Fair/RT/Idle precedence、隐藏 yield self-pick 或削弱 fixed-set progress 才能通过 runtime gate。

## 备选方案

### Nice-aware RR

未选择。把 Linux nice weight 映射为时间片长度会在极端权重下产生过长/过短 quantum；clamp 后又不再保持比例语义。若改为 credit/deficit RR，状态和证明接近 Stride，却不如最小 pass 模型直接。

### Runtime-accounted min-vruntime

延期。按实际纳秒推进 weighted virtual runtime 能更精确处理 sub-tick block/wake，但需要 execution-segment accounting、分段不变 remainder 和更复杂的 wake/renice contract。本 RFC选择固定 tick Stride，不能在实现中悄然演化为 CFS-lite。

### 修复并恢复 EEVDF

延期。旧 RFC 的 eligibility、membership、lag、remainder 和 rebase 问题尚未关闭。未来 EEVDF 若独立修复并验收，可以作为另一个 Fair backend 候选，但必须先经过自己的 RFC/review/runtime gate，再引入 `sched_fair_policy`。

### Runtime Fair Backend Wrapper

未选择。当前只允许编译期唯一 backend；wrapper 会增加无用途的 runtime tag、逐 transaction 分支和混合 payload 可能性。

### BTree / Indexed Heap

第一版不选择。`BTreeMap` 或 intrusive/indexed heap 可以改善任意 dequeue，但会增加 queued key/index protocol。本轮先用最小堆覆盖常见 pick/requeue，线性 dequeue 保持接口完整。

## 风险

- Linux nice 极端 weight 比约 5917:1，低 weight task 的合法 service 间隔可能很长。控制方式是把它作为用户已接受的比例语义，用 fixed-set KUnit 证明 eventual progress，不擅自压缩 weight 表。
- tick 只近似 CPU service，sub-tick block/wake 可能获得相对响应性优势。控制方式是限定公平证明范围和 wake floor，不加入未经批准的纳秒 accounting。
- heap snapshot 与 entity pass 可能失配。控制方式是 queued pass immutable、owner-CPU noirq transaction 和 pop/dequeue 常开一致性断言。
- yield clamp 会让 task 主动放弃较多 service credit。控制方式是只在存在 Fair peer 的显式 Yield transaction 中执行，并只提升到最小 peer pass。
- pass/sequence 理论上可能溢出。控制方式是 `u128`、checked arithmetic 和静态上界审计，不使用 silent wrapping/saturation。
- class graph 和 default selector 改动可能破坏已经验收的 RT class。控制方式是分 checkpoint 接线、三种 selector build、集中 precedence KUnit 和 RT source audit。
- 当前 IRQ-off queue allocation 限制继续存在。它是由 [ANE-20260622-IRQ-OFF-HEAP-ALLOCATION](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation) 跟踪的共享基础设施债务，不改变本 RFC 的算法选择或验收归属。

## 收口

本节在实现完成后填写。RFC 已接受进入实现并建立 transaction devlog；阶段 0 与 Checkpoint 1 已按 canonical gate 关闭，执行与验证事实见事务日志。Checkpoint 2-3 尚未完成，不在本节提前写成实现收口。
