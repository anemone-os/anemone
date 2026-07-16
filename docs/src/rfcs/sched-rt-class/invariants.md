# Sched RT Class 不变量需求

**状态：** Canonical
**适用修订：** R1
**最后更新：** 2026-07-14
**父 RFC：** [RFC-20260711-sched-rt-class](./index.md)

## 闭合条件

第一版必须同时满足：

- FIFO 与 RR 只有一个共享的 `Realtime` scheduler class identity。
- `RtEntity` 是 effective RT policy、priority、RR budget 与 RR committed rotation obligation 的唯一真相源。
- `RtPriority` 只允许 `1..=99`，且数值越大优先级越高。
- 相同 priority 的 FIFO/RR task 共享一条 FIFO 等待序列。
- pick 总是选择最高 non-empty priority bucket 的队头。
- arrival 只在 candidate priority 严格高于 current 时请求抢占。
- FIFO 不因 tick 轮转；RR 只在 quantum 到期且存在同 priority peer 时提交 rotation obligation，已提交义务不依赖消费时 peer 仍存在。
- `Realtime > Fair > Idle` 是唯一跨 class precedence。
- RT/FIFO 与 RT/RR 都可以通过 shared Kconfig selector 构建；repository default 当前为 `fair`。R0 用户态集成由 RT/RR 整套 LTP 运行关闭，FIFO 用户态专项验证不作为 R0 闭合条件。
- 没有任何调度属性 syscall、测试入口或普通 task setter 能修改已发布 task 的 effective RT state。

## 非目标

- 不证明 hard realtime latency、bounded response 或 priority inversion freedom。
- 不证明 RT bandwidth、throttling、cgroup、PI、deadline 或 SMP migration。
- 不证明运行期调度属性修改和 syscall ABI。
- 不通过不同 priority 的用户态 workload 关闭 priority correctness。
- 不改变 wait-core identity、logical state 或 park protocol。
- 不要求 production mixed workload 来验证 `Realtime > Fair > Idle`；该项由 source/KUnit 关闭。

## 状态所有权

### Task 与 Scheduler Core

- `TaskSchedState` 继续单独拥有 runnable / waiting / zombie 逻辑状态。
- `SchedEntity::on_runq` 继续只表示 owner CPU runqueue 上的 physical membership。
- `Task::cpuid()` 继续是 owner CPU 真相源。
- `Processor::pending_resched` 是面向下一次 owner-CPU full pick 的 scheduler-core 单 bit 合并 latch；successful pick 确认此前 slot，no-pick 路径保留或由 destructive-take caller 恢复。它不编码 request cause，也不进入 scheduler-class transaction。
- `take_pending_resched()` 返回 typed pending-pick snapshot 并清 slot；`restore_pending_resched()` 只做 union，不丢失 take 后新增的 request。`schedule_preempt(pending)` 只用非空 snapshot 证明 entry 合法，deferred 后仍由 caller 恢复原 snapshot。
- `ScheduleMode::Preempt`、`ScheduleDecision::Preempted`、`Scheduler::requeue_preempted_current()` 与 `RunQueue` 不携带 pending。no-switch abort / deferred preempt 不发生 class transaction；successful full pick 继续是 acknowledgement 点。
- `SchedClassKind` 是从 class payload 派生的 immutable class identity，供 `RunQueue` 执行 dispatch 与集中 precedence；它不是独立可变的 policy 真相源。
- `SchedEntity::{new_default,new_idle}` 的公开构造 facade 位于 `entity.rs`；custom RT fresh construction、RT policy、quantum 和 payload factory 由 `rt.rs` 单独拥有，RT runtime types 不从共享 `mod.rs` 公开 re-export。

### RT Entity

`SchedEntity::Realtime(RtEntity)` 单独拥有：

- effective `RtPriority`；
- effective `RtPolicy`；
- RR policy 内的 `remaining_ticks`；
- RR policy 内的 `rotation_due`。

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
- `RtPolicy::RoundRobin` 直接携带 `remaining_ticks` 与 `rotation_due`，不得把 budget 或 rotation obligation 拆到 Task、processor pending slot、queue node 或 sibling owner。
- `rotation_due` 是行为协议状态，不是诊断字段；它只决定 current 下一次离开 active segment 时的 RT placement。
- fresh policy 通过对称的 `RtPolicy::fifo()` / `RtPolicy::round_robin()` 入口构造。

## Fresh Construction

- `new_default()` 是所有非 idle production task 的唯一默认构造入口。
- custom priority/policy 的 fresh RT entity 只允许由 `rt.rs` 的 class/test-private helper 构造，不形成共享 `SchedEntity::new_realtime(...)` surface。
- 不保留与 `new_default()` 重叠的 `new_normal()`。
- `new_idle()` 只创建 Idle entity。
- RT/FIFO 或 RT/RR default build 中，`new_default()` 根据 selector 创建 fresh RT entity，并使用 `RtPriority::MIN`。
- fresh RR entity 的 `remaining_ticks` 必须等于 full quantum，`rotation_due == false`。
- clone 不复制 parent 的 on-runq、policy runtime state、RR remaining 或 rotation obligation；它调用 `new_default()` 获得 fresh entity。
- `SchedEntity` 及其 class payload 不实现 `Clone`；published entity 不能通过复制形状伪装成 fresh entity。
- `Task` 不向普通 crate caller 暴露可用的完整 `&mut SchedEntity`。entity lock bridge 必须消费不可由 scheduler-class owner 外构造的 capability；scheduler core 只使用只读 class/membership observation。

任何 published-task class/policy/priority 修改都超出本 RFC。未来设计必须以 owner-CPU transaction 同时处理旧 queue removal、entity replacement、new queue placement、current state 与 preempt decision。

## Class Precedence

唯一合法顺序是：

```text
Realtime > Fair > Idle
```

- class implementation 只声明 identity，不各自保存 rank。
- `RunQueue` 消费集中 precedence，不复制第二份顺序。
- 任意 runnable RT task 都高于 Fair 与 Idle task，与 RT priority 数值无关；Fair 高于 Idle。
- 无 bandwidth controller 时，FIFO task 对较低优先级 RT task 和同优先级 cooperative peer 的无限 starvation 是接受语义。
- Idle 只在 RT 和 Fair 都没有 runnable task 时选择。

## Queue Membership

- 每个 queued RT task 必须恰好位于其 `RtPriority` 对应的一个 bucket。
- FIFO 与 RR policy 不拆分 bucket。
- queued task 的 `on_runq == true`；running current、blocked、exiting 与 fresh unpublished task 的 `on_runq == false`。
- current running task 不同时存在于 RT bucket。
- fresh、queued、blocked 与 exiting RT/RR task 的 `rotation_due == false`；只有 active current execution segment 可以携带 `rotation_due == true`。
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

current 因更高 priority task arrival 被抢占时，若 `rotation_due == false`，必须回到自身 priority bucket 队头，以保留其原执行位置；若此前已提交 rotation obligation，则必须先履行队尾 placement。

## Lifecycle Placement Matrix

| Transaction | Placement | RR budget | Rotation obligation |
| --- | --- | --- | --- |
| `enqueue_new()` | priority bucket tail | fresh full budget | require clear |
| `enqueue_woken()` | priority bucket tail | preserve | require clear |
| `requeue_yielded_current()` | priority bucket tail | preserve | clear / consume |
| involuntary preempt, obligation clear | priority bucket head | preserve | remain clear |
| involuntary preempt, obligation set | priority bucket tail | preserve current remainder | clear / consume |
| `handoff_woken_current()` | priority bucket tail | preserve | clear / consume |
| `put_prev_blocked()` | no placement | preserve in entity | clear without refill |
| `put_prev_exiting()` | no placement | irrelevant after exit | clear |

`task_tick()` 在 RR quantum expiry 且存在同 priority peer 时先提交 `rotation_due = true`，再返回 `RequestResched`。该义务一旦提交，就不因 full pick 延迟、后续 arrival、同 priority peer 在消费前消失或 scheduler-core pending slot 被合并而撤销。消费阶段不得重新断言请求产生时的 peer / higher-candidate 条件。

多个 expiry 只合并为一个 `rotation_due`。current 不在 ready queue 中，因此一个 active segment 最多需要表达一次“下次离开时进入队尾”，不存在需要累计多个队尾移动的队列债务。

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
- `rotation_due` 不参与 budget 数值校验，也不能被解释为“remaining 当前仍为 full”。

RR tick transaction：

1. `remaining_ticks > 1` 时减一；已有 `rotation_due` 原样保留。
2. `remaining_ticks == 1` 时补满。
3. expiry 时若同 priority bucket 有 peer，则置 `rotation_due = true` 并返回 `RequestResched`；无 peer 时保持原 obligation，不为本次 expiry 新增 request。

若 full pick 被延迟，后续 timer tick 可以在 `rotation_due == true` 时继续递减 refill 后的新 quantum；后续 expiry 仍只保持同一个 bool。最终 preempted-current transaction 必须履行队尾 placement，并保留消费时仍位于合法范围内的 current remainder。

低 priority peer 不触发 rotation。FIFO tick 永远返回 `None`，且不能通过伪造 RR budget 复用该分支。

## Wait-Core 边界

- RT class 不读取 `WaitState`、`WakeToken`、`PrePark/Parked` 或 wait id。
- 正常 wake 只通过 `enqueue_woken()` 获得队尾 placement。
- parked wake handoff 只通过 `handoff_woken_current()` 获得队尾 placement。
- no-switch early abort 不调用 class transaction，不执行 full pick，不确认 processor pending slot。
- wait lifecycle 不重置 RR budget；handoff / block 必须清除 active-segment rotation obligation，普通 wake 只接受 obligation clear 的 entity。

该区分依赖 method-first transaction 名称，不新增 catch-all wait reason enum。

## 锁序与生命周期

- RT queue 只在 owner CPU 的 noirq scheduler transaction 内修改。
- `RunQueue` 不得持有 task entity lock 进入会再次锁 entity 的 class dispatch。
- class 对 entity priority / policy 的访问必须保持短临界区；queue container 操作不在 entity lock 下执行。
- scheduler core 的 current-only transaction entry 是 `rotation_due` 可置真或消费的调用合同；RT 不复制一份 current `Weak<Task>`。`Processor::running_task` 继续是 current identity 的唯一系统真相源。
- priority 在本 RFC 内不可变，使 snapshot 后的 bucket 操作无需跨锁重验证 mutation；仍应以 `assert!` 检查 entity/bucket 一致性。
- cleanup / dequeue 必须先移除 physical membership，再更新 shared `on_runq` truth；失败通过 correctness assertion 暴露。
- 不新增 remote queue lock、跨 CPU entity mutation 或可能睡眠的调度锁。

## Kconfig 不变量

- shared `sched_default_policy` 允许 `fair | rt_rr | rt_fifo`；本 RFC 只拥有两个 RT selector 分支。
- selector 必须由受约束配置类型解析，不能由多个互斥 boolean 拼装。
- repository default 现由 Fair RFC 设为 `fair`；选择 `rt_rr` / `rt_fifo` 时仍必须创建本 RFC 定义的 fresh RT entity。
- default RT priority 固定为代码内 `RtPriority::MIN`，不得增加 Kconfig 镜像。
- RR timeslice 以时间单位进入 Kconfig，remaining state 以换算后的 tick 保存。
- selector 只影响 fresh construction，不允许运行期重新解释已经创建的 entity。

## 验证证明边界

source/KUnit 必须覆盖：

- priority bounds / ordering / bucket mapping；
- highest-bucket FIFO pick；
- equal-priority FIFO/RR 混排；
- strict higher-priority preemption；
- obligation-clear preempt head requeue 与 rotation-due preempt tail requeue；
- expiry commit、delayed tick preserve、重复 expiry coalescing 与 peer 消失后仍履行 rotation；
- yield/handoff 消费 rotation，block/exit 清除 rotation，fresh/queued/woken state 保持 clear；
- core pending take / restore / union / full-pick acknowledgement，且 class surface 不再接收 pending；
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
- higher-priority preempt 后把 obligation-clear current 放到队尾。
- 让 higher-priority preempt 覆盖已经提交的 `rotation_due`，把 current 错放回队头。
- RR 到期后因低 priority peer 而轮转。
- 在 preempt 消费阶段重新要求原 same-priority peer 或 higher-priority candidate 仍存在。
- 把 `rotation_due` 放入 processor pending slot、Task sibling field、queue node 或 RT owner 之外的结构。
- 把 request cause、`PendingResched` 或 pending snapshot 继续传入 `Scheduler::requeue_preempted_current()`、`RunQueue` 或 class transaction。
- block、wake 或 yield 隐式补满 RR budget。
- 用 test-only syscall、隐藏 setter 或直接 entity mutation 制造不同 priority 用户态测试。
- 重新引入已删除的 `requeue_aborted_wait_current()` 或等价 abort-park class transaction。
- 让 `new_default()` 与另一个 default/normal constructor 形成并列入口。
- 用多个 boolean Kconfig 表达 default selector。
- 把实现阶段未确定误写成已批准 write set 或 gate。

## 完成标准

- canonical 文本与代码对 class/policy/priority/rotation ownership 一致。
- 旧独立 `RoundRobin` class identity 与 queue 不再并存。
- `fair`、`rt_rr` 与 `rt_fifo` 三种 compile-time selector build 通过批准的构建 gate。
- priority、rotation lifecycle 与 core pending/class boundary 的 source/KUnit 证据完整。
- user-run、agent-run 与 unrun 项明确区分。
- ABI、dynamic transaction、bandwidth、priority runtime validation 和 procfs observation 仍明确延期。
