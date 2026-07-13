# Sched Fair / Stride 不变量需求

**状态：** Canonical
**最后更新：** 2026-07-14
**父 RFC：** [RFC-20260713-sched-fair-stride](./index.md)

## 闭合条件

第一版必须同时满足：

- `Fair` 是稳定 scheduler class identity，当前唯一 backend 是编译期别名 `Fair = Stride`。
- class precedence 只有一份集中真相：`Realtime > Fair > Idle`。
- `sched_default_policy` 只允许 `fair | rt_rr | rt_fifo`，repository default 为 `fair`。
- 在第二个可用 Fair backend 出现前，不存在 `sched_fair_policy`、runtime backend tag 或 backend wrapper。
- 一个 timer tick 是一个 Stride service unit；持续 runnable、fixed-nice 的有限 Fair 集合按 Linux nice weight 获得比例 service 和 eventual progress。
- ready queue 的唯一选择顺序是 `(pass, enqueue_seq)` 最小。
- `Task::nice()` 是唯一 nice truth；Stride 不保存 cached nice/weight。
- fresh/placed 只由 `pass: Option<u128>` 表达，唯一初始化转换是 `enqueue_new()` 的 `None -> Some(floor)`。
- fresh/wake task 不低于 class-owned `placement_floor`；floor 只按完整 transaction 的最终 post-state 刷新，不参与 pick 或 eligibility。
- 有其它 Fair peer 时，Yield transaction 后至少一个既有 peer 必须先于 yielding task 被 Fair pick。
- queued pass snapshot 与 entity pass 始终一致；在完整 `RunQueue` transaction 边界，current 与 queued membership 互斥且 `on_runq` 与 class-local heap membership 一致。
- 所有状态变化只发生在 owner CPU 的既有 method-first class transaction 中。

## 非目标

- 不证明动态 nice、class、policy 或 RT priority 修改；现有 weak nice setter 不构成 owner-CPU linearized renice transaction。
- 不证明 actual-runtime、sub-tick sleeper 或动态竞争集合的精确 CPU-time share。
- 不证明跨 CPU fairness、migration、load balance 或 bandwidth control。
- 不修改 wait-core、pending-resched、trap/IPI 或 schedule entry contract。
- 不证明 latency upper bound；Linux nice 极端权重可以产生较长合法 service 间隔。
- 不要求第一版任意 dequeue 为 `O(log n)`。

## 状态所有权

### Scheduler Core 与 Task

- `TaskSchedState` 继续唯一拥有 runnable / waiting / zombie 逻辑状态。
- `SchedEntity::on_runq` 继续由 `RunQueue` 唯一发布 owner CPU runqueue membership；它在完整 transaction 边界与 class-local physical membership 一致，不要求 class dispatch 的中间步骤同步翻转。
- `Task::cpuid()` 继续是 owner CPU 真相源。
- `Processor::pending_resched` 继续是等待下一次 full pick 的合并 latch；Stride 不保存 pending snapshot。
- `Task::nice()` 是 Fair weight 的唯一长期来源。现有 weak setter 不构成本 RFC 的调度属性 transaction；只有推进 pass 的 tick/charged-yield transaction 消费其单次 observation。
- `SchedClassKind` 从 opaque class payload 派生，只提供稳定 class identity；它不是可独立修改的 policy slot。

### Shared Class Domain

- `class/mod.rs` 唯一拥有 `Realtime > Fair > Idle` precedence。
- `RunQueue` 唯一拥有 per-CPU class instances、跨 class dispatch、`ntasks` 和 generic membership transition。
- `entity.rs` 只拥有 `SchedEntity` facade、opaque payload union、identity mapping 和 mutation capability。
- `fair/mod.rs` 拥有稳定 Fair backend alias 和 nice-to-weight contract。
- `fair/stride.rs` 唯一拥有 Stride pass arithmetic、heap、placement floor、current identity、sequence 和 focused KUnit。
- RT policy/priority/budget 继续只由 `rt.rs` 拥有；Stride 不读取或复制 RT state。

### Stride Entity

每个 Fair task 的 `StrideEntity` 只保存 `pass: Option<u128>`：`None` 表示 fresh、尚未由 owner CPU 放置，`Some(pass)` 表示已经拥有 Stride service coordinate。

- `StrideEntity::new_fresh()` 只能产生 `None`。
- `enqueue_new()` 是唯一初始化线性化点，必须常开断言 `None` 后设置 `Some(placement_floor)`。
- 不存在 `Some -> None` 转换；block、wake、preempt、handoff 和 exit 都不得重置 coordinate。
- `enqueue_woken()`、dequeue、pick、set-next、tick、yield、preempt、handoff、block、exit 和 same-Fair arrival decision 必须常开断言所需 entity 为 `Some(pass)`。
- clone 产生独立的 fresh `None`，只继承 nice，不复制 parent pass。

不得增加 cached nice、cached weight、deadline、eligibility、sleep lag、runtime timestamp、pending resched、heap index 或 backend tag，除非后续 RFC 改变 accepted contract。

### Stride Class

每个 CPU 的 `Stride` 实例拥有：

- ready min-heap；
- class-active current 的 weak identity；
- monotonic admission `placement_floor`；
- monotonic `next_enqueue_seq`。

这些字段是 protocol state。诊断计数不得反向驱动 pick、placement 或 pass progression。

## Class Identity 与编译期选择

稳定 class identity：

```text
SchedClassKind::Realtime
SchedClassKind::Fair
SchedClassKind::Idle
```

不得增加 `SchedClassKind::Stride` 或 `SchedClassKind::Eevdf`。具体 backend 只通过 `Fair` / `FairEntity` 编译期 alias 接入。

当前 selector：

```text
sched_default_policy = fair | rt_rr | rt_fifo
```

- selector 只影响 fresh non-idle entity 构造。
- 一个 build 中所有 production `new_default()` 调用得到同一种 policy。
- task 发布后，本 RFC 不允许改变 class/policy。
- `fair` selector 只构造 Stride-backed Fair entity；`rt_rr` / `rt_fifo` 继续构造 RT entity。
- 第二个 Fair backend 出现前，不得增加 `sched_fair_policy`。
- 未来 `sched_fair_policy` 也只能是编译期唯一 alias 选择，不能引入 runtime mixed backend。

## Nice Weight 与 Pass Arithmetic

Fair weight 使用 Linux 40 项固定表：

```text
nice range     = [-20, 19]
nice 0 weight  = 1024
all weights    > 0
```

Stride delta：

```text
PASS_SCALE = 1 << 32
delta(w)   = ceil(PASS_SCALE * 1024 / w)
```

必须满足：

- `delta(1024) == PASS_SCALE`；
- 任意合法 nice 的 delta 都大于零；
- weight 越大，delta 不增；
- 乘法、ceil adjustment 和累加使用 `u128` checked arithmetic；
- pass 不允许 wrapping 或 saturation 后继续；
- `PASS_SCALE` 是内部精度，不进入 Kconfig；
- queued task 的 nice 变化即使由既有 weak setter 发生，也不得修改其当前 pass 或 heap key；每个推进 pass 的 tick 或 charged-yield transaction 只读取一次 `Task::nice()`，首次观察到新值的 transaction 使用新 delta。本 RFC 不承诺与 update 竞争的 charge 观察旧值还是新值、观察延迟上界、current segment split 或历史 pass 重算。

固定 nice 公平证明使用本区间内不变的 delta。动态 reweight proof 留给后续调度属性 RFC。

## Heap Order 与 Snapshot

每个 ready entry 的行为 key 为：

```text
(pass_snapshot ascending, enqueue_seq ascending)
```

不变量：

- 在完整 `RunQueue` transaction 边界，每个 queued Fair task 恰好有一个 heap entry 且 `on_runq == true`。
- 在完整 `RunQueue` transaction 边界，running current、blocked、exiting 和 fresh unpublished task 的 `on_runq == false`，且 current 不得同时出现在 heap。
- `pass_snapshot == task.StrideEntity.pass.expect(...)` 在整个 queued lifetime 内成立。
- entity pass 只能在 task 不是 queued member 时修改。
- pop/dequeue 必须以常开断言验证 snapshot、class identity、`Some(pass)` 和 membership。
- `enqueue_seq` 每次入堆分配新值，严格递增，不得 wrap。
- pass 相同时，较早 sequence 先运行。
- heap entry 的 pass 只是 immutable ordering snapshot，不能被当作独立 accounting truth 修改。

第一版允许 `dequeue(task)` 线性定位 entry 并 rebuild heap。任何 indexed heap follow-up 都必须保持上述 key、snapshot 和 membership contract。

### RunQueue Membership 发布顺序

`on_runq` 是 `RunQueue` facade 发布的 generic membership，不是 Fair class 每个内部步骤都要同步维护的第二个 heap flag。live method-first transaction 必须保持：

- `enqueue_new()` / `enqueue_woken()`：入口为 `on_runq == false` 且无 heap entry；Fair 先完成入堆，`RunQueue` 再发布 `on_runq = true`；
- yield / preempt / handoff requeue：入口为 active current、`on_runq == false` 且无 heap entry；Fair 先 clear current 并入堆，`RunQueue` 再发布 `on_runq = true`；
- `dequeue()`：入口为 `on_runq == true` 且有唯一 heap entry；Fair 先移除 entry，`RunQueue` 再发布 `on_runq = false`；
- `pick_next_task()`：入口为 `on_runq == true` 且有唯一 heap entry；Fair 先 pop，`RunQueue` 再清除 `on_runq`，随后 `set_next_task()` 才建立 class-active current。

上述短暂差异只允许存在于同一 owner-CPU IRQ-off `RunQueue` transaction 内。不得在 class dispatch 与 generic flag 发布之间加入 admission、callback、unlock 或 remote observation，也不得增加 class-private `queued` / `member` flag 复制 `on_runq`。

## Current Identity

- `set_next_task(task)` 是建立 class-active current 的唯一 transaction。
- 建立 current 前必须没有旧 current，task 必须为 `Some(pass)`、已从 Fair heap 取出且 `on_runq == false`。
- `task_tick()`、yield、preempt、handoff、block 和 exit 必须验证参数为 `Some(pass)` 且与 class-active current 相同。
- yield/preempt/handoff 在入堆前清除 current。
- block/exit 在离开 Fair visible set 前清除 current。
- weak current 只提供 identity，不拥有 task 生命周期；upgrade 失败表示 lifecycle bug，不能 fallback 到任意 heap task。
- `pick_next_task()` 要求 current 已清除。

## Placement Floor

定义 visible Fair set：

```text
ready heap union class-active current
```

`placement_floor` 满足：

- floor 只在一个完整 lifecycle transaction 的最终 post-state 刷新；中间的 pop、clear-current 或尚未完成的 requeue 不得刷新；
- final visible set 非空时，计算 `min_visible`，常开断言 `min_visible >= old_floor`，再设置 `floor = min_visible`；不得用 `max(old_floor, min_visible)` 隐藏低于 floor 的 entity；
- final visible set 为空时，保留最后 floor；
- fresh enqueue 设置 `pass = Some(floor)`；
- placed wake 设置 `pass = Some(max(old_pass, floor))`；
- new/wake 后不得存在低于 floor 的 queued entity；
- floor 只服务 admission，不参与 heap comparison、eligibility、tick decision 或 preempt decision；
- 不允许 no-eligible branch、fallback pick 或用 floor 把 runnable task排除在候选集合外。

`pick_next_task()` 只 pop/验证并保持 floor 不变。live owner-CPU IRQ-off full-pick transaction 必须无 admission、无 callback 地继续调用 `set_next_task(selected)`；set-next 建立 current 后才按 current 与 remaining heap 刷新。若该 pair 出现独立 production caller、可中断分支或中间 admission，必须停止当前 gate 并回到 RFC review，不能增加 selected/in-flight state 兜底。

## Service Unit 与 Tick

一个 scheduler timer tick 是一个 Stride service unit。

`task_tick(current)` 线性化顺序：

1. 验证 current identity、class、非 queued membership 和 `Some(pass)`；
2. 从 `Task::nice()` 读取本次 typed weight；
3. 对 current pass 增加一个 delta；
4. 刷新 placement floor；
5. 若 ready heap 非空，返回 `RequestResched`；否则返回 `None`。

不得把 pass charge 延迟到 `requeue_preempted_current()`，否则 deferred preempt 或合并 Tick 会丢失 service units。`PendingResched::Tick` 只说明存在未消费的 full-pick request；每次实际 timer tick 已经在 class transaction 中独立收费。

## Lifecycle Placement Matrix

| Transaction | Pass action | Placement/current action | Floor action |
| --- | --- | --- | --- |
| `enqueue_new()` | assert `None`, set `Some(floor)` | heap tail by fresh sequence | refresh after push |
| `enqueue_woken()` | require `Some`, clamp to `max(old, floor)` | heap tail by fresh sequence | refresh after push |
| `task_tick()` | require `Some`, add one delta | request pick only when peer exists | refresh after charge |
| `requeue_yielded_current()` with peer | require `Some`, add one delta, then raise to at least min peer | clear current, heap with fresh sequence | refresh after push |
| `requeue_yielded_current()` without peer | require `Some`, unchanged | clear current, heap, immediate normal self-pick allowed | refresh after push |
| `requeue_preempted_current()` | require `Some`, unchanged | clear current, heap with fresh sequence | refresh after push |
| `handoff_woken_current()` | require `Some`, unchanged | clear current, heap with fresh sequence | refresh after push |
| `put_prev_blocked()` | require `Some`, unchanged | clear current, not queued | refresh after clear |
| `put_prev_exiting()` | require `Some`, irrelevant after exit | clear current, not queued | refresh after clear |
| `dequeue()` | require `Some`, unchanged | remove queued entry | refresh after remove |
| `pick_next_task()` | require `Some`, unchanged | pop minimum entry | do not refresh |
| `set_next_task()` | require `Some`, unchanged | establish current | refresh after current establishment |
| same-Fair `decide_preempt_current()` | require current/candidate `Some` | verify active current and queued candidate | unchanged |

Tick path只能收费一次。`requeue_preempted_current()` 即使收到 Tick pending，也不得再次推进 pass。

## Yield Handoff

若 Yield transaction 开始时存在 Fair peer，设 `P` 为 transaction 内稳定的最小 peer pass；该 transaction 只读取一次 `Task::nice()`：

```text
current.pass = max(current.pass + delta(current.weight), P)
```

随后 current 使用晚于所有现有 entries 的新 sequence 入堆。必须证明：

- 至少一个 transaction 开始时已存在的 Fair peer 在 current 之前被 Fair pick；
- current 不能因更高 weight / 更小普通 delta 立即 self-pick；
- 不需要特殊 pick mode、skip flag、yield target 或来源分类；
- 多个同 pass peer 可以全部排在 current 前，合同只保证“至少一个”；
- 没有 Fair peer 时不增加 pass；
- 用户 `sched_yield()` 与内核 `yield_now()` 使用同一语义。

若 RT task 同时 runnable，RT precedence 可以先运行；它不改变 Fair heap 内的 handoff order。

## Arrival 与 Cross-Class Precedence

唯一跨 class 顺序：

```text
Realtime > Fair > Idle
```

- RT candidate 到达 Fair current 时请求 resched。
- Fair candidate 到达 Idle current 时请求 resched。
- Fair candidate 到达 RT current 时不请求 resched。
- same-Fair decision 必须常开断言 current 为 `Some(pass)`、匹配 class-active current 且 `on_runq == false`，candidate 为 `Some(pass)` 且 `on_runq == true`；candidate 不立即抢占 current，下一 timer tick 在存在 peer 时产生 Tick resched request，实际 full pick 可以被 scheduler core 合法延迟。
- class implementation 只声明 identity；rank 只由 centralized precedence 持有。
- source CPU 不读取 target CPU Fair heap/current。

## Fairness 与 Progress

证明前提：

- owner CPU 的持续 runnable Fair set 有限且非空；
- 每个 task 的 nice/weight 在观察区间固定；
- 所有 weight 和 delta 为正；
- scheduler tick 持续到达，full pick 最终可以执行；
- 没有更高 Realtime class 无限占用 CPU。

在上述前提下：

- 每次 pick 选择最小 pass；
- 被选 task 的 pass 在每个 service tick 后正向推进；
- 未被选 task 的 pass 保持不变并最终成为最小；
- 因此每个持续 runnable task 都获得无限多个 service units，不会稳定饿死；
- task 的长期 service-unit 速率与 `1 / delta(weight)` 成比例，并近似 Linux weight 比例；
- equal weight task 依靠 sequence tie-break 呈现 RR-like order。

explicit yield 可以主动放弃 credit，因此不属于“不调用 yield 的固定集合比例”证明。block/wake task 离开持续 runnable set，wake floor 只保证重新加入时不携带无界陈旧 credit。

## 锁序与事务边界

- 所有 Stride class state 只由 owner CPU 在 local IRQ disabled 的 `RunQueue` transaction 中修改。
- `RunQueue` 在 class dispatch 前后短暂使用 capability-gated entity lock检查并发布 `on_runq`；class implementation 不得让 entity lock 跨回 `RunQueue` dispatch。class-local physical mutation与 generic flag 发布按“RunQueue Membership 发布顺序”执行，只在完整 transaction 边界要求一致。
- heap 比较不得动态获取和修改 task entity key；比较使用 immutable entry snapshot。
- class transaction 可以短暂读取 entity、释放 lock、操作 heap，再在明确点重取 lock做断言/更新；queued pass immutable 保证 snapshot 稳定。
- 不得从 Stride class 内调用 scheduler core、直接修改 processor pending slot、wait state 或 task lifecycle。
- `local_pick_next()` 必须是 `pick_next_task()` / `set_next_task()` 的唯一 production scheduler-core caller，并在同一 owner-CPU IRQ-off full-pick transaction 中无 admission、无 callback 地成对调用。
- `set_next_task()` 发生在成功 pick 后、architecture switch 前；Stride current identity 不取代 `Processor::running_task`，只表达 class-active segment。

## 禁止退化项

- 增加 `SchedClassKind::Stride`、`SchedClassKind::Eevdf` 或 backend-specific class precedence。
- 在当前只有一个 Fair backend 时增加 selector、runtime backend enum 或 forwarding wrapper。
- 把 `placement_floor` 用作 eligibility 或 pick filter。
- 在复合 lifecycle transaction 的中间 pop/clear-current 状态刷新 floor。
- 用 `pass + initialized` 保留可表达矛盾组合的双状态，或让 `enqueue_new()` 之外的路径执行 `None -> Some`。
- queued 时直接修改 entity pass，却不重建 heap key。
- 从 heap snapshot 反向覆盖 entity pass。
- 缓存 nice/weight 并允许与 `Task::nice()` 冲突。
- 在 tick charge 后又因 Tick pending 在 requeue 时重复收费。
- 有 peer 的 yield 只做普通 `pass += delta`，从而允许高 weight task立即 self-pick。
- 用 forced pick、skip flag 或 task-type 特例替代 yield pass/order contract。
- 为 block/wake 加入实际 runtime、sleep credit 或 lag state。
- 用 saturation、wrapping 或 arbitrary fallback 隐藏 pass arithmetic failure。
- 为通过 runtime gate降低 precedence、压缩 Linux weight 或给 service kthread 增加隐式特殊 class。
- 让 archived EEVDF 类型进入 production Fair payload 或 heap。

## 完成标准

- index、invariants、implementation 对 Fair identity、default policy、pass arithmetic、heap snapshot、floor、yield 和 lifecycle 完全一致。
- focused KUnit 锁定 arithmetic/order/合法 placement/yield/progress、transaction-boundary membership 和 class precedence；expected-panic contract 由 source audit 确认常开断言，不作为普通 KUnit pass case。
- source audit 证明 production graph 只有 stable Fair identity 和一个 Stride backend alias。
- `fair`、`rt_rr`、`rt_fifo` 三种 selector build 均通过。
- Fair default boot/KUnit 通过；用户运行 `fair-test`，并以同 checkout RT/RR 为 A/B baseline 裁定真实 LTP integration 没有稳定的多倍整体退化。
- 独立 review 无未关闭 Apollyon 或 Keter；Euclid 必须有明确处置、owner、验证点和回写路径，但不自动阻塞 checkpoint。
- accepted IRQ-off allocation limitation 明确保留，不被误写成已修复。
- 若任何目标/不变量因实现反馈需要改变，先回到 RFC review，不能只在 transaction devlog 或代码注释中削弱合同。
