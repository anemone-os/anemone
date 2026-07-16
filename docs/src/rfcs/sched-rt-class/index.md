# RFC-20260711-sched-rt-class

**状态：** Closed
**修订：** R1
**负责人：** doruche, Codex
**最后更新：** 2026-07-14
**领域：** scheduler / realtime / FIFO / RR / scheduler class
**事务日志：** [2026-07-12-sched-rt-class (R0)](../../devlog/transactions/2026-07-12-sched-rt-class.md)、[2026-07-14-sched-rt-class-r1 (R1)](../../devlog/transactions/2026-07-14-sched-rt-class-r1.md)
**开放问题：** 无；`KETER-RT-007` 已 neutralize
**下一步：** R1 已关闭；动态调度属性、bandwidth control 与 archived EEVDF 迁移继续按独立 RFC 边界处理。

## 摘要

本 RFC 定义 Anemone 第一版 realtime scheduler class。`SCHED_FIFO` 与 `SCHED_RR` 不实现为两个具有固定先后顺序的 scheduler class，而是同一个 `Realtime` class 下的两种 task policy。二者共享 `1..=99` 的 typed RT priority domain 和同一组按 priority 分桶的 FIFO ready queue；RR 只在 FIFO 语义之上增加 tick-based quantum。

本 RFC 只把 method-first `Scheduler` trait、`RunQueue` dispatch、`SchedEntity` class payload 和 scheduler-core-owned typed `PendingResched` 视为下层接口合同。R1 明确：pending 只表示“需要一次 full pick”，不向 scheduler class 传播 request cause。

第一版不实现调度属性 syscall，也不允许运行期修改已发布 task 的 class、RT policy 或 priority。为了在没有 `sched_setscheduler()` 的情况下进行真实用户态验证，Kconfig 可以在编译期选择所有非 idle task 的默认 class / policy；当前 shared selector 为 `fair | rt_fifo | rt_rr`，repository default 已由 Fair RFC 设为 `fair`。选择任一 RT 分支时，本 RFC 仍完整拥有对应 fresh entity 与算法语义。

## 背景

本 RFC 假定 scheduler class domain 提供以下稳定接缝：

- `RunQueue` 统一分发 class-local lifecycle transaction。
- `SchedEntity` 保存 `on_runq` 与 class-specific payload。
- `SchedClassKind` 只提供 class identity snapshot；跨 class precedence 集中定义。
- current requeue 已区分 yield、involuntary preempt 与 parked wake handoff；wait no-switch abort 不调用 class transaction。
- R1 的 `PendingResched` 是 scheduler-core / processor-owned 单 bit 合并 latch，只表示下一次 owner-CPU full pick 尚未完成；successful pick 确认此前 slot，no-pick 路径保留或恢复。Tick 与 arrival 的产生路径不再形成 class-visible cause taxonomy。
- owner CPU、noirq transaction、wait-core logical state 与 physical runqueue membership 的所有权已经闭合。

待替换的 legacy `RoundRobin` class 只有单个 `VecDeque`，没有 RT priority，也没有独立 quantum state；其 tick 路径每个 tick 都请求 resched。第一版 RT 工作应替换这份遗留 class，而不是在其旁边再增加独立 FIFO class。

把 FIFO 与 RR 拆成两个 scheduler class 无法表达 Linux/POSIX 的 priority-first 语义。例如 RR priority 99 必须高于 FIFO priority 1，而固定 class precedence 无法同时满足这一点。因此 policy 必须位于一个共享的 RT class 内部。

## 目标

- 引入单一 `Realtime` scheduler class，并删除遗留的独立 `RoundRobin` class identity。
- 在 `RtEntity` 中保存 effective RT priority 与 policy，作为真实调度行为的唯一真相源。
- 使用 typed `RtPriority` 约束 `1..=99`；数值越大，调度优先级越高。
- 使用 `RtPolicy::Fifo` 与 `RtPolicy::RoundRobin { remaining_ticks, rotation_due }` 表达 policy；FIFO 的类型形态中不存在无效 quantum 或 rotation state。
- 为每个 RT priority 提供一个共享 FIFO/RR ready queue；第一版固定 99 个 bucket，不引入 bitmap。
- 固定跨 class precedence 为 `Realtime > Fair > Idle`；该顺序只由 scheduler-class domain 集中拥有。
- 实现严格高 priority arrival preemption、同 priority 不抢占、FIFO no-timeslice 和 RR quantum rotation。
- 复用 `Scheduler` trait 的 method-first class transaction，不新增 catch-all event 或调度 reason bus。
- 增加 `SchedEntity::new_default()`；所有非 idle production 创建路径统一使用该 facade，custom priority/policy 的 fresh construction 留在 RT class owner 内。
- 在 shared Kconfig selector 中提供 `rt_fifo` 与 `rt_rr` 两个 RT 分支；不复制 default policy truth。
- 默认 RT 构造使用代码内固定的 `RtPriority::MIN`，不增加 default RT priority Kconfig。
- RR timeslice 以 Kconfig 时间目标表示，再根据 `SYSTEM_HZ` 换算为 tick budget。
- 用 source proof / KUnit 关闭 priority correctness，用用户态 smoke 验证同 priority FIFO/RR lifecycle。

## 非目标

- 不实现 `sched_setscheduler()`、`sched_setparam()`、`sched_setattr()`、`sched_getscheduler()`、`sched_getparam()` 或 `sched_rr_get_interval()`。
- 不实现已发布 task 的运行期 class、policy 或 priority 修改，也不实现对应 owner-CPU command / IPI transaction。
- 不实现 `SCHED_RESET_ON_FORK`、`RLIMIT_RTPRIO`、`CAP_SYS_NICE`、user namespace、LSM 或其它调度属性权限语义。
- 不建立独立的 task-local ABI policy/priority accounting state。
- 不实现 RT bandwidth controller、throttling、cgroup RT bandwidth、priority inheritance 或 deadline scheduler。
- 不实现跨核迁移、load balance、remote runqueue observation、CPU hotplug 或 scheduler domain。
- 不保证 hard realtime latency、最大响应时间或抢占延迟上界。
- 不要求用户态测试不同 RT priority；该验证依赖未来安全的调度属性 transaction。
- 不把精确 100ms、吞吐比例或 benchmark 数字作为 RR 接受条件。
- 不修改其它 scheduler class 的内部算法。
- 不补齐 procfs scheduling policy / RT priority 字段；对应观察面留给后续 ABI 工作。

## 文档地图

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)
- [Tracking Issues](./tracking-issues.md)

背景材料：

- None

## 修订记录

| 修订 | 接受日期 | 状态 / 证据 | 语义摘要 |
| --- | --- | --- | --- |
| R0 | 2026-07-12 | `e7db92d7` accepted；`83ff742d` closed；[R0 事务](../../devlog/transactions/2026-07-12-sched-rt-class.md) | 建立共享 `Realtime` class、FIFO/RR policy、typed priority、priority bucket、RR quantum 与 compile-time selector。 |
| R1 | 2026-07-14 | `39ba07a9` implemented；[R1 事务](../../devlog/transactions/2026-07-14-sched-rt-class-r1.md) Completed | 删除 class-visible resched cause continuation；RR entity 显式拥有 committed rotation obligation，core pending 收窄为单 bit pending-pick snapshot。 |

## 方案

### Class、Policy 与 Priority

第一版内部形状为：

```rust
struct RtEntity {
    priority: RtPriority,
    policy: RtPolicy,
}

enum RtPolicy {
    Fifo,
    RoundRobin {
        remaining_ticks: u32,
        rotation_due: bool,
    },
}
```

`RtPriority` 的合法范围固定为 `1..=99`，数值越大优先级越高。`0` 不是 RT priority。ABI 编码、权限和用户指针不进入该类型；未来 syscall RFC 只能在 ABI 边界完成解析，再把 typed priority 交给 owner-CPU scheduler transaction。

`RtEntity` 位于 `SchedEntity` 的 class-specific payload 中。不得再在 `Task`、ABI accounting 或 runqueue node 中缓存一份 effective policy / priority。`remaining_ticks` 是 RR 当前 budget 的唯一真相源；`rotation_due` 是只属于 active current 的已提交队尾放置义务，不是 processor pending state。

### Fresh Constructor 与编译期 Default

`SchedEntity` 对共享调用者只暴露两个语义构造入口：

```text
new_default()      -> 根据 Kconfig 创建 fresh Fair、RT/FIFO 或 RT/RR
new_idle()         -> 创建 idle entity
```

不保留与 `new_default()` 重叠的 `new_normal()`。ordinary task、clone child、bootstrap task、`kthreadd` 和普通 kthread 都调用 `new_default()`；idle task 继续只调用 `new_idle()`。

Kconfig 使用单一受约束 selector：

```toml
sched_default_policy = "fair" # fair | rt_rr | rt_fifo
```

不得用多个互斥 boolean 表达 selector。选择 RT/FIFO 或 RT/RR 时，所有非 idle task 都以 `RtPriority::MIN`，即 priority 1，创建 fresh RT entity；该值没有 ABI default 含义，也不进入 Kconfig。

显式 typed priority / policy 的 fresh RT entity 只由 `rt.rs` 的 class/test-private helper 构造；共享 `SchedEntity` facade 不公开 `new_realtime()`，也不 re-export RT runtime policy representation。未来 production custom construction 或运行期 policy change 必须另开 RFC；后者必须由 owner CPU 同时修改 class payload、queue membership 与 preempt decision。

`SchedEntity::{new_default,new_idle}` 的公开 facade constructors 位于共享 `entity.rs`；`rt.rs` 提供窄的 RT payload factory、custom fresh helper、policy/quantum 校验与 class 算法。`SchedEntity` 和其 class payload 不提供 `Clone`，Task 的 entity lock 也不向 scheduler-class owner 外提供可构造的 whole-entity mutation capability，避免把 published runtime state 复制或替换成另一个 entity。

clone 在本 RFC 中仍创建 fresh default entity。RT/RR 默认构建下，child 获得完整的新 quantum，而不是复制 parent 的剩余 budget。

### Cross-Class Precedence 与饥饿边界

跨 class precedence 固定为：

```text
Realtime > Fair > Idle
```

只要 owner CPU 上存在 runnable RT task，Fair 和 Idle task 就不会被选择；没有 RT task 时，Fair 高于 Idle。第一版没有 RT bandwidth controller，因此一个永不阻塞、永不 yield 的 FIFO task 可以无限期饿死该 CPU 上较低优先级的 RT task、同优先级但依赖 cooperative yield 的 peer，以及 Fair task。这是接受的第一版语义，不是 fallback 或 anomaly。

编译期默认 RT/FIFO 测试必须使用受控 workload；不得依靠降低 class precedence、插入隐式 tick rotation 或给 service kthread 增加未记录的特例来避免合法 starvation。

### RT Ready Queue

第一版 `Realtime` class 使用固定 priority bucket：

```rust
struct Realtime {
    queues: [VecDeque<Arc<Task>>; RtPriority::WIDTH],
}
```

其中 `RtPriority::WIDTH == 99`。FIFO 与 RR task 在相同 priority 下共享同一 bucket，policy 不参与 tie-break。pick 从 priority 99 向 1 扫描，返回最高非空 bucket 的队头。

第一版不使用 bitmap。最多扫描 99 个 bucket 是固定上界；empty `VecDeque` 不分配 backing storage。若后续证明 bucket scan 成为 hot-path 问题，可以在不改变 queue semantics 的前提下单独增加 bitmap gate。

### Priority-First 排队语义

同一 RT class 内遵守以下规则：

- candidate priority 严格高于 current 时请求 resched。
- candidate 与 current priority 相等或更低时不因 arrival 抢占。
- new task 与普通 wake 进入各自 priority bucket 的队尾。
- 显式 `sched_yield()` 进入同 priority 队尾。
- current 被更高 priority task 抢占时，若没有已提交 rotation obligation，则回到自身 priority 队头，保留原执行次序；若 `rotation_due` 已为真，则该义务优先，回到队尾。
- FIFO 不因 tick 请求 resched。
- RR quantum 到期时，只有同 priority bucket 存在 peer 才提交 rotation obligation 并请求 resched；该义务使 current 在后续离开 active segment 时进入队尾。低 priority peer 不触发新的 rotation。

RR quantum 到期且存在同 priority peer 时，先在 RR entity 中提交 `rotation_due = true`，再请求一次 full pick；arrival 不能把已经提交队尾义务的 current 放回队头。FIFO 不产生 tick rotation。preempt transaction 只消费 entity obligation，不重新判断 request cause、peer 或 pending slot。

`rotation_due` 是 RT-owned placement obligation。只有 active current 可以置真；fresh、queued、blocked、exiting task 必须为假。延迟 tick 保持该义务；preempt / yield / handoff / block / exit 分别按 lifecycle 合同消费或清除它。多个 expiry 合并为一个 bool，不累计 processor 事件。

### RR Quantum

RR 的时间目标进入 Kconfig：

```toml
rt_rr_timeslice_ms = 100
```

100ms 不是“最优值”证明，而是 Linux 长期采用的每秒约 10 个 quantum 的兼容性基线。实际 full budget 由 `SYSTEM_HZ` 动态换算：

```text
full_quantum_ticks =
    max(1, ceil(RT_RR_TIMESLICE_MS * SYSTEM_HZ / 1000))
```

换算使用足够宽的中间类型，并在编译期断言 `SYSTEM_HZ > 0`、timeslice 非零且结果可以表示。量化误差小于一个 tick。未来若实现 `sched_rr_get_interval()`，应报告换算后的 effective quantum，而不是声称超过 tick 分辨率的精度。

RR lifecycle：

- fresh RR entity 以 full budget 开始。
- 每个 scheduler tick 消耗一个 tick。
- 到期时立即补满；有同 priority peer 则提交 rotation obligation 并请求 resched，无 peer 则不新增 obligation / request 并继续运行。已有 obligation 始终保留到 lifecycle transaction 消费。
- higher-priority preempt、显式 yield、正常 block/wake 和 `handoff_woken_current()` 都不重置剩余 budget。
- `remaining_ticks` 在持久状态中保持 `1..=full_quantum_ticks`；到期路径不留下零值。`rotation_due` 不改变 budget，也不是 budget 是否 full 的证明。

### Wait-Tail 语义

wait-core 的状态所有权保持不变。RT class 只消费既有 method-first transaction：

- 正常 block：`put_prev_blocked()` 不入队；之后 `enqueue_woken()` 入队尾。
- wait 在 park 前已经完成且 scheduler 不切换：不调用 class transaction，不执行 full pick，位置、RR budget 与 processor pending slot 都不变。
- `handoff_woken_current()`：按逻辑 wake 处理，进入同 priority 队尾，保留 RR budget。
- exiting task 不再入队。

RT class 不读取 wait identity、`PrePark/Parked` 或 wake token，也不新增 wait-specific policy state。

## 验证边界

priority correctness 通过理论、source audit 与 focused KUnit 闭合：

- `RtPriority` 边界与 queue index。
- 99 个 bucket 的降序选择。
- 严格高 priority preemption 与同 priority no-preempt。
- higher-priority preempt 在无 rotation obligation 时回队头。
- expiry 后延迟 full pick、preempt tail/head、yield/handoff clear、block/exit clear 以及 peer 消失后的已提交 rotation。
- `Realtime > Fair > Idle`。
- FIFO policy 无 quantum state。
- RR decrement、补满、无 peer continuation 与有 peer rotation。

用户态验证边界为默认 RT/RR 下的整体集成与 lifecycle 稳定性：

- RT/RR build 已由用户完整运行整套 LTP 测例，作为真实用户态 workload、yield、block/wake 与长链路调度集成证据；该记录不表示每个 LTP case 都通过。
- RT/FIFO selector 已通过 build，FIFO no-timeslice 与 shared-class lifecycle 由 source audit 和 focused KUnit 覆盖；本 RFC 不再要求 FIFO 用户态专项 smoke 才能收口。
- 不要求用户态制造不同 priority，不增加 test-only syscall、隐藏 setter 或临时 service-kthread priority。
- 不以精确 quantum、吞吐比例或 latency bound 作为第一版 gate。

## 接受边界

接受本 RFC 意味着：

- FIFO/RR 共享一个 priority-first `Realtime` class。
- effective RT policy、priority、RR remaining budget 与 committed rotation obligation 由 `RtEntity` 单独拥有。
- 编译期 default selector 是没有属性 syscall 时的正式验证入口。
- compile-time default selector 由 shared class domain 拥有；本 RFC 只定义其中 `rt_fifo` / `rt_rr` 的构造与行为。
- 第一版运行时集成以用户完成的 RT/RR 整套 LTP 运行为关闭证据；FIFO 用户态专项验证明确未运行且不阻塞本 RFC。
- 第一版明确接受无 bandwidth control 导致的 RT starvation。
- `Scheduler` trait surface 足够；preempted-current transaction 不读取 core pending cause，RT rotation 由 RR entity 自己提交和消费。

接受本 RFC 不表示：

- 用户态已经可以设置或查询调度 policy / priority。
- published task 可以非事务性地修改 class、policy 或 priority。
- FIFO/RR 具有 hard realtime guarantee。
- 不同 priority 的用户态 runtime 测试已经完成。
- R1 的关闭不表示调度属性 syscall、RT bandwidth、跨 CPU migration 或 archived EEVDF 已经实现；这些仍属于独立后续边界。

## 备选方案

### FIFO 与 RR 分成两个 Class

拒绝。固定 class precedence 无法表达跨 policy 的 RT priority ordering，也会制造重复 queue 与 preempt logic。

### 只实现 Dormant RT Class

拒绝作为完成边界。没有 production placement 就无法验证真实 tick、yield、block/wake 和 switch lifecycle；编译期 default selector 提供正式入口。

### 单个线性 RT Queue

拒绝作为第一选择。它会让 pick、同 priority peer 检查和“插入本 priority 队头”都进行全队列扫描；固定 bucket 更直接地保存 FIFO 不变量。

### 为 Default RT Priority 增加 Kconfig

拒绝。所有 compile-default RT task 使用相同 priority，配置该值不改变本 RFC 的测试语义。priority 1 作为代码内最小合法值即可。

### 根据 Runnable 数动态调整 RR Quantum

拒绝。它会让同一 policy 的 interval 随负载变化，并使未来 `sched_rr_get_interval()` 无法返回稳定值。第一版只根据配置时间目标与 `SYSTEM_HZ` 做确定性换算。

### 临时属性 Syscall 或 Test-Only Setter

拒绝。本 RFC 不为测试绕过未来 owner-CPU policy transaction，也不制造第二份 effective policy/priority state。

## 风险

- RT/FIFO default build 可能因真实的同 priority cooperative behavior 暴露长期不阻塞、不 yield 的任务；不得自动归类为 scheduler bug。
- 反之，如果 tick 导致 FIFO peer 自动轮转，则属于明确算法错误。
- RR quantum 只能达到 tick 分辨率；`SYSTEM_HZ` 很低时 effective quantum 会明显量化。
- 99 个 empty bucket 增加每 CPU 固定容器元数据，但避免更复杂的线性插入与选择逻辑。
- bucket 的 `VecDeque` 首次 materialize 或扩容可能在 owner-CPU noirq transaction 中触发堆分配；这与 legacy `RoundRobin` 已有的 ready-queue 风险相同，本 RFC 暂接受并沿用该限制，不把它描述为 allocation-free。对应风险由 [ANE-20260622-IRQ-OFF-HEAP-ALLOCATION](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation) 跟踪；未来若改成预分配或 intrusive queue，必须另开 gate。
- 编译期 selector 要求所有非 idle task creation 收敛到 `new_default()`；若仍保留并列 default constructor，会重新制造默认 policy 的多重入口。
- 在没有 ABI syscall 的阶段，procfs 或其它观察面可能仍显示旧的普通调度字段；本 RFC 不以伪造 read-back 掩盖该缺口。

## 收口

R0 已收口：共享 `Realtime` class、FIFO/RR policy、priority bucket、RR quantum、class dispatch 与 capability-gated entity mutation 均已实现；RT/RR 与 RT/FIFO selector build、focused KUnit、source audit 和独立 review 已通过。用户在 RT/RR 默认配置下完整运行了整套 LTP 测例，作为用户态集成证据；这不表示所有 LTP case 均通过。FIFO 用户态专项验证未运行，并按用户裁定不作为第一版关闭条件。

R1 接受的修正是：将 rotation obligation 归属 RR entity，并把 pending 收窄为 core-only full-pick snapshot；Fair/Idle 只做 trait 机械适配，算法不变。实现证据见 [R1 事务日志](../../devlog/transactions/2026-07-14-sched-rt-class-r1.md)。

noirq `VecDeque` allocation 仍是已登记限制。ABI 调度属性、动态 policy transaction、bandwidth control、不同 priority runtime validation 与未来 FIFO 专项验证均不在 R1 内。
