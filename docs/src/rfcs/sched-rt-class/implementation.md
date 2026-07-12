# Sched RT Class 迁移实施计划

**状态：** Active；Scheduler-Core 前置 Gate 已关闭
**最后更新：** 2026-07-12
**父 RFC：** [RFC-20260711-sched-rt-class](./index.md)
**不变量：** [不变量需求](./invariants.md)

本计划把 RT class 实现限制在现有 scheduler-class owner boundary 内。算法只增加一个共享的 `Realtime` class、99 个 priority bucket 和 FIFO/RR 两种 policy；不新增 scheduler-core event、task-local pending state、wait-specific transaction 或运行期属性 mutation API。

实现顺序按“前置合同、RT class 原子切换、集成验证”推进。每个 checkpoint 都有独立的 write set 和停止条件；前一个 gate 未通过时，不继续扩大后续写集。

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

## Checkpoint 1：RT Class 原子切换

### 调整理由

文档层 review 证明 class payload、class identity、`RunQueue` exhaustive dispatch、legacy owner 删除、default constructor 和 RR full quantum 不能拆成彼此不可编译的 checkpoint。用户已批准在不削弱功能或不变量的前提下调整文件切分，因此本 checkpoint 把这些同一 scheduler-class owner 的表面合并为一个原子切换；不保留 `RoundRobin -> Realtime` identity 伪装、双 queue fallback 或临时 hard-coded quantum。

### 写集

- `anemone-kernel/src/sched/class/entity.rs`
- `anemone-kernel/src/sched/class/rt.rs`（新增）
- `anemone-kernel/src/sched/class/mod.rs`
- `anemone-kernel/src/sched/class/runqueue.rs`
- 删除 `anemone-kernel/src/sched/class/rr.rs`
- `anemone-kernel/src/arch/riscv64/bootstrap.rs`
- `anemone-kernel/src/arch/loongarch64/bootstrap.rs`
- `anemone-kernel/src/task/api/clone/mod.rs`
- `anemone-kernel/src/task/kthread/kthreadd.rs`
- `conf/.defconfig`
- 根 `kconfig`（gitignored live build input；只同步/切换本 checkpoint 新增的 selector 与 timeslice，保留其它开发者本地选项，不提交）
- `scripts/xtask/src/config/kconfig.rs`
- 构建生成的 `anemone-kernel/src/kconfig_defs.rs`（只由 repository build flow 生成）

本 checkpoint 不修改 `processor.rs`、wait-core、trap/IPI pending plumbing、task topology、调度属性 syscall 或其它 scheduler class 算法。若实际编译要求越过上述文件，worker 必须先提交 expansion request；不得在已批准写集内制造 adapter 绕过。

### 实现内容

- 增加 `RtPriority`，严格校验 `1..=99`，提供无歧义的 bucket index 映射；
- 增加 `RtPolicy::Fifo`、`RtPolicy::RoundRobin { remaining_ticks }` 与 `RtEntity`，使 policy、priority 和 RR budget 只有一个行为真相源；
- 增加单一 `Realtime` identity 和 99 个 priority bucket，删除 legacy `RoundRobin` identity、queue owner 与实现；
- 将集中 precedence 固定为 `Realtime > Idle`，把全部 method-first lifecycle dispatch 接到 `Realtime`；
- 实现 enqueue、dequeue、pick、current placement、FIFO/RR tick 和 strict higher-priority arrival decision；
- 增加 `SchedEntity::new_realtime(...)` 与 `new_default()`，删除 `new_normal()`，并原子迁移所有非 idle production constructor；
- 在 `conf/.defconfig` / xtask 配置 owner 中增加受约束的 `sched_default_policy = "rt_rr" | "rt_fifo"` 和 `rt_rr_timeslice_ms = 100`；同步 live 根 `kconfig` 的新增键但不覆盖其它本地配置；selector 解析为 enum，并生成 kernel 可直接消费的 typed constant；
- full quantum 使用 `max(1, ceil(rt_rr_timeslice_ms * SYSTEM_HZ / 1000))`，使用足够宽的中间类型并拒绝零值/溢出；不在 entity 中复制 full quantum；
- 默认 priority 固定为代码内 `RtPriority::MIN`；selector 只影响 fresh construction。

### Placement 规则

- new、wake、yield 和普通 handoff 进入 priority bucket 队尾；
- higher-priority arrival 抢占时，current 回到自身 bucket 队头；
- RR quantum 到期且存在同 priority peer 时，current 进入队尾并补满 budget；
- FIFO tick 永远返回 `TickAction::None`；
- `PendingResched` 同时包含 Tick 与 RunnableArrival 时，已到期 RR current 使用队尾 placement；
- block、exit 和 no-switch abort 不在 class 内重新入队，也不重置 RR budget。

### 已接受的 noirq 分配限制

fixed bucket 可以 lazy materialize `VecDeque`，其首次 `push_back()` 或扩容仍可能在 owner-CPU noirq transaction 中分配。legacy `RoundRobin` 已有同一风险；本 checkpoint 不把它误写为 RT 新引入的能力，也不宣称 allocation-free。实现必须在 queue 字段或 materialization helper 旁说明该限制并链接删除条件；现状由 [ANE-20260622-IRQ-OFF-HEAP-ALLOCATION](../../register/open-issues.md#ane-20260622-irq-off-heap-allocation) 跟踪。若出现简单、同 owner 且无需新 shared contract 的 allocation-free 修复，可先上报后纳入；否则不扩大本 checkpoint。

### 验证 Floor

- `RtPriority` 边界、排序和 bucket mapping focused KUnit；
- highest-bucket FIFO pick、mixed FIFO/RR ordering、strict higher-priority preempt、arrival head requeue 与 Tick tail requeue focused KUnit 或等价可审查 source proof；
- `Realtime > Idle` 的集中 precedence 与 Idle 只在 RT queue 为空时选择的 focused KUnit 或等价 source proof；
- RR budget decrement/refill、FIFO no-budget 和 peer/no-peer decision focused KUnit；
- RT/RR 与 RT/FIFO 两个 selector 都通过 repository-owned kernel build；
- source audit 确认 pending snapshot 不写回 `RtEntity`，queue node 不复制 priority，production tree 无 `SchedClassKind::RoundRobin`、`new_normal()`、legacy queue owner、published-task policy/priority/class setter 或 direct entity mutation bridge；
- `git diff --check`、`mdbook build docs`、`just build`；KUnit runtime 通过 repository QEMU entrypoint 执行并在 transaction 记录命令与结果。

### 独立 Review Gate

实现 worker 完成后，必须由未参与写入的 reviewer 按 Anemone review 等级检查完整 checkpoint diff。pass 要求没有未关闭的 Apollyon / Keter / Euclid，且 reviewer 明确确认：单一 RT state truth、identity/dispatch/constructor 原子切换、priority-first placement、Tick + RunnableArrival precedence、`Realtime > Idle` 与 idle fallback、full quantum 配置来源、noirq allocation 注释边界、无 wait/pending owner 越界、`new_realtime()` 只服务 fresh entity、无 published-task setter / direct mutation / test-only bridge。finding 修复仍受同一 write set 约束；需要新 owner surface 时先停止并上报。

### 停止条件

如果实现需要修改 `Scheduler` trait、pending acknowledgement、wait transaction、跨 CPU ownership、task-local pending state、第二份 effective priority/full quantum、published-task setter，或配置生成链无法提供受约束 selector，停止并回到 RFC/write-set review；不得用 catch-all event、任意字符串、多个互斥 boolean、legacy identity alias 或 hard-coded fallback 通过 gate。

## Checkpoint 2：集成与用户态 Smoke

Checkpoint 1 关闭前不启动。本阶段默认只记录 source/KUnit/build 之外的真实用户态 lifecycle 证据，不再重复修改 class identity、queue owner、constructor 或配置 contract。

### 默认写集

- `docs/src/devlog/transactions/2026-07-12-sched-rt-class.md`，只追加 user-run / unrun 证据和分类；
- RFC canonical 文档只在 runtime evidence 证明 accepted contract、阶段 gate 或接受边界错误时，按 workflow 停止后回写。

默认不批准 smoke app、rootfs、runner 或 kernel code 写集。若现有 workload 不足，需要新增 harness、rootfs 路由或 runner 入口，必须先停止本 checkpoint，在 transaction 中记录最小文件集合、watchdog/退出条件、失败信号和独立 review gate，再由用户或总控批准后继续。

### 验证 Floor

验证目标为 RT/RR 同 priority CPU-bound progress，以及 RT/FIFO no automatic rotation、explicit yield、block/wake。不同 priority runtime ordering、动态 policy syscall、bandwidth control 和 procfs read-back 仍不纳入。

- 分别记录 RT/RR 与 RT/FIFO build provenance、workload、watchdog、退出条件和结果；
- 区分合法 FIFO starvation、harness timeout、kernel panic、错误 tick rotation 与普通环境失败；
- 不以 broad LTP、精确 quantum、吞吐比例或 latency bound 替代上述 lifecycle proof。

### Review Gate 与停止条件

总控在收口前必须独立复核 user-run 证据是否确实覆盖目标 lifecycle，且没有把未运行项写成通过。若结果要求隐式 tick rotation、service-task 特例、较弱 starvation 语义、test-only setter，或显示 class/policy/priority owner、placement、不变量或接受边界错误，立即停止并回到 RFC review；不得把失败降级成 limitation。若只缺少 harness/runner 文件，走上述 write-set review，不修改调度语义。

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

transaction 与导航已在 Scheduler-Core 前置 Gate 建立；后续 checkpoint 结果只追加到事务日志。本 RFC 已完成文档层提升和实施协议收敛。
