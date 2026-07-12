# Sched RT Class 不变量需求

**状态：** Canonical
**最后更新：** 2026-07-12
**父 RFC：** [RFC-20260711-sched-rt-class](./index.md)

## 闭合条件

第一版必须同时满足：

- FIFO 与 RR 只有一个共享的 `Realtime` scheduler class identity。
- `RtEntity` 是 effective RT policy、priority 与 RR budget 的唯一真相源。
- `RtPriority` 只允许 `1..=99`，且数值越大优先级越高。
- 相同 priority 的 FIFO/RR task 共享一条 FIFO 等待序列。
- pick 总是选择最高 non-empty priority bucket 的队头。
- arrival 只在 candidate priority 严格高于 current 时请求抢占。
- FIFO 不因 tick 轮转；RR 只在 quantum 到期且存在同 priority peer 时轮转。
- `Realtime > Idle` 是唯一跨 class precedence。
- RT/FIFO 与 RT/RR 可以通过 Kconfig 构建并运行用户态 smoke；默认值沿用当前 legacy RoundRobin 行为。
- 没有任何调度属性 syscall、测试入口或普通 task setter 能修改已发布 task 的 effective RT state。

## 非目标

- 不证明 hard realtime latency、bounded response 或 priority inversion freedom。
- 不证明 RT bandwidth、throttling、cgroup、PI、deadline 或 SMP migration。
- 不证明运行期调度属性修改和 syscall ABI。
- 不通过不同 priority 的用户态 workload 关闭 priority correctness。
- 不改变 wait-core identity、logical state 或 park protocol。
- 不要求 production mixed workload 来验证 `Realtime > Idle`；该项由 source/KUnit 关闭。

## 状态所有权

### Task 与 Scheduler Core

- `TaskSchedState` 继续单独拥有 runnable / waiting / zombie 逻辑状态。
- `SchedEntity::on_runq` 继续只表示 owner CPU runqueue 上的 physical membership。
- `Task::cpuid()` 继续是 owner CPU 真相源。
- `Processor::pending_resched` 继续是面向下一次 owner-CPU full pick 的 scheduler-core 合并 latch；successful pick 确认此前 slot，no-pick 路径保留或由 destructive-take caller 恢复。
- `SchedClassKind` 是从 class payload 派生的 immutable class identity，供 `RunQueue` 执行 dispatch 与集中 precedence；它不是独立可变的 policy 真相源。

### RT Entity

`SchedEntity::Realtime(RtEntity)` 单独拥有：

- effective `RtPriority`；
- effective `RtPolicy`；
- RR policy 内的 `remaining_ticks`。

禁止在 `Task`、runqueue bucket、ABI accounting、procfs cache 或诊断字段中复制一份 effective RT policy / priority。queue node 只保存 task identity；其 bucket 必须由 entity priority 推导。

Kconfig default selector 只决定 fresh entity 的创建 policy，不是每个 task 的运行期 policy 真相源。task 一旦发布，本 RFC 内 policy 与 priority 不再变化。

## 类型不变量

```text
RtPriority::MIN   = 1
RtPriority::MAX   = 99
RtPriority::WIDTH = 99
```

- 内部构造非法 priority 必须 fail fast，不能 clamp。
- queue index 是 typed priority 的无歧义映射。
- priority comparison 使用领域语义“数值越大越高”，不得把 Linux 内部反向 `prio` 编码泄漏进 class。
- `RtPolicy::Fifo` 不携带 budget。
- `RtPolicy::RoundRobin` 直接携带 `remaining_ticks`，不得把 remaining 拆成 sibling field。

## Fresh Construction

- `new_default()` 是所有非 idle production task 的唯一默认构造入口。
- `new_realtime(...)` 只允许构造 caller-owned、尚未发布且尚未入队的 fresh entity。
- 不保留与 `new_default()` 重叠的 `new_normal()`。
- `new_idle()` 只创建 Idle entity。
- RT/FIFO 或 RT/RR default build 中，`new_default()` 根据 selector 创建 fresh RT entity，并使用 `RtPriority::MIN`。
- fresh RR entity 的 `remaining_ticks` 必须等于 full quantum。
- clone 不复制 parent 的 on-runq、policy runtime state 或 RR remaining；它调用 `new_default()` 获得 fresh entity。

任何 published-task class/policy/priority 修改都超出本 RFC。未来设计必须以 owner-CPU transaction 同时处理旧 queue removal、entity replacement、new queue placement、current state 与 preempt decision。

## Class Precedence

唯一合法顺序是：

```text
Realtime > Idle
```

- class implementation 只声明 identity，不各自保存 rank。
- `RunQueue` 消费集中 precedence，不复制第二份顺序。
- 任意 runnable RT task 都高于 Idle task，与 RT priority 数值无关。
- 无 bandwidth controller 时，FIFO task 对较低优先级 RT task 和同优先级 cooperative peer 的无限 starvation 是接受语义。
- Idle 只在其它 class 没有 runnable task 时选择。

## Queue Membership

- 每个 queued RT task 必须恰好位于其 `RtPriority` 对应的一个 bucket。
- FIFO 与 RR policy 不拆分 bucket。
- queued task 的 `on_runq == true`；running current、blocked、exiting 与 fresh unpublished task 的 `on_runq == false`。
- current running task 不同时存在于 RT bucket。
- duplicate enqueue、missing dequeue、bucket/entity priority 不一致必须通过 `assert!` 暴露。
- priority 在本 RFC 内不可变，因此 class transaction 可以短暂读取 typed priority、释放 entity lock，再操作对应 bucket。
- pick 只能从最高 non-empty bucket 的队头移除 task。
- 第一版不维护 bitmap、cached highest priority 或第二份 runnable count truth。
- bucket 的 materialization / growth allocation 继承 legacy `RoundRobin` ready queue 的既有 noirq 限制；本 RFC 不新增 allocator/OOM 语义，也不把该路径宣称为 allocation-free。该接受限制必须在实现注释和事务日志中明确，若要消除需另开结构 gate。

## Arrival 与 Preempt

同 class arrival decision：

```text
candidate.priority > current.priority  -> RequestResched
candidate.priority <= current.priority -> KeepCurrent
```

- equal priority arrival 不因 policy 不同而抢占。
- cross-class decision 仍由 `RunQueue` 的集中 precedence 处理。
- arrival decision 只能在 owner CPU placement 完成后执行。
- source CPU 不得读取 target CPU current 或 RT bucket。

current 因 RunnableArrival 被更高 priority task 抢占时，必须回到自身 priority bucket 队头。这样 current 在同 priority FIFO order 中保留其原执行位置。

## Lifecycle Placement Matrix

| Transaction | Placement | RR budget |
| --- | --- | --- |
| `enqueue_new()` | priority bucket tail | fresh full budget |
| `enqueue_woken()` | priority bucket tail | preserve |
| `requeue_yielded_current()` | priority bucket tail | preserve |
| higher-priority arrival preempt | priority bucket head | preserve |
| RR quantum expiry | priority bucket tail | refill then preserve full/new remainder |
| `handoff_woken_current()` | priority bucket tail | preserve |
| `put_prev_blocked()` | no placement | preserve in entity |
| `put_prev_exiting()` | no placement | irrelevant after exit |

如果 `PendingResched` 同时包含 Tick 与 RunnableArrival，Tick 只能来自 RR quantum expiry，因此 current 使用到期后的队尾 placement。RunnableArrival 不能把已耗尽 quantum 的 task 恢复到队头。

`PendingResched` 只按值传入 preempted-current transaction，并在同一次 full pick 前由 current RT task 消费。successful pick acknowledgement 只清 processor slot，不抹除已捕获的 transaction snapshot；RT class 不保存 pending bits，也不恢复 processor pending slot。

## RR Quantum

full budget 使用：

```text
max(1, ceil(RT_RR_TIMESLICE_MS * SYSTEM_HZ / 1000))
```

必须满足：

- `SYSTEM_HZ > 0`。
- `RT_RR_TIMESLICE_MS > 0`。
- 中间乘法不会因窄整数溢出。
- full budget 至少为 1 tick。
- persistent `remaining_ticks` 始终位于 `1..=full budget`。

RR tick transaction：

1. `remaining_ticks > 1` 时减一并返回 `TickAction::None`。
2. `remaining_ticks == 1` 时补满。
3. 同 priority bucket 有 peer 时返回 `RequestResched`；无 peer 时返回 `None` 并继续运行。

低 priority peer 不触发 rotation。FIFO tick 永远返回 `None`，且不能通过伪造 RR budget 复用该分支。

## Wait-Core 边界

- RT class 不读取 `WaitState`、`WakeToken`、`PrePark/Parked` 或 wait id。
- 正常 wake 只通过 `enqueue_woken()` 获得队尾 placement。
- parked wake handoff 只通过 `handoff_woken_current()` 获得队尾 placement。
- no-switch early abort 不调用 class transaction，不执行 full pick，不确认 processor pending slot。
- wait lifecycle 不重置 RR budget。

该区分依赖 method-first transaction 名称，不新增 catch-all wait reason enum。

## 锁序与生命周期

- RT queue 只在 owner CPU 的 noirq scheduler transaction 内修改。
- `RunQueue` 不得持有 task entity lock 进入会再次锁 entity 的 class dispatch。
- class 对 entity priority / policy 的访问必须保持短临界区；queue container 操作不在 entity lock 下执行。
- priority 在本 RFC 内不可变，使 snapshot 后的 bucket 操作无需跨锁重验证 mutation；仍应以 `assert!` 检查 entity/bucket 一致性。
- cleanup / dequeue 必须先移除 physical membership，再更新 shared `on_runq` truth；失败通过 correctness assertion 暴露。
- 不新增 remote queue lock、跨 CPU entity mutation 或可能睡眠的调度锁。

## Kconfig 不变量

- `sched_default_policy` 只能是 `rt_rr` 或 `rt_fifo`。
- selector 必须由受约束配置类型解析，不能由多个互斥 boolean 拼装。
- 默认 selector 沿用当前 legacy RoundRobin 行为，即 `rt_rr`。
- default RT priority 固定为代码内 `RtPriority::MIN`，不得增加 Kconfig 镜像。
- RR timeslice 以时间单位进入 Kconfig，remaining state 以换算后的 tick 保存。
- selector 只影响 fresh construction，不允许运行期重新解释已经创建的 entity。

## 验证证明边界

source/KUnit 必须覆盖：

- priority bounds / ordering / bucket mapping；
- highest-bucket FIFO pick；
- equal-priority FIFO/RR 混排；
- strict higher-priority preemption；
- arrival head requeue 与 Tick tail requeue；
- Tick + RunnableArrival 的 Tick-dominant placement；
- FIFO no-budget / no-tick-rotation；
- RR budget decrement、refill、peer/no-peer decision；
- class precedence；
- constructor selector 与 fresh RR full budget。

用户态只承担：

- RT/RR 同 priority CPU-bound progress；
- RT/FIFO 同 priority no automatic tick rotation；
- explicit yield 与 block/wake lifecycle。

不同 priority runtime ordering、动态 policy change、ABI read-back 和权限不属于本 RFC 的用户态验证。

## 禁止退化项

- 把 FIFO 与 RR 重新拆成两个 class。
- 在 Task 或 ABI state 中复制 effective RT policy / priority。
- 为避免 FIFO starvation 偷加 tick rotation、隐式 yield 或 service-task exception。
- 让 equal priority arrival 抢占 current。
- higher-priority preempt 后把未到期 current 放到队尾。
- RR 到期后因低 priority peer 而轮转。
- block、wake 或 yield 隐式补满 RR budget。
- 用 test-only syscall、隐藏 setter 或直接 entity mutation 制造不同 priority 用户态测试。
- 重新引入已删除的 `requeue_aborted_wait_current()` 或等价 abort-park class transaction。
- 让 `new_default()` 与另一个 default/normal constructor 形成并列入口。
- 用多个 boolean Kconfig 表达 default selector。
- 把实现阶段未确定误写成已批准 write set 或 gate。

## 完成标准

- canonical 文本与代码对 class/policy/priority ownership 一致。
- 旧独立 `RoundRobin` class identity 与 queue 不再并存。
- RT/RR 与 RT/FIFO 两种 compile-time default build 通过已批准的构建与用户态 smoke。
- priority correctness 的 source/KUnit 证据完整。
- user-run、agent-run 与 unrun 项明确区分。
- ABI、dynamic transaction、bandwidth、priority runtime validation 和 procfs observation 仍明确延期。
