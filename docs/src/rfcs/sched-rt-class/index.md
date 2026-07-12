# RFC-20260711-sched-rt-class

**状态：** Draft
**负责人：** doruche, Codex
**最后更新：** 2026-07-12
**领域：** scheduler / realtime / FIFO / RR / scheduler class
**事务日志：** None
**开放问题：** 见 [Tracking Issues](./tracking-issues.md)
**下一步：** 执行 [迁移实施计划](./implementation.md) 的 Scheduler-Core 前置 Gate；通过后按 checkpoint 开始 RT class 实现。

## 摘要

本 RFC 定义 Anemone 第一版 realtime scheduler class。`SCHED_FIFO` 与 `SCHED_RR` 不实现为两个具有固定先后顺序的 scheduler class，而是同一个 `Realtime` class 下的两种 task policy。二者共享 `1..=99` 的 typed RT priority domain 和同一组按 priority 分桶的 FIFO ready queue；RR 只在 FIFO 语义之上增加 tick-based quantum。

本 RFC 只把 method-first `Scheduler` trait、`RunQueue` dispatch、`SchedEntity` class payload 和 typed `PendingResched` 视为下层接口合同。

第一版不实现调度属性 syscall，也不允许运行期修改已发布 task 的 class、RT policy 或 priority。为了在没有 `sched_setscheduler()` 的情况下进行真实用户态验证，Kconfig 可以在编译期选择所有非 idle task 的默认 policy：RT/FIFO 或 RT/RR。默认值沿用当前 legacy RoundRobin 行为。

## 背景

本 RFC 假定 scheduler class domain 提供以下稳定接缝：

- `RunQueue` 统一分发 class-local lifecycle transaction。
- `SchedEntity` 保存 `on_runq` 与 class-specific payload。
- `SchedClassKind` 只提供 class identity snapshot；跨 class precedence 集中定义。
- current requeue 已区分 yield、involuntary preempt 与 parked wake handoff；wait no-switch abort 不调用 class transaction。
- `PendingResched` 区分 Tick 与 RunnableArrival，是面向下一次 owner-CPU full pick 的 scheduler-core 合并 latch，不是 class policy state。successful pick 确认此前 slot，no-pick 路径保留或恢复。
- owner CPU、noirq transaction、wait-core logical state 与 physical runqueue membership 的所有权已经闭合。

待替换的 legacy `RoundRobin` class 只有单个 `VecDeque`，没有 RT priority，也没有独立 quantum state；其 tick 路径每个 tick 都请求 resched。第一版 RT 工作应替换这份遗留 class，而不是在其旁边再增加独立 FIFO class。

把 FIFO 与 RR 拆成两个 scheduler class 无法表达 Linux/POSIX 的 priority-first 语义。例如 RR priority 99 必须高于 FIFO priority 1，而固定 class precedence 无法同时满足这一点。因此 policy 必须位于一个共享的 RT class 内部。

## 目标

- 引入单一 `Realtime` scheduler class，并删除遗留的独立 `RoundRobin` class identity。
- 在 `RtEntity` 中保存 effective RT priority 与 policy，作为真实调度行为的唯一真相源。
- 使用 typed `RtPriority` 约束 `1..=99`；数值越大，调度优先级越高。
- 使用 `RtPolicy::Fifo` 与 `RtPolicy::RoundRobin { remaining_ticks }` 表达 policy；FIFO 的类型形态中不存在无效 quantum state。
- 为每个 RT priority 提供一个共享 FIFO/RR ready queue；第一版固定 99 个 bucket，不引入 bitmap。
- 固定跨 class precedence 为 `Realtime > Idle`。
- 实现严格高 priority arrival preemption、同 priority 不抢占、FIFO no-timeslice 和 RR quantum rotation。
- 复用 `Scheduler` trait 的 method-first class transaction，不新增 catch-all event 或调度 reason bus。
- 增加 `SchedEntity::new_realtime(...)` 与 `SchedEntity::new_default()`；所有非 idle production 创建路径统一使用 `new_default()`。
- 通过单一 Kconfig selector 选择 `rt_fifo` 或 `rt_rr`；默认值沿用当前 legacy RoundRobin 行为。
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
    },
}
```

`RtPriority` 的合法范围固定为 `1..=99`，数值越大优先级越高。`0` 不是 RT priority。ABI 编码、权限和用户指针不进入该类型；未来 syscall RFC 只能在 ABI 边界完成解析，再把 typed priority 交给 owner-CPU scheduler transaction。

`RtEntity` 位于 `SchedEntity` 的 class-specific payload 中。不得再在 `Task`、ABI accounting 或 runqueue node 中缓存一份 effective policy / priority。`remaining_ticks` 是 RR 当前 budget 的唯一真相源，并直接位于 `RoundRobin` 变体内。

### Fresh Constructor 与编译期 Default

`SchedEntity` 对本 RFC 暴露三个语义构造入口：

```text
new_default()      -> 根据 Kconfig 创建 fresh RT/FIFO 或 RT/RR
new_realtime(...) -> 创建显式 typed priority / policy 的 fresh RT entity
new_idle()         -> 创建 idle entity
```

不保留与 `new_default()` 重叠的 `new_normal()`。ordinary task、clone child、bootstrap task、`kthreadd` 和普通 kthread 都调用 `new_default()`；idle task 继续只调用 `new_idle()`。

Kconfig 使用单一受约束 selector：

```toml
sched_default_policy = "rt_rr" # rt_rr | rt_fifo
```

不得用多个互斥 boolean 表达 selector。选择 RT/FIFO 或 RT/RR 时，所有非 idle task 都以 `RtPriority::MIN`，即 priority 1，创建 fresh RT entity；该值没有 ABI default 含义，也不进入 Kconfig。

`new_realtime()` 只构造未发布的 fresh entity，不是 published-task mutation API。未来运行期 policy change 必须另开 RFC，并由 owner CPU 同时修改 class payload、queue membership 与 preempt decision。

clone 在本 RFC 中仍创建 fresh default entity。RT/RR 默认构建下，child 获得完整的新 quantum，而不是复制 parent 的剩余 budget。

### Cross-Class Precedence 与饥饿边界

跨 class precedence 固定为：

```text
Realtime > Idle
```

只要 owner CPU 上存在 runnable RT task，idle task 就不会被选择。第一版没有 RT bandwidth controller，因此一个永不阻塞、永不 yield 的 FIFO task 可以无限期饿死该 CPU 上较低优先级的 RT task，以及同优先级但依赖 cooperative yield 的 peer。这是接受的第一版语义，不是 fallback 或 anomaly。

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
- current 被更高 priority task 抢占时回到自身 priority 队头，保留原执行次序。
- FIFO 不因 tick 请求 resched。
- RR quantum 到期时，只有同 priority bucket 存在 peer 才请求 resched并进入队尾；低 priority peer 不触发 rotation。

若 Tick 与 RunnableArrival 同时 pending，RR 已到期的队尾语义优先；arrival 不能把已经耗尽 quantum 的 current 放回队头。FIFO 不产生 Tick request，因此其 arrival preempt 始终回队头。

这里读取的 `PendingResched` 是进入 preempted-current transaction 时捕获的 pre-pick 值 snapshot。snapshot 捕获后到本次 full pick 前 current 不会更换；successful pick 只确认 processor slot，RT class 不保存 snapshot 或任何 task-local resched state。

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
- 到期时立即补满；有同 priority peer 则请求 resched，无 peer 则继续运行。
- higher-priority preempt、显式 yield、正常 block/wake 和 `handoff_woken_current()` 都不重置剩余 budget。
- `remaining_ticks` 在持久状态中保持 `1..=full_quantum_ticks`；到期路径不留下零值。

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
- higher-priority preempt 回队头。
- Tick / RunnableArrival 同时 pending 时的到期队尾规则。
- `Realtime > Idle`。
- FIFO policy 无 quantum state。
- RR decrement、补满、无 peer continuation 与有 peer rotation。

用户态只验证同 priority lifecycle：

- RT/RR build：多个 CPU-bound worker 不主动 yield，仍都获得进展。
- RT/FIFO build：受控 worker 在显式 yield 前证明同 priority peer 不会因 tick 自动运行；yield 后 peer 获得执行。
- 受控 yield 与 block/wake workload 证明真实 class transaction 接线。
- 不要求用户态制造不同 priority，不增加 test-only syscall、隐藏 setter 或临时 service-kthread priority。
- 不以精确 quantum、吞吐比例或 latency bound 作为第一版 gate。

具体 test app、rootfs 路由、运行顺序和 agent/user 分工留待 `implementation.md` 后续收敛。

## 接受边界

接受本 RFC 意味着：

- FIFO/RR 共享一个 priority-first `Realtime` class。
- effective RT policy、priority 与 RR remaining budget 由 `RtEntity` 单独拥有。
- 编译期 default selector 是没有属性 syscall 时的正式验证入口。
- compile-time default selector 只在 RT/FIFO 与 RT/RR 之间选择；默认值沿用当前 legacy RoundRobin 行为。
- 第一版明确接受无 bandwidth control 导致的 RT starvation。
- `Scheduler` trait surface 足够，不因 RT 引入 catch-all event 或新的 core policy state。

接受本 RFC 不表示：

- 用户态已经可以设置或查询调度 policy / priority。
- published task 可以非事务性地修改 class、policy 或 priority。
- FIFO/RR 具有 hard realtime guarantee。
- 不同 priority 的用户态 runtime 测试已经完成。
- 具体实现阶段、write set 和 review gate 已经关闭。

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
- 编译期 selector 要求所有非 idle task creation 收敛到 `new_default()`；若仍保留并列 default constructor，会重新制造默认 policy 的多重入口。
- 在没有 ABI syscall 的阶段，procfs 或其它观察面可能仍显示旧的普通调度字段；本 RFC 不以伪造 read-back 掩盖该缺口。

## 收口

本 RFC 的第一版收口应区分：

- class 算法和 transaction 接线已实现；
- RT/RR 与 RT/FIFO 两种 compile-time default 构建状态；
- source/KUnit 已证明的 priority 语义；
- 用户态已运行和未运行的同 priority smoke；
- 明确延期的 ABI、动态 policy transaction、bandwidth control 与 priority runtime validation。

具体阶段和证据记录格式待后续 `implementation.md` 收敛后确定。
