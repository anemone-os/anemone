# Sched EEVDF-lite 不变量需求

**状态：** Draft
**最后更新：** 2026-07-10
**父 RFC：** [RFC-20260622-sched-eevdf-lite](./index.md)

本文定义 `EEVDF-lite` 作为默认 normal scheduler 时必须保持的状态所有权、公平性和 scheduler/wait 边界。当前版本以 sched-split 为已接受前提；若后续实现需要改变 `TaskSchedState`、`PrePark/Parked`、token-bound wait sleep、preempt-defer 或 stale-safe wake placement，必须回到 `sched-wait-preempt-arming` RFC review，而不是在 EEVDF-lite 内局部绕过。

## 闭合条件

迁移完成后必须同时满足：

1. 除 idle task 外，ordinary user task、bootstrap task 和 kthread 默认使用 `EEVDF-lite`，不再依赖临时 RR 作为 production normal class。
2. task 的真实执行归属仍由 `Task::cpuid()` 表示，第一版不做跨核迁移。
3. `SchedEntity::on_runq()` 仍只表示 owner CPU runqueue 上的物理排队事实。
4. `SchedEntity` 不再被强行保持为 `Copy POD`；EEVDF state 必须位于 class-specific payload，不能污染 idle/RR 的语义。
5. task creation / clone 必须创建 fresh normal entity；clone 不得复制父 task 的 `SchedEntity` 或 EEVDF runtime state。
6. EEVDF pick 不是 deadline-only；eligible 约束必须参与 pick。
7. 如果没有 eligible task，fallback 必须可观测，并且不能成为稳定 workload 的常态路径。
8. runtime accounting 对每段实际执行时间只记一次，不重复推进 `vruntime`。
9. wake placement 必须 exactly-once 地避免陈旧 `vruntime` 造成无限奖励或长期惩罚。
10. `Task::nice()` / `set_nice()` 是唯一 nice truth；EEVDF entity 不得保存另一份长期 nice state。
11. pending resched request 必须用 `PendingResched` flags 区分 tick 与 runnable arrival，不得继续用 bool 静默压扁抢占来源。
12. scheduler class contract 必须是 method-first class-local atomic transaction surface；不得使用 `SchedEvent` / `on_event` / catch-all event bus 承载路径语义。
13. RR 行为保持适配阶段不得改变现有 wait/wake stale-safe placement 语义。
14. sched-split 的 `ScheduleMode` 和 processor-private `PendingResched` 不能泄漏为 scheduler class 所有的状态；class 层只在 preempted-current transaction 中按值读取 `PendingResched` flags。
15. bootstrap task、`kthreadd` 和普通 kthread 的第一版进展证明只要求 normal EEVDF eventual progress；不得用隐式 RR 例外、特殊优先级或单独 class 补齐该证明。

如果任一条件不成立，当前实现只能视为迁移中间态，不能声明 EEVDF-lite 默认调度器闭合。

## 非目标

本需求不包含：

1. Linux 完整 EEVDF 兼容性证明。
2. CFS、RT、deadline/EDF 或用户可见 scheduler policy 切换。
3. SMP load balancing、task migration、scheduler domains 或 CPU affinity enforcement。
4. cgroup、autogroup、NUMA、utilization clamp 或 latency nice。
5. wait-core stale wake、`PrePark` deferred fairness gap、source-owner nested wait、IRQ-off allocation 或 long non-preemptible kernel path 的独立修复。
6. agent 侧 iozone、LTP、长 profile 或用户侧 baseline 分析。
7. timer worker、OOM worker、`kthreadd` 或 deferred disposal 的 bounded latency / emergency priority 设计。

## 状态所有权

调度状态必须保持单一真相源：

1. task 是否 runnable / waiting / zombie 由 `TaskSchedState` 拥有。
2. task 是否物理在 runqueue 上由 `SchedEntity::on_runq()` 拥有。
3. task 属于哪个 CPU 由 `Task::cpuid()` 拥有。
4. `Task::nice()` 是 nice / weight source 的唯一长期真相源。
5. 普通任务公平调度字段由 `SchedClassPrv::Eevdf(EevdfEntity)` 或等价 class-specific payload 拥有。
6. runqueue 公平时钟由 owner CPU 的 `Eevdf` class 拥有。
7. task 的 effective scheduler class、class-specific payload 和 queue membership 只能由 owner CPU 的 `RunQueue` transaction 修改；远端 task 不得直接拿 `sched_entity` 锁改 class 或 EEVDF payload。
8. scheduler-private `ScheduleMode` 只属于 core entry permission，不得由 scheduler class 保存、缓存或解释为算法状态。
9. processor-private `PendingResched` 只属于 pending preempt request；class 可以按值读取它作为 `requeue_preempted_current()` 的 cause flags，但不得负责 restore 或驱动 processor pending state。执行 destructive `take_pending_resched()` 的 scheduler-core caller 负责在 deferred preempt 时恢复同一组 flags。
10. 用户态 scheduling ABI accounting 不是本 RFC 的行为真相源；未来 `sched_*` syscall 只能通过明确的后续 RFC 接入真实调度策略。

clone inheritance 只能继承 nice 等对应 owner state，不能继承父 task 的 scheduler entity。新 task publication 必须从 fresh `SchedEntity::new_normal()` 或等价构造器进入 `enqueue_new()` placement，让 `vruntime` / `deadline` / `exec_start` 由 owner CPU EEVDF class 初始化。

未来若支持 scheduler policy / class switch，source CPU 只能完成 ABI / permission / target identity 校验并向 target owner CPU 提交 command；queued、current、blocked 和 exiting task 的 class 迁移必须在 owner CPU `RunQueue` transaction 内线性化。本 RFC 不引入这项能力，只固定 future extension 不得绕过 owner boundary。

允许诊断字段记录 anomaly count、last anomaly reason、last class transaction 或 runtime snapshots。`anomaly` 是 EEVDF-lite 本地诊断概念，不是 Linux / EEVDF 标准状态；它用于记录 no-eligible fallback、virtual-time saturation 等不应在稳定 workload 中常态化的异常路径。诊断字段不得反向驱动调度选择，除非它被提升为正式协议状态并进入本文。

## 身份与能力模型

第一版不引入跨 CPU task ownership 能力。

要求：

1. enqueue/dequeue/pick 只能操作当前 CPU 拥有的 runqueue。
2. remote wake 仍只能请求 owner CPU 做本地 stale-safe placement。
3. producer / waker 不能直接修改目标 CPU runqueue 内部 EEVDF 字段。
4. 未来 scheduler policy / class change producer 也不能直接修改目标 task 的 `SchedEntity`；它必须请求 owner CPU 执行本地 `RunQueue` command。
5. `Arc<Task>` 仍是队列元素身份；若后续引入 tree index，必须使用稳定 tie-breaker，例如 tid 或 task identity，处理重复 `deadline` / `vruntime`。
6. scheduler class transaction 是本次调度路径的 class-visible 线性化动作，不是可跨事件缓存的 capability。
7. scheduler class transaction 不得携带 `WakeToken`、`WaitState` pointer、`PrePark/Parked`、`WakeEnqueueResult` 或 source-local trigger。

## Entry、Pending Resched 与 Class Transaction 边界

必须显式区分三层语义：

1. scheduler core entry permission：`ScheduleMode::{WaitSleep, Preempt, Yield, Idle, Zombie}` 或等价私有 mode。
2. processor / scheduler core pending request：`PendingResched` flags，至少包含 `ReschedCause::{Tick, RunnableArrival}`。
3. scheduler class transaction：`enqueue_new()`、`enqueue_woken()`、`requeue_yielded_current()`、`requeue_preempted_current()`、`handoff_woken_current()`、`requeue_aborted_wait_current()`、`put_prev_blocked()`、`put_prev_exiting()`、`pick_next_task()`、`set_next_task()`、`task_tick()` 和 `decide_preempt_current()` 等 method-first surface。

硬性要求：

1. `schedule_wait_sleep()` 是 token-bound explicit wait sleep；EEVDF class 不得取得 token，也不得自行判断 wait identity。
2. `schedule_preempt(pending)` 对 `Waiting/PrePark` 的 `Deferred` 不代表一次 switch-out，不得触发 EEVDF switch-out accounting。
3. yield entry、idle entry 和 zombie entry 只能通过对应 class transaction 或 core no-op 间接影响 EEVDF，不得把 `ScheduleMode` 作为 class API。
4. `need_resched` 不能在 EEVDF 路径继续作为 untyped bool；tick 与 runnable-arrival request 必须合并到 `PendingResched`，而不是覆盖。
5. `DeferredPreempt` 必须由执行 `take_pending_resched()` 的 caller 恢复同一组 `PendingResched` flags，不能把 deferred request 重新压成 generic bool。
6. `ReschedCause::RunnableArrival` 不代表 wake placement，不得触发 current 的 wake clamp；它只说明已有 runnable candidate 使 current 需要在 preempt tail 重新调度。
7. `Yield`、tick preempt、runnable-arrival preempt、abort-park requeue 和 parked wake handoff 不能被合并为同一个 generic requeue transaction。
8. `Scheduler` trait 不提供通用 `enqueue_runnable()` 默认底座；会改变 membership 的 transaction 必须由每个 class 显式实现。

## Class-Local Atomic Transaction

`Scheduler` 虚表方法是 class-local atomic transaction：

1. 一个方法可以包含多个 class-private 步骤，例如 accounting、placement、yield penalty、wake clamp、统计更新和内部队列操作。
2. scheduler core / `RunQueue` 不能拆开组合这些 class-owned 步骤，只能选择调用哪一个 transaction。
3. scheduler core / `RunQueue` 负责 transaction 之间的全局线性化顺序、owner CPU/noirq 事务、class dispatch、`on_runq` 和 `ntasks`。
4. `enqueue_new()` / `enqueue_woken()` 只做 placement，不做 preempt decision，也不接收 wall-clock `now`；new / wake placement 若需要当前时间，说明 accepted placement contract 不足，必须回到 RFC review。
5. `decide_preempt_current()` 只做 current-vs-candidate decision，不做 enqueue。它可以把 current accounting 推进到 `now`，但不得改变 candidate 的 queue membership。
6. remote runnable arrival 的 preempt decision 必须在目标 task 的 owner CPU placement transaction 内执行；source CPU 不能读取或比较目标 CPU current。
7. `task_tick()` 可以更新 class-local state，但只能返回 `TickAction`，不能直接调用 scheduler core。
8. `pick_next_task()` 与 `set_next_task()` 必须分离；前者选择并移出队列，后者记录 next 开始运行。
9. 简单 class 若忽略路径差异，应在自己的 impl 内用私有 helper 复用逻辑，而不是依赖 trait 默认 generic enqueue。

## 线性化点

必须显式定义以下线性化点：

1. runtime accounting：EEVDF private `account_current(now)` 何时推进当前 task 的 `vruntime`，并刷新 `exec_start`。
2. switch-in：`pick_next_task()` 选择并移出 class queue / 清 `on_runq` 后，scheduler core 必须先调用 class `set_next_task(task, now)`，再执行地址空间切换准备，例如当前 `switch_mapping(prev, next)`，然后进入现有 `Task::on_switch_in()`、`set_current_task()` 和 architecture switch。
3. runnable requeue：当前 task 在 class requeue transaction 内已经通过 `account_current(now)` 更新 `vruntime` / `deadline`。
4. enqueue placement：task 从非 runqueue 状态进入 EEVDF queue 时，`vruntime` clamp、`deadline` 初始化和 `on_runq = true` 的顺序。
5. wake placement：wait-core 已经把 task 逻辑状态转为 runnable 后，物理 placement 如何进入 owner CPU runqueue。
6. parked handoff：`ParkPending` 由 scheduler 收口为 physical requeue 时，如何 exactly-once 地执行 wake clamp。
7. abort-park requeue：wait park 后 scheduler 复查发现 current 已 runnable 时，如何不带 wake reward 地重新入队。
8. fallback anomaly：没有 eligible task 被确认、选择 fallback task、记录 anomaly 的顺序。

硬性要求：

1. `on_runq` 与 queue membership 不得长期不一致；同一个 lock / interrupt-disabled 事务内可以有受控过渡。
2. `vruntime` 更新必须发生在当前 task 被重新 enqueue、park switch 或 exit switch 之前。
3. `deadline` 更新必须基于已经推进后的 `vruntime`。
4. `rq_vtime` 更新不能依赖会跨 CPU 读取的 task 状态。
5. pick 不能选择非 runnable、waiting、zombie 或非当前 owner CPU 的 task。
6. `DeferredPreempt` 不得结束当前执行段；它只返回 deferred result，让执行 destructive take 的 caller 恢复 / 保留 resched 请求。
7. no-switch abort 不调用 scheduler class transaction。
8. 未真正切换到 next task 的路径不得调用 `set_next_task()`；bootstrap first task、idle fallback、block、yield 和 zombie 切换路径都必须能按同一顺序 source-audit。
9. `exec_start` 从 `set_next_task()` 开始计入即将运行的 execution segment；如果实现期认为 mapping 准备时间必须排除在公平执行段外，必须回到 RFC review，而不是局部移动 `set_next_task()`。

## Runtime Accounting

EEVDF-lite 必须使用唯一幂等 helper 推进当前执行段：

1. EEVDF private `account_current(now)` 是推进 `vruntime`、更新 `deadline`、刷新 `exec_start` 和更新 `rq_vtime` 的唯一入口。
2. `account_current(now)` 不是 shared trait 方法；trait 只暴露 current execution accounting 的生命周期点。
3. tick 和 switch-out / requeue 可以通过对应 class transaction 调用同一个 helper，但不得各自维护独立 `delta_exec` 计算。
4. 每次 `account_current(now)` 成功推进后必须刷新 `exec_start = now`，避免后续 switch-out 双记。
5. 如果 `now <= exec_start`，入口必须 no-op 或 fail closed，不能让 `vruntime` 倒退。
6. `Task::on_switch_out()` 仍是 task / CPU usage hook，不是 EEVDF fairness accounting truth。
7. 第一版直接使用 `Instant`；`Instant::now()` 必须通过阶段审计证明适合 scheduler noirq / tick path，不预设新的 scheduler time abstraction。
8. `Vruntime`、`Deadline` 和 `rq_vtime` 的长期存储使用 normalized nanoseconds 的 `u64` scalar；nice 0 下 `1ns` actual runtime 对应 `1` virtual ns。
9. virtual-time 乘除使用 `u128` 中间值，并通过 EEVDF private helper saturate 回 `u64`；overflow / saturation 必须记录 anomaly，不得 panic，也不得把 `Result` 扩散到 `Scheduler` trait 或 `RunQueue` surface。
10. 正 `delta_exec` 计算出的 `delta_vruntime` 若为 `0`，必须至少推进 `1`，保证持续运行最终推进公平账本。

## Wake Placement

wake clamp 必须 exactly once：

1. `WakeEnqueueResult::Enqueued` 是普通 wake placement 的 clamp 点；scheduler core / `RunQueue` 在实际入队后调用 `enqueue_woken()`。
2. `handoff_woken_current()` 是 `ParkPending` 由 scheduler 收口入队的 clamp 点。
3. `WakeEnqueueResult::Stale` 不得修改 EEVDF entity。
4. `WakeEnqueueResult::AlreadyCurrent` 不得修改 EEVDF entity。
5. `WakeEnqueueResult::ParkPending` 不得立即 clamp；它只允许后续 scheduler handoff 分支 exactly-once clamp。
6. `WakeEnqueueResult::AlreadyQueued` 不得二次 clamp。
7. no-switch abort 不调用 class；`requeue_aborted_wait_current()` 不得走 wake clamp，也不得套 yield penalty。

禁止用 source-local flag、diagnostic wait id、`WakeToken::is_armed()`、`PollRegisterResult::Armed` 或 `WakeEnqueueResult` 驱动 EEVDF placement。

## 公平性与 Eligibility

EEVDF-lite 必须保持以下公平性边界：

1. 权重越高，单位实际执行时间推进的 `vruntime` 越慢。
2. `deadline = vruntime + slice_ns * NICE_0_WEIGHT / weight` 或等价形式；deadline 只在初始化或 `vruntime >= deadline` 时自然续期，普通 requeue 不得无条件重算 deadline。
3. `rq_vtime` 第一版使用 monotonic min-vruntime floor：visible runnable set 包含 ready queue 中的 runnable tasks 和当前正在运行的 EEVDF task，`rq_vtime = max(rq_vtime, min_visible_vruntime)`；visible set 为空时保持不变。
4. current task 被 pick 出 queue 后仍参与 `rq_vtime` 更新，但不参与 queue membership 或 pick scan。
5. eligible 判断使用 `task.vruntime <= rq_vtime`。
6. `rq_vtime` 不能回退，也不能是不说明更新点的临时统计。
7. 短 slice 可以带来更早 virtual deadline 和更好响应性，但不能绕过 eligibility 长期获得超过公平份额的 CPU。
8. fresh new task placement 使用 `vruntime = rq_vtime`，并按当前 nice weight 与 base slice 计算 deadline；不得读取 wall-clock `now`。
9. 如果没有 eligible task，fallback 只能作为 forward-progress 保护：选择最小 `vruntime` task，记录 anomaly，并把 `rq_vtime` 推进到 fallback task 的 `vruntime`。稳定 CPU-bound workload 在 warm-up 后不得持续增长 fallback anomaly。
10. wake clamp 窗口必须围绕 `rq_vtime`，不能直接把所有 wake task 重置到最有利位置。
11. `sched_yield()` 必须使用 bounded penalty：只把 yielding task 的 deadline 后推到至少 `rq_vtime + yield_penalty_window_vruntime(weight)`，不得修改 `vruntime`、nice 或 weight，也不能把 yielding task 永久惩罚出公平队列。
12. `Task::nice()` 是唯一 weight truth；renice 后下一次 owner CPU accounting / enqueue / pick / preempt decision 必须读取最新 nice，但已存在 deadline 不要求立即重算，也不得为此引入远端 EEVDF payload 修改或 class migration。

禁止退化为：

- 只按 `deadline` 排序。
- 只按 `vruntime` 排序且完全忽略 `deadline`。
- 每 tick 无条件轮转。
- wake 时无条件把任务放到队首或最小 `vruntime`。
- yield 时无条件立即选回自己，或无界推后到长期饥饿。

## Bootstrap / Kthread Progress 边界

bootstrap task、`kthreadd` 和普通 kthread 第一版直接使用 normal EEVDF。本文证明目标是 owner CPU 上的 eventual scheduler progress，不是服务线程的 bounded latency：

1. 除 idle task 外，默认 task publication 只能创建 fresh normal entity；保留的 RR entity 必须是 debug / bisect 对照，不能参与 production placement。
2. owner CPU 的 normal runnable 集合是有限集合；每个 normal runnable task 的 `Task::nice()` 对应正且有限的 weight。
3. 如果一个 normal task 持续 runnable，`account_current(now)`、eligible pick、no-eligible fallback 和 bounded yield penalty 必须共同保证它不会被永久排除在 pick 之外。
4. wake clamp 和 new-task placement 只能把 entity 放到 `rq_vtime` 附近的 bounded window，不能制造永久领先或永久落后。
5. timer worker、OOM worker、`kthreadd` 等 service kthread 的等待、唤醒、停止和每轮主动 `yield_now()` 属于各自 owner 的 lifecycle 纪律；EEVDF 不保存 service-type identity，也不按 kthread 名称改变调度决策。
6. wait-core progress 不由一个隐藏 scheduler-critical kthread 表达；deferred disposal、IRQ-off allocation、long non-preemptible path 等风险按其 owner / register 路由，不作为 EEVDF forward-progress 证明的兜底条件。
7. 若后续发现某类 service kthread 需要 bounded latency、emergency priority 或单独 class，必须新增明确 scheduler policy / kthread priority 设计，不能在 EEVDF-lite default switch 中保留隐式 RR 例外。

## 锁序与生命周期规则

第一版必须保持当前 per-CPU scheduler 访问模型：

1. runqueue 修改仍在本地 interrupt-disabled 上下文中完成。
2. `SchedEntity` 字段更新不得在没有 owner CPU 证明的远端路径执行。
3. 不为第一版 EEVDF 引入额外 `RunQueue` transaction lock；本地 interrupt-disabled `RunQueue` transaction 是 scheduler class、queue membership、`on_runq` 和 future policy-change command 的线性化边界。
4. `Task::nice()` 是 task-owned weight truth 的例外，可由 `setpriority()` 远端更新；EEVDF 只能在 owner CPU 的 accounting / enqueue / pick 中观察该值，不得把 nice 更新伪装成 class payload migration。
5. scheduler class 不得在 `task_tick()` 中访问当前 processor percpu 变量并制造重入。
6. task exit / zombie 路径不得把已退出 task 留在 EEVDF queue。
7. dequeue 失败仍应暴露 bug，不能为了兼容 tree/index 更新失败静默忽略。
8. strategy constants 中的 base slice、wake clamp、yield penalty 和 anomaly threshold 必须来自 Kconfig。
9. idle task 保持 fallback singleton；不通过 `requeue_*_current()` 物理入队。

阶段 1 可以主动做同一 scheduler owner 内的结构拆分，例如把 `RunQueue` facade 移到 `sched/class/runqueue.rs`，把 `SchedEntity` / `SchedClassPrv` 移到 `sched/class/entity.rs`，同时让 `Scheduler` trait 留在 `sched/class/mod.rs`。该拆分属于结构维护：行为保持、不扩大 public API、不改变 wait-core API、不改变 task topology。阶段 1 拆出 `entity.rs` 时，`SchedEntity` 的 RR/Idle 形状和 `Copy` 行为保持不变；阶段 2 再引入 EEVDF payload。

将 `SchedEntity` 拆为 class-specific payload、增加 `sched/class/eevdf.rs`、或引入 EEVDF-specific state，若保持同一 scheduler owner、行为保持、public API 不扩大，属于结构维护。若拆分改变 wait-core API、task topology、public scheduler policy 或 shared contract，必须回到 RFC review。

## 禁止退化项

以下做法不能作为最终关闭条件：

1. 用 benchmark 特化逻辑处理 `iozone` worker。
2. 为了让 `iozone` 数字好看关闭 eligibility。
3. 隐藏或禁用 fallback anomaly 记录。
4. 把 RR 保留为 production normal class，只把 EEVDF 作为未使用实验类。
5. 在 `sched_setaffinity()`、`sched_setscheduler()` 或 future syscall 中绕过本 RFC 的 owner CPU 不变量。
6. 为避免 starvation 放宽 wait-core stale-safe wake assertions。
7. 把 `rq_vtime` 写成不可审查的临时统计字段。
8. 用单个 `BTreeMap<deadline, task>` 替代算法语义。
9. 把 nice 值复制到多个长期字段并允许冲突。
10. 把 tree/index 优化和算法迁移混在同一不可回滚阶段。
11. 重新引入 scheduler owner 外的裸 `schedule()` 或公开 `ScheduleCaller` taxonomy。
12. 把 sched-split residual fairness gap 包装成 EEVDF 应兜底的目标。
13. 使用 `SchedEvent` / `on_event` 或类似 catch-all event bus 承载 scheduler class path semantics。
14. 通过 generic `enqueue_runnable()` 默认实现绕过 `enqueue_new()`、`enqueue_woken()`、`requeue_*_current()` 等 transaction。

## 完成标准

文档层完成标准：

1. `rq_vtime` 最低约束、eligibility 规则、wake clamp 窗口、yield penalty、slice / weight 公式和 anomaly 语义已经写入 canonical 文档或 probe gate。
2. method-first scheduler class transaction surface 覆盖所有现有 schedule / requeue / wake path，并与 scheduler-private `ScheduleMode` 分层。
3. accepted contract 中不存在 `SchedEvent` / `on_event` / catch-all event bus。
4. 实施计划能先行为保持适配 RR，再引入 EEVDF，再切默认 class。
5. bootstrap / kthread direct EEVDF 的 eventual-progress 证明已经写入本文，且不依赖 focused smoke 作为 contract 决策。
6. `tracking-issues.md` 中影响实现顺序和验收边界的问题已有处理结论。

实现层完成标准：

1. RR 适配阶段行为保持。
2. 除 idle task 外，ordinary task、bootstrap task 和 kthread 默认进入 EEVDF class，且无 production RR 特例。
3. 多 runnable task 不稳定饿死，CPU 时间份额随 nice 权重方向变化。
4. wake/sleep workload 不因陈旧 lag、重复 wake reward、missing parked handoff clamp 或 stale wake clamp 饿死/刷分。
5. bounded yield penalty 有 focused smoke，证明其它 runnable task 能运行且 yielding task 不永久饿死。
6. fallback anomaly 有统计或日志观察面，并在稳定 workload 下不持续增长。
7. 用户侧 runtime log 若显示 wait-preempt residual，按 owner 路由回对应 RFC，不降低本 RFC 的 eligibility / accounting / placement 不变量。
