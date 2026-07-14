# Sched RT Class 迁移实施计划

**状态：** R0 Completed；R1 Completed
**当前修订：** R1
**最后更新：** 2026-07-14
**父 RFC：** [RFC-20260711-sched-rt-class](./index.md)
**不变量：** [不变量需求](./invariants.md)

本文件按修订增量保留实施历史。下方 R0 计划与完成记录证明第一版 class 落地；R1 另行追加 Scheduler-Core / RT rotation 修正 gate，不重开或改写 R0 transaction。

R0 实现顺序按“前置合同、RT class 原子切换、集成验证”推进。每个 checkpoint 都有独立的 write set 和停止条件；前一个 gate 未通过时，不继续扩大后续写集。

> **R1 supersession:** R0 中把 `PendingResched` 作为 class-visible preempt snapshot、按 `Tick` / `RunnableArrival` 选择 placement、禁止修改 `Scheduler` trait / pending owner，以及只列 `Realtime > Idle` 的条款，只是 R0 已完成实现的历史合同，已由 R1 取代。R0 的阶段、验证和关闭事实继续保留；当前实现目标以本文件末尾“R1 增量实施计划”为准。

## R0 实施计划与完成记录

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
- `anemone-kernel/src/task/sched.rs`
- `anemone-kernel/src/sched/processor.rs`
- `conf/.defconfig`
- 根 `kconfig`（gitignored live build input；只同步/切换本 checkpoint 新增的 selector 与 timeslice，保留其它开发者本地选项，不提交）
- `scripts/xtask/src/config/kconfig.rs`
- 构建生成的 `anemone-kernel/src/kconfig_defs.rs`（只由 repository build flow 生成）

`task/sched.rs` 与 `processor.rs` 的扩展只用于关闭 whole-entity mutable bridge：`Task` 保留 entity lock owner，scheduler-core caller 改用只读 membership observation，只有 scheduler-class owner 能构造 mutation capability。该扩展不修改 wait-core、trap/IPI pending plumbing、task topology、调度属性 syscall 或其它 scheduler class 算法。若实际编译要求继续越过上述文件，worker 必须先提交 expansion request；不得在已批准写集内制造 adapter 绕过。

模块 owner 必须保持以下边界：

- `rt.rs` 单独拥有 `RtPriority`、`RtPolicy`、`RtEntity`、full-quantum 派生、RT payload accessor / fresh-payload factory、class-private custom fresh construction、bucket/placement/tick 算法和 RT-focused KUnit；
- `entity.rs` 负责 `SchedEntity` storage、公开 facade constructors (`new_default`、`new_idle`)、class payload union、class identity 映射和不可由 class owner 外构造的 entity-mutation capability；它只调用 `rt.rs` 的窄 default-payload factory，不实现 RT budget、priority、policy 或配置逻辑；
- `mod.rs` 只保留 module / narrow re-export、共享 `Scheduler` trait、typed pending contract 和集中 class precedence；不得公开 re-export RT runtime representation，也不得承载 RT policy/quantum/entity 算法或 RT-focused test logic；
- `task/sched.rs` 只持有 entity lock 与 capability-gated mutation bridge，并提供 scheduler-core 所需的只读 membership observation；`processor.rs` 不取得 class payload mutation capability。

### 实现内容

- 增加 `RtPriority`，严格校验 `1..=99`，提供无歧义的 bucket index 映射；
- 增加 `RtPolicy::Fifo`、`RtPolicy::RoundRobin { remaining_ticks }` 与 `RtEntity`，使 policy、priority 和 RR budget 只有一个行为真相源；
- 为 fresh policy 提供对称的 `RtPolicy::fifo()` 与 `RtPolicy::round_robin()` 构造入口；
- 增加单一 `Realtime` identity 和 99 个 priority bucket，删除 legacy `RoundRobin` identity、queue owner 与实现；
- 将集中 precedence 固定为 `Realtime > Idle`，把全部 method-first lifecycle dispatch 接到 `Realtime`；
- 实现 enqueue、dequeue、pick、current placement、FIFO/RR tick 和 strict higher-priority arrival decision；
- 增加 `SchedEntity::new_default()`，删除 `new_normal()`，并原子迁移所有非 idle production constructor；custom priority/policy 的 fresh construction 只留在 `rt.rs` 的 class/test-private helper，不形成共享 public facade；
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
- delayed Tick consumption：RR expiry 后在 pending full pick 前再经过 timer tick，requeue 不 panic 且保留新 quantum 的当前 remainder；
- RT/RR 与 RT/FIFO 两个 selector 都通过 repository-owned kernel build；
- source audit 确认 pending snapshot 不写回 `RtEntity`，queue node 不复制 priority，`SchedEntity`/RT payload 不提供 published-state `Clone`，production tree 无 `SchedClassKind::RoundRobin`、`new_normal()`、public `new_realtime()`、RT runtime type re-export、legacy queue owner、published-task policy/priority/class setter 或 class owner 外可构造的 direct entity mutation bridge；
- `git diff --check`、`mdbook build docs`、`just build`；KUnit runtime 通过 repository QEMU entrypoint 执行并在 transaction 记录命令与结果。

### 独立 Review Gate

实现 worker 完成后，必须由未参与写入的 reviewer 按 Anemone review 等级检查完整 checkpoint diff。pass 要求没有未关闭的 Apollyon / Keter / Euclid，且 reviewer 明确确认：RT types/state/quantum/算法只由 `rt.rs` 拥有，`SchedEntity` public facade constructors 只由 `entity.rs` 拥有，custom RT fresh construction 与 RT runtime types 不离开 `rt.rs`，`entity.rs` / `mod.rs` 只暴露窄 wiring，单一 RT state truth、无 published-state `Clone`、identity/dispatch/constructor 原子切换、priority-first placement、Tick + RunnableArrival precedence、`Realtime > Idle` 与 idle fallback、full quantum 配置来源、noirq allocation 注释边界、无 wait/pending owner 越界、无 published-task setter / class-owner 外 whole-entity mutation / test-only bridge。finding 修复仍受同一 write set 约束；需要新 owner surface 时先停止并上报。

### 停止条件

如果实现需要修改 `Scheduler` trait、pending acknowledgement、wait transaction、跨 CPU ownership、task-local pending state、第二份 effective priority/full quantum、published-task setter，或配置生成链无法提供受约束 selector，停止并回到 RFC/write-set review；不得用 catch-all event、任意字符串、多个互斥 boolean、legacy identity alias 或 hard-coded fallback 通过 gate。

## Checkpoint 2：集成与用户态 Smoke

Checkpoint 1 关闭前不启动。本阶段默认只记录 source/KUnit/build 之外的真实用户态 lifecycle 证据，不再重复修改 class identity、queue owner、constructor 或配置 contract。

### 默认写集

- `docs/src/devlog/transactions/2026-07-12-sched-rt-class.md`，只追加 user-run / unrun 证据和分类；
- RFC canonical 文档只在 runtime evidence 证明 accepted contract、阶段 gate 或接受边界错误时，按 workflow 停止后回写。

默认不批准 smoke app、rootfs、runner 或 kernel code 写集。若现有 workload 不足，需要新增 harness、rootfs 路由或 runner 入口，必须先停止本 checkpoint，在 transaction 中记录最小文件集合、watchdog/退出条件、失败信号和独立 review gate，再由用户或总控批准后继续。

### 验证 Floor

验证目标为默认 RT/RR 下真实用户态 workload 的整体调度集成与 lifecycle 稳定性。RT/FIFO no automatic rotation、policy shape 和 shared-class placement 由 Checkpoint 1 的 source audit / focused KUnit 闭合，不再要求 FIFO 用户态专项 smoke。不同 priority runtime ordering、动态 policy syscall、bandwidth control 和 procfs read-back 仍不纳入。

- 保留 RT/RR 与 RT/FIFO selector build provenance，以及 RT/RR 用户运行 workload 和结果；
- RT/RR 整套 LTP 可以作为 broad integration evidence，但不替代 priority、FIFO no-timeslice 和 RR quantum 的 source/KUnit proof，也不表示每个 LTP case 都通过；
- FIFO 用户态专项验证明确记录为未运行，不写成通过，也不作为本 RFC 的关闭 blocker；
- 不以精确 quantum、吞吐比例或 latency bound 作为验收条件。

### Review Gate 与停止条件

总控在收口前必须复核 user-run 证据属于真实 RT/RR 用户态集成，并确保 FIFO 未运行项没有被写成通过。若结果要求隐式 tick rotation、service-task 特例、较弱 starvation 语义、test-only setter，或显示 class/policy/priority owner、placement、不变量或接受边界错误，立即停止并回到 RFC review；不得把失败降级成 limitation。

## 最终验证 Gate

### Agent-run 验证

- RT/RR compile-time default build；
- RT/FIFO compile-time default build；
- priority、bucket、preempt、quantum、constructor selector 的 focused KUnit；
- source audit：无独立 FIFO class、无 legacy RR owner、无第二份 effective policy/priority、无 task-local pending state、无 `requeue_aborted_wait_current()`；
- `just build`、`git diff --check` 和 RFC 文档 whitespace 检查。

### User-run 验证

- RT/RR compile-time default 下完整运行整套 LTP 测例，作为真实 workload 的调度集成与 lifecycle 证据；
- 该证据只确认整套运行完成，不宣称每个 LTP case 都通过。

### 不运行或不纳入本 RFC 的验证

- 不通过 test-only syscall、隐藏 setter 或临时 service-kthread priority 制造不同 priority workload；
- 不以精确 quantum、吞吐比例、latency bound 或 hard realtime guarantee 作为 gate；
- RT/FIFO 用户态 no-timeslice、explicit-yield 与 block/wake 专项 smoke 本轮未运行，不阻塞第一版收口；
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

transaction 与导航已在 Scheduler-Core 前置 Gate 建立。Checkpoint 1 的实现、build/KUnit/source/review gate 和 Checkpoint 2 的用户侧 RT/RR 整套 LTP 集成验证均已关闭；FIFO 用户态专项验证按用户裁定明确为未运行且不阻塞收口。最终证据见事务日志。

## R1 增量实施计划

R1 修复 [KETER-RT-007](./tracking-issues.md#keter)：core resched request 只表示需要一次 full pick，不能继续充当 RT placement decision 的延迟 cause。RR expiry 产生的队尾 rotation 由 RR entity 在 request 产生时提交，并由后续 class lifecycle transaction 消费。

R1 使用独立事务 [2026-07-14-sched-rt-class-r1](../../devlog/transactions/2026-07-14-sched-rt-class-r1.md)。R0 Completed 事务不追加 R1 执行事实。

### D0：R1 文档与事务闭合

**目标：** 在代码改动前闭合 consolidated R1 contract、跨 RFC shared-interface 对齐、write set、验证 floor 与停止条件。

**写集：**

- `docs/src/rfcs/sched-rt-class/{index,invariants,implementation,tracking-issues}.md`；
- Fair / Stride、wait-preempt-arming 与 Closed/deferred EEVDF-lite canonical 文本中的 shared scheduler contract 对齐；
- `docs/src/devlog/transactions/2026-07-14-sched-rt-class-r1.md`、事务索引、当前双周 devlog、`docs/src/rfcs.md` 与 `docs/src/SUMMARY.md`。

**验证与 review：** `git diff --check`、`mdbook build docs`，并由未参与写入的 reviewer 复核：R0 baseline 可验证、R0 implementation/history 未被重写、R1 contract 不再依赖 pending cause、cross-RFC 当前文本无相反合同。

**退出条件：** canonical 文本、transaction 和导航同时闭合后，D0 才能标记完成；D0 不修改 kernel code，也不把 C1/C2 写成已执行。

### C1：Core-Only Pending 与 RT Rotation 原子修正

**目标：** 在一个可编译 gate 中同时删除 class-visible resched cause continuation，并让 RT/RR 自己保存、消费 committed rotation obligation。trait 变化、core caller 和三个 production class 的适配不能拆成中间不可编译 checkpoint。

**代码写集：**

- `anemone-kernel/src/sched/class/mod.rs`
- `anemone-kernel/src/sched/class/runqueue.rs`
- `anemone-kernel/src/sched/class/rt.rs`
- `anemone-kernel/src/sched/class/fair/stride.rs`
- `anemone-kernel/src/sched/class/idle.rs`
- `anemone-kernel/src/sched/processor.rs`
- `anemone-kernel/src/sched/mod.rs`

对应 focused KUnit 与 source comments 可以在上述 owner 文件内修改。`sched/class/eevdf.rs` 仍是未编译归档实现，不进入 production gate；未来重开 EEVDF 前必须按 R1 shared contract 机械迁移。若编译证明还需要 architecture trap caller、task owner 或其它文件，先在 R1 transaction 记录文件、原因、contract 影响和验证扩展，再等待 write-set 批准。

**Core contract：**

- 删除 `ReschedCause::{Tick, RunnableArrival}`；`request_resched()` 不接收 cause。
- `PendingResched` 移到 processor / scheduler-core owner，收窄为 typed single-bit pending-pick snapshot；保留 `empty`、`is_empty`、`union`、destructive take、caller-owned restore 与 successful full-pick acknowledgement。
- `schedule_preempt(pending)` 保留参数，只用于 non-empty entry proof 和 caller 在 `Deferred` 后恢复同一 snapshot；pending 不进入 `ScheduleMode`、`ScheduleDecision`、`local_requeue_preempted_current()`、`RunQueue` 或 class transaction。
- successful full pick 仍确认旧 slot；wait no-switch abort 与 deferred preempt 仍不确认。R1 不改变 take / restore / acknowledgement 的线性化点。

**RT contract：**

- `RtPolicy::RoundRobin` 增加 `rotation_due: bool`，与 `remaining_ticks` 同属 RT entity owner。
- fresh、queued、woken、blocked、exiting state 必须 clear；只有 active current execution segment 可以置真。
- expiry 且当时存在同 priority peer 时，先置真再返回 `RequestResched`；延迟 tick 保持该义务，budget 继续按正常 quantum 消耗和 refill。
- `requeue_preempted_current()` 原子消费 obligation：真则队尾，假则队头；消费后 clear。不得在消费时重新断言原 peer 或 higher candidate 仍存在。
- yield 与 `handoff_woken_current()` 固有队尾 placement，同时 clear / consume；block 与 exit clear，但不 refill remaining budget。
- no-switch abort 与 deferred preempt 不调用 class transaction，因此保留 obligation。多次 expiry 合并为一个 bool。
- 不为 RT 增加 class-local current `Weak<Task>`；`Processor::running_task` 与 current-only method entry 继续提供 identity / lifecycle proof。

**Fair / Idle 适配：** 删除 `requeue_preempted_current()` 的 pending 参数及相关测试输入。Fair 的 tick pass charge、yield、placement floor、current identity 与 heap 算法不变；Idle 只做 trait 机械适配。

### C2：验证、独立 Review 与 R1 收口

**Focused KUnit：**

- core pending 的 empty/request/take/union-restore state；full-pick acknowledgement 与四个 architecture trap-tail 的 deferred restore 保持 owner-local source proof，不通过修改全局 processor / trap state 的 KUnit 模拟；
- RR fresh / queued obligation clear、expiry commit、延迟 tick preserve、重复 expiry coalescing；
- obligation-clear preempt 入队头、obligation-set preempt 入队尾并消费；
- expiry 后 peer 消失仍履行队尾 placement，且消费阶段不依赖 request-time queue condition；
- yield/handoff 消费、block/exit 清除，所有路径保留合法 current remainder；
- FIFO 无 rotation state / tick rotation；Fair 现有 tick charge 与 lifecycle focused KUnit 保持通过；
- `Realtime > Fair > Idle` 与三个 selector 的 class dispatch 保持不变。

**Build / runtime floor：**

- `fair`、`rt_rr`、`rt_fifo` 三个 selector 的 repository-owned kernel build；
- RT/RR selector 下通过 repository QEMU entrypoint 运行 scheduler focused KUnit，并记录完整 KUnit 结果；
- `git diff --check`、适用的 repository format check 与 `mdbook build docs`。

R1 是 owner / protocol 修正，不新增用户可见 policy 或 ABI；不重复要求整套 LTP 才能关闭。若 focused runtime 或现有 scheduler integration 暴露更广行为回归，再由 transaction 分类并决定是否扩大 runtime gate。

**Source audit：**

- production graph 无 `ReschedCause`，`request_resched()` 无 cause 参数；
- `PendingResched` 只由 scheduler core 拥有，不出现在 `Scheduler` trait、`RunQueue`、class implementation、`ScheduleMode` 或 `ScheduleDecision`；
- `rotation_due` 只位于 RT/RR entity，并只由 current lifecycle transaction 改变；
- Fair 算法 diff 只有 trait / caller 机械适配，无 pass、floor、heap、yield 或 nice contract 变化；
- take / restore / acknowledgement 与 wait no-switch / deferred-preempt 边界保持 R0 已验证语义。

**独立 Review Gate：** 未参与写入的 reviewer 必须检查完整 C1/C2 diff。pass 要求没有未关闭的 Apollyon / Keter / Euclid，并明确确认：rotation 单一 owner、所有 lifecycle clear/consume 路径、peer 消失语义、core-only pending、caller-owned deferred restore、full-pick acknowledgement、Fair 算法不变与 archived EEVDF 边界。

review 与验证通过后，`KETER-RT-007` 才能 neutralize，R1 transaction 才能 Completed，RFC 状态才返回 Closed。

**完成记录（2026-07-14）：** C1 由 `39ba07a9` 原子实现；三个 selector build、clean-rootfs RT/RR QEMU `131/131` KUnit、source audit、repository format / whitespace 检查与独立 review 均已关闭。独立 review 的一项 Euclid（为 full-pick clear 增加单次使用 helper）已在提交前 neutralize，最终无剩余 Apollyon / Keter / Euclid。完整执行证据与 pretest workload 边界见 [R1 事务](../../devlog/transactions/2026-07-14-sched-rt-class-r1.md)。

### R1 停止条件

出现以下任一情况时停止 C1/C2，回到 RFC / write-set review：

- 需要为 core 增加 generation、epoch、token 或多 cause event log 才能保持 correctness；
- 需要改变 pending take / restore / acknowledgement 线性化点，或让 scheduler 内部替 caller restore；
- rotation obligation 必须放到 RT entity owner 之外，或需要复制 current identity 才能证明 lifecycle；
- 需要让 current 同时保留在 ready queue、重做 runqueue membership，或改变 wait-core transaction；
- 需要运行期 policy / priority mutation、跨 CPU migration 或新的 ABI；
- Fair 需要 pass / placement / yield / heap 的语义改动才能适配 trait。

不得用保留 `ReschedCause` 的兼容参数、task-local pending mirror、多个互斥 boolean、peer 消失时静默丢弃 rotation 或 class-specific core branch 绕过停止条件。
