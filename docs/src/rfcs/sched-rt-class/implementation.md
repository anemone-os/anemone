# Sched RT Class 迁移实施计划

**状态：** Draft；实施阶段已收敛，尚未开始
**最后更新：** 2026-07-12
**父 RFC：** [RFC-20260711-sched-rt-class](./index.md)
**不变量：** [不变量需求](./invariants.md)

本计划把 RT class 实现限制在现有 scheduler-class owner boundary 内。算法只增加一个共享的 `Realtime` class、99 个 priority bucket 和 FIFO/RR 两种 policy；不新增 scheduler-core event、task-local pending state、wait-specific transaction 或运行期属性 mutation API。

实现顺序按“前置合同、class-local 算法、RunQueue 接线、默认构造与配置、集成验证”推进。每个 checkpoint 都有独立的 write set 和停止条件；前一个 gate 未通过时，不继续扩大后续写集。

## 实施目标

最终实现必须满足：

- FIFO 与 RR 由同一个 `Realtime` class owner 实现；
- `RtEntity` 单独拥有 effective priority、policy 和 RR remaining budget；
- `Realtime` 使用固定 99 个 priority bucket，pick 从高到低选择非空 bucket 的队头；
- FIFO 不因 tick 请求 resched，RR 只在 quantum 到期且存在同 priority peer 时请求 resched；
- `PendingResched` 只作为 preempted-current transaction 的按值 snapshot 输入；
- 所有非 idle production 创建路径收敛到 `new_default()`，idle 仍使用 `new_idle()`；
- RT/RR 与 RT/FIFO 都能完成构建，并通过批准的 focused KUnit 与用户态 smoke。

## 前置 Gate：Scheduler-Core 合同

### 目的

确认 RT class 可以直接消费现有 scheduler-core 合同，不在本 RFC 实现中修补 core 状态。

### 最小写集

本 gate 默认不修改代码。若 source audit 发现合同不成立，必须停止并回到 scheduler-core 对应计划；不得在 RT class 中增加兼容字段或旁路 transaction。

### 必须确认

- successful owner-CPU full pick 确认 pick 前的 processor pending slot；
- deferred preempt 和 wait no-switch abort 不确认 slot，destructive-take caller 保持 restore 责任；
- `requeue_aborted_wait_current()` 已从 method-first class surface 删除；
- wait no-switch abort 不调用 class transaction，parked wake 只通过 `handoff_woken_current()` 收口；
- RT class 读取的 pending bits 是 pre-pick snapshot，不保存 processor slot 或 task-local resched state。

### Gate 验证

- source audit 覆盖 `PendingResched` 的 take/restore/acknowledge 路径；
- source audit 确认 no-pick 路径不会消费 pending cause；
- `rg` 确认 RT 写集不引入第二份 pending、wait identity 或 abort-park class transaction。

### 停止条件

如果上述合同不能由当前 scheduler core 证明，停止 RT 实现，先更新 scheduler-core 计划及其 transaction 记录。本 RFC 不通过复制状态来绕过该 gate。

## Checkpoint 1：RT Entity 与 Class-Local 算法

### 写集

- `anemone-kernel/src/sched/class/entity.rs`
- `anemone-kernel/src/sched/class/rt.rs`（新增）
- `anemone-kernel/src/sched/class/mod.rs`（仅声明新 module，不改变 production dispatch）

### 实现内容

- 增加 `RtPriority`，严格校验 `1..=99`，提供无歧义的 bucket index 映射；
- 增加 `RtPolicy::Fifo` 与 `RtPolicy::RoundRobin { remaining_ticks }`；
- 增加 `RtEntity`，使 policy、priority 和 RR budget 只有一个行为真相源；
- 增加 `Realtime` class 的 99 个 `VecDeque<Arc<Task>>` bucket；
- 实现 enqueue、dequeue、pick 和各类 current requeue 的 placement 规则；
- 实现 FIFO/RR tick decision、strict higher-priority arrival preemption 和同 priority no-preempt；
- 本 checkpoint 不改变 production class identity 或 RunQueue dispatch；`Realtime` identity 与 trait 接线留到 Checkpoint 2，避免中间态出现 class identity 与 dispatch 不匹配。

### Placement 规则

- new、wake、yield 和普通 handoff 进入 priority bucket 队尾；
- higher-priority arrival 抢占时，current 回到自身 bucket 队头；
- RR quantum 到期且存在同 priority peer 时，current 进入队尾并补满 budget；
- FIFO tick 永远返回 `TickAction::None`；
- `PendingResched` 同时包含 Tick 与 RunnableArrival 时，已到期 RR current 使用队尾 placement；
- block、exit 和 no-switch abort 不在 class 内重新入队，也不重置 RR budget。

### 验证

- `RtPriority` 边界、排序和 bucket mapping 的 focused KUnit；
- RR budget decrement/refill、FIFO no-budget 和 peer/no-peer decision 的 focused KUnit；
- source audit 确认 pending snapshot 不写回 `RtEntity`，queue node 不复制 priority；
- `git diff --check`。

### 停止条件

如果算法需要 task-local pending state、第二份 effective priority，或修改 `Scheduler` trait 才能表达上述 placement，停止并回到 RFC review；不得在本 checkpoint 扩展成 catch-all event。

## Checkpoint 2：RunQueue 接线与 Legacy Class 替换

### 写集

- `anemone-kernel/src/sched/class/runqueue.rs`
- `anemone-kernel/src/sched/class/mod.rs`
- `anemone-kernel/src/sched/class/rt.rs`
- 删除 `anemone-kernel/src/sched/class/rr.rs`

### 实现内容

- 将 `RunQueue` 的 `RoundRobin` owner 替换为 `Realtime`；
- 将集中 class precedence 固定为 `Realtime > Idle`；
- 把 enqueue、dequeue、yield、preempt、wake handoff、block、exit、pick、tick 和 preempt dispatch 全部接到 `Realtime`；
- 保持 `RunQueue` 对跨 class precedence 的单一消费，不在 `Realtime` 内复制 class rank；
- 删除 legacy `RoundRobin` class identity、queue owner 和行为实现，不保留并列 fallback；
- 保持 `processor.rs`、wait-core 和 pending-resched 的 owner 与调用协议不变。

### 验证

- RunQueue lifecycle 的 focused KUnit 或等价 source-level coverage；
- 验证最高非空 priority bucket pick、同 priority FIFO/RR 混排和 higher-priority head requeue；
- 验证 `Realtime > Idle`，以及 Idle 只在 RT queue 为空时被选择；
- source audit 确认无 `SchedClassKind::RoundRobin`、无独立 FIFO class、无 legacy queue owner；
- `git diff --check`。

### 停止条件

如果 RunQueue 接线要求修改 pending acknowledgement、wait transaction 或跨 CPU ownership，停止并提交 write-set expansion；不得把 core 改动隐含在 class 替换中。

## Checkpoint 3：Default Constructor 与 Kconfig 接线

### 写集

- `anemone-kernel/src/sched/class/entity.rs`
- `anemone-kernel/src/arch/riscv64/bootstrap.rs`
- `anemone-kernel/src/arch/loongarch64/bootstrap.rs`
- `anemone-kernel/src/task/api/clone/mod.rs`
- `anemone-kernel/src/task/kthread/kthreadd.rs`
- `kconfig`
- `scripts/xtask/src/config/kconfig.rs`
- 构建生成的 `anemone-kernel/src/kconfig_defs.rs`（只由构建流程生成）

### 实现内容

- 增加 `SchedEntity::new_realtime(...)` 和 `SchedEntity::new_default()`；
- 删除与 `new_default()` 重叠的 `new_normal()`；
- 将 bootstrap、clone、`kthreadd` 和普通 kthread 的非 idle 创建路径迁移到 `new_default()`；
- 在配置层增加受约束的 default policy selector：`rt_rr | rt_fifo`；
- 增加 RR 时间目标 `rt_rr_timeslice_ms`，由 `SYSTEM_HZ` 换算为 tick budget；
- 默认 RT priority 固定为代码内 `RtPriority::MIN`，不增加 priority Kconfig 镜像；
- selector 只影响 fresh construction，不重新解释已经发布的 entity。

建议的配置形状为：

```toml
sched_default_policy = "rt_rr" # rt_rr | rt_fifo
rt_rr_timeslice_ms = 100
```

selector 应在 xtask 配置层解析为受约束 enum，并生成 kernel 可消费的 typed constant；不使用多个互斥 boolean，也不让 `new_default()` 依赖运行期字符串解析或隐藏 fallback。

### 验证

- 两个 selector 值都能生成有效 kernel configuration；
- `new_default()` 在 RT/RR 下产生完整 fresh quantum，在 RT/FIFO 下不携带 quantum state；
- source audit 确认所有非 idle production constructor 都已收敛到 `new_default()`；
- source audit 确认没有 published-task policy/priority setter 或 test-only mutation bridge；
- `git diff --check`。

### 停止条件

如果配置生成链无法提供受约束 selector，停止并先修复 build/config owner；不得在 kernel 内部接受任意字符串、多个互斥 flag 或默认值旁路。

## 最终验证 Gate

### Agent-run 验证

- RT/RR compile-time default build；
- RT/FIFO compile-time default build；
- priority、bucket、preempt、quantum、constructor selector 的 focused KUnit；
- source audit：无独立 FIFO class、无 legacy RR owner、无第二份 effective policy/priority、无 task-local pending state、无 `requeue_aborted_wait_current()`；
- `just build`、`git diff --check` 和 RFC 文档 whitespace 检查。

### User-run 验证

- RT/RR 同 priority CPU-bound worker progress smoke；
- RT/FIFO 受控 no-timeslice、explicit-yield 和 block/wake smoke；
- smoke harness 必须有 watchdog 和明确退出条件，不能把合法 FIFO starvation 误判为 kernel hang。

### 不运行或不纳入本 RFC 的验证

- 不通过 test-only syscall、隐藏 setter 或临时 service-kthread priority 制造不同 priority workload；
- 不以精确 quantum、吞吐比例、latency bound 或 hard realtime guarantee 作为 gate；
- 不运行 broad LTP 作为 RT class 的唯一验收依据；
- ABI policy syscall、动态 policy transaction、bandwidth control、priority runtime ordering 和 procfs observation 留给后续工作。

## 总体停止边界

以下任一情况出现时，停止当前 checkpoint 并回到 RFC review 或 write-set review：

- 需要修改 `PendingResched` 的 owner 或 acknowledgement 语义；
- 需要重新引入 abort-park class transaction；
- 需要在 Task、queue node 或 ABI accounting 中复制 RT policy/priority；
- 需要增加跨 CPU queue lock、remote entity mutation 或运行期属性 setter；
- 需要为了通过 smoke 隐式 tick rotation、service-task 特例或更弱的 starvation 语义；
- 需要扩大到其它 scheduler class 的内部算法。

实现期事实、checkpoint 结果和验证证据写入 transaction devlog；只有当阶段顺序、write set、验证 floor、不变量或接受边界发生变化时，才回写本计划或 `index.md` / `invariants.md`。

transaction 建立和后续导航更新留待本计划对应实现 gate 通过后进行；本 RFC 已完成文档层提升。
