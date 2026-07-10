# RFC-20260622-sched-eevdf-lite

**状态：** Accepted for Implementation
**负责人：** doruche, Codex
**最后更新：** 2026-07-10
**领域：** scheduler / fairness / runtime accounting / scheduler class
**事务日志：** [2026-07-09-sched-eevdf-lite](../../devlog/transactions/2026-07-09-sched-eevdf-lite.md)
**开放问题：** 见 [Tracking Issues](./tracking-issues.md)
**下一步：** 按事务日志推进 Checkpoint 2D / Gate P3：实现 ordinary wake / parked handoff exactly-once wake clamp，并证明 stale、already-current、already-queued 和 abort path 不会获得重复或错误 reward；不得提前切换 default normal class。

## 摘要

本 RFC 草案定义 Anemone 的默认普通任务调度器从临时 `RoundRobin` 迁移到 `EEVDF-lite` 的方向。第一版目标不是完整复刻 Linux EEVDF，而是在当前 fixed owner CPU、per-CPU runqueue、无迁移、无 load balance 的边界内，引入带权重的 virtual runtime、virtual deadline、eligibility / lag 约束、wake placement、bounded yield penalty 和幂等 runtime accounting。

本版草案以 `sched-wait-preempt-arming` / sched-split 为已接受的下层前提。scheduler core 已经通过 scheduler-private `ScheduleMode` 和语义化 wrapper 区分 `schedule_preempt()`、token-bound `schedule_wait_sleep()`、yield entry、idle entry 和 `schedule_zombie_never_return()`；EEVDF-lite 不重新设计 wait-core `PrePark/Parked`、token-bound wait sleep、stale-safe wake placement 或 preempt-defer contract。EEVDF-lite 只在这些入口之后补齐 method-first scheduler class lifecycle transaction、runtime accounting、placement 和 default normal class 语义。

早期动机来自单核 `iozone -t 4` 中 per-child `Min xfer = 0` 或极低的公平性异常。sched-split 已经改变了 wait/preempt 边界，因此该现象现在只保留为历史动机和用户侧反馈来源；新的 runtime log 若显示 wait-preempt residual，例如长 `PrePark` deferred 窗口或 source-owner nested wait，应路由回 `sched-wait-preempt-arming`，不要求 EEVDF-lite 兜底。

## 背景

当前 scheduler class 只有 `RoundRobin` 和 `Idle`。`RoundRobin` 使用 `VecDeque<Arc<Task>>`，`pick_next()` 从队头取任务，`on_tick()` 每个 tick 都请求 resched；`Scheduler` trait 仍只有 `enqueue()`、`dequeue()`、`pick_next()` 和 `on_tick()`，无法表达当前 task 离开 CPU、重新入队、阻塞、退出、tick 结算、wake placement、new task placement 和 switch-in 的生命周期边界。

纠偏后的方向不是给 class 传一个 catch-all `SchedEvent`，而是把 scheduler class 虚表扩展为 method-first 的 class-local atomic transaction surface。路径语义由方法名和 `RunQueue` facade 的调用点表达；只有同一 transaction 内确实需要算法复用时，才允许窄参数或返回类型，例如 `PendingResched`、`TickAction` 和 `PreemptDecision`。

调度核心已有几个必须保留的边界：

- task 创建后拥有固定 `Task::cpuid()`，该 CPU 是 task 的 owner CPU。
- 每个 CPU 拥有本地 `Processor` 和 `RunQueue`。
- `SchedEntity::on_runq()` 是 owner CPU runqueue 上的物理排队事实，不能被跨 CPU 随意读取或修改。
- `TaskSchedState` 拥有 runnable / waiting / zombie 逻辑状态；scheduler class 不能绕过 wait-core 直接重写这些状态。
- stale-safe wake placement 仍由 wait core 完成逻辑 wake 后调用 `wake_enqueue()`，并以 `WakeEnqueueResult` 表示物理 placement 结果。
- sched-split 后，scheduler owner 外不得重新引入裸 `schedule()` 或公开 `ScheduleCaller` taxonomy。

EEVDF-lite 要解决的是 normal runnable task 到达调度点后的公平选择和响应性，而不是 wait-core、IRQ-off、long non-preemptible kernel path 或 source-owner sleepability 的独立缺陷。用户侧保存和分析的 baseline / 实作后 runtime log 可作为 implementation feedback；agent 不把 iozone、长日志、LTP profile 或 deferred-count trace 作为本 RFC 的必跑验证。

## 目标

- 将 `EEVDF-lite` 作为默认 normal scheduler，替换临时 RR；除 idle task 外，ordinary user task、bootstrap task 和 kthread 第一版都进入 EEVDF normal class。
- bootstrap task、`kthreadd` 和普通 kthread 第一版直接使用 normal EEVDF；本 RFC 只承诺它们在有限 runnable 集合中的 eventual scheduler progress，不承诺 bounded latency 或特殊服务优先级。
- 保持 fixed owner CPU、per-CPU runqueue、无迁移、无 load balance 的调度边界。
- 扩展 `Scheduler` trait 为 method-first class-local atomic transaction surface，使调度类能在明确生命周期点维护 class-local state。
- 明确 `ScheduleMode`、`PendingResched` 与 scheduler class transaction 的分层：entry permission、pending preempt source 和 class-visible accounting / placement transaction 不能互相泄漏。
- 为普通任务维护 class-specific EEVDF state：`vruntime`、`deadline`、`slice`、`exec_start`、initialized 标记和 anomaly 诊断字段。
- 为 task creation / clone 定义 fresh normal entity 初始化路径；clone 可以继承 nice，但不得复制父 task 的 `SchedEntity` 或 EEVDF runtime state。
- 使用 `Task::nice()` / `set_nice()` 作为唯一 nice 真相源；EEVDF entity 不长期复制 nice，也不在第一版保存 `cached_weight`。
- 使用固定 Linux nice weight 表；base slice、wake clamp window、yield penalty window 和 anomaly threshold 进入 Kconfig。
- trait 暴露 current execution accounting 的生命周期点；EEVDF 必须通过 class-private `account_current(now)` 幂等结算当前执行段，tick 和 switch-out / requeue 不得重复推进同一段 `delta_exec`。
- 使用 eligibility / `rq_vtime` 约束，避免第一版退化成 deadline-only 调度器；第一版 `rq_vtime` 使用 monotonic min-vruntime floor，visible runnable set 包含 ready queue 和当前正在运行的 EEVDF task。
- 定义 wake placement exactly-once：普通 wake 只有在 stale-safe placement 返回 `Enqueued` 后通过 `enqueue_woken()` 执行 wake clamp；parked current 的 wake handoff 通过 `handoff_woken_current()` 执行 wake clamp；no-switch abort 和 `requeue_aborted_wait_current()` 不执行 wake clamp。
- 为 `sched_yield()` 定义 bounded yield penalty，避免 yield task 立即无界选回，也避免永久饿死。
- 第一版使用线性 `Eevdf` class 容器和 O(n) pick/dequeue；树索引作为后续优化 gate。
- 为 eligibility fallback 和 virtual-time saturation 记录 anomaly，保证异常路径可观测；anomaly 是 EEVDF-lite 本地诊断概念，不是 Linux / EEVDF 标准状态，也不得反向驱动调度决策。
- 建立 agent / user 验证责任分层：agent 负责 build、source audit 和 focused smoke；用户侧 runtime log / iozone / LTP 作为反馈材料。

## 非目标

- 不实现 Linux 完整 EEVDF 细节，例如完整 vlag、delayed dequeue、lag decay、latency nice、cgroup scheduling、utilization clamp 或 bandwidth controller。
- 不实现 CFS、realtime、deadline/EDF 调度类，也不实现用户动态切换 scheduler policy 的 `sched_*` syscall 语义。
- 不实现 runtime scheduler policy / class switch；未来若支持 `sched_setscheduler()` / `sched_setattr()` 这类真实策略切换，必须通过 target owner CPU 的 `RunQueue` command / IPI 线性化，不能由远端 task 直接改 `SchedEntity` class 或 class-specific payload。
- 不引入跨核迁移、SMP load balancing、scheduler domain、CPU hotplug 或 remote runqueue observation。
- 不把 first version 的 runqueue 容器优化为 RB-tree、heap、双 `BTreeMap` 索引或其它复杂结构。
- 不重新设计 sched-split 的 wait-core / scheduler entry contract。
- 不承诺修复 wait-core stale wake、`PrePark` setup 长 deferred 窗口、source-owner nested wait、IRQ-off allocation、timer/deferred disposal 或长期关中断/不可抢占内核路径。
- 不为 timer worker、OOM worker、`kthreadd` 或 deferred disposal 引入隐式 RR 例外、特殊优先级或单独 scheduler class；若后续证明需要 bounded latency / emergency priority，必须进入后续 kthread priority / service class / RT-like 设计。
- 不以 `iozone` 达到某个吞吐数字作为 RFC 接受条件，也不承诺单核运行达到 4 核运行时的 throughput。
- 不承诺 `getpriority()` / `setpriority()` 的完整 Linux ABI、权限、返回值或动态 renice 即时性；第一版只消费内部 nice-like weight source。
- 不接受 `SchedEvent`、`on_event` 或类似 catch-all event bus 作为 scheduler class contract。

## 文档地图

计划主文档：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)
- [Tracking Issues](./tracking-issues.md)

背景材料：

- None

## 方案

第一版引入 `Eevdf` 作为默认 normal scheduler class。`RoundRobin` 可以保留为 trait plumbing 的行为保持对照、debug 或 bisect class，但 default switch 完成后不得仍是 production placement path；所有 `SchedClassPrv::RoundRobin` 保留点必须被分类。`Idle` 仍维持 fallback singleton 语义。

### Entry、Pending Request 与 Class Transaction Surface

sched-split 的 `ScheduleMode` 仍保持 scheduler-private，但需要用路径语义命名：

- `WaitSleep`：token-bound explicit wait sleep，有权消费 `PrePark`。
- `Preempt`：trap / preempt tail 的 involuntary preempt，只能抢占 runnable current；遇到 `Waiting/PrePark` 返回 deferred。
- `Yield`：`sched_yield()` / `yield_now()` 入口，current 必须 runnable，调度类可执行 bounded yield penalty。
- `Idle`：idle loop 专用入口，idle task 保持 fallback singleton，不进入 normal requeue。
- `Zombie`：exit no-return 入口。

preempt request 保持 scheduler core / processor-private。当前 `need_resched` bool 必须升级为 `PendingResched` flags，至少保存 tick 与 runnable arrival 两个 bit：

```rust
enum ReschedCause {
    Tick,
    RunnableArrival,
}

struct PendingResched {
    // flags value, not a capability token
}
```

`request_resched(cause)` 合并 bit，而不是 last-writer-wins；`Tick` 不能被后续 runnable arrival 擦掉。trap tail 通过 `take_pending_resched()` 或等价 API 取得 `PendingResched` 后进入 `schedule_preempt(pending)`；idle loop 只把非空 pending 作为离开 idle 并调用 `schedule_idle()` 的触发。若 preempt 因 `Waiting/PrePark` deferred，执行 destructive take 并进入 `schedule_preempt(pending)` 的 caller 必须恢复同一组 pending bits，避免抢占请求被吞掉或被重新压成 generic bool。

`PendingResched` 是普通值语义 flags，不是 processor state capability。scheduler class 可以在 `requeue_preempted_current(task, now, pending)` 中读取它来区分 tick、runnable arrival 或二者同时存在，但 restore pending request 只属于执行 `take_pending_resched()` 的 scheduler-core caller。

`Scheduler` trait 不提供 catch-all event 方法，也不提供通用 `enqueue_runnable()` 默认底座。会改变 runqueue membership 的 transaction 必须由每个 class 显式实现；简单 class 若不关心路径差异，可以在自身 impl 内用私有 helper 复用逻辑。

第一版 class-visible transaction surface 至少包含：

```rust
trait Scheduler {
    fn enqueue_new(&mut self, task: Arc<Task>);
    fn enqueue_woken(&mut self, task: Arc<Task>);
    fn dequeue(&mut self, task: &Arc<Task>) -> bool;

    fn requeue_yielded_current(&mut self, task: Arc<Task>, now: Instant);
    fn requeue_preempted_current(
        &mut self,
        task: Arc<Task>,
        now: Instant,
        pending: PendingResched,
    );
    fn handoff_woken_current(&mut self, task: Arc<Task>, now: Instant);
    fn requeue_aborted_wait_current(&mut self, task: Arc<Task>, now: Instant);

    fn put_prev_blocked(&mut self, task: &Arc<Task>, now: Instant);
    fn put_prev_exiting(&mut self, task: &Arc<Task>, now: Instant);

    fn pick_next_task(&mut self) -> Option<Arc<Task>>;
    fn set_next_task(&mut self, task: &Arc<Task>, now: Instant);
    fn task_tick(&mut self, task: &Arc<Task>, now: Instant) -> TickAction;
    fn decide_preempt_current(
        &mut self,
        current: &Arc<Task>,
        candidate: &Arc<Task>,
        now: Instant,
    ) -> PreemptDecision;
}
```

具体命名可按实现风格调整，但必须保持 method-first 原则：

- 虚表方法是 class-local atomic transaction。一个方法可以包含 accounting、placement、penalty、clamp 和统计更新等多个内部步骤，但 scheduler core 不能拆开组合 class-owned 状态转换。
- scheduler core / `RunQueue` 负责选择调用哪个 transaction，并负责 transaction 之间的全局线性化顺序。
- `enqueue_new()` / `enqueue_woken()` 只做 placement，不做 preempt decision，也不接收 wall-clock `now`。第一版 new / wake placement 必须由 class state、`rq_vtime` 和 bounded clamp window 表达；若实现期发现必须依赖当前时间，必须停止并回到 RFC review，而不是自行扩展接口参数。
- `decide_preempt_current()` 在 owner CPU 上的 placement 完成后调用，只返回 `PreemptDecision`，不做 enqueue。它可以在 class 内部把 current accounting 推进到 `now`，但不得改变 candidate 的 queue membership。remote new-task / wake arrival 不能在 source CPU 读取目标 CPU current；若 placement 通过 IPI 发生，preempt decision 也必须在 owner CPU 的 placement transaction 内线性化。阶段 1 若为 RR 行为保持暂时保守设置 `ReschedCause::RunnableArrival`，必须显式标为临时路径，并在 EEVDF placement 接入前收口。
- `task_tick()` 是可变生命周期 transaction，只返回 `TickAction`，不直接调用 scheduler core。
- `pick_next_task()` 和 `set_next_task()` 分离；前者选择并移出 class queue，后者记录 next 开始运行，组合顺序由 `RunQueue` / scheduler core 负责。
- switch-in 顺序固定为 `pick_next_task()` 选择并移出 class queue / 清 `on_runq`，scheduler core 随后调用 `set_next_task(task, now)`，再执行地址空间切换准备，例如当前 `switch_mapping(prev, next)`，然后进入现有 `Task::on_switch_in()`、`set_current_task()` 和 architecture switch。`exec_start` 从 `set_next_task()` 开始计入即将运行的 execution segment；若实现期认为 mapping 准备时间必须排除在公平执行段外，必须停下回到 RFC review。未真正切换的路径，例如 no-switch abort 和 deferred preempt，不得调用 `set_next_task()`，也不得结束 current 的 execution segment。

`RunQueue` 拥有 class dispatch、`ntasks`、`SchedEntity::on_runq`、owner/noirq 事务和 idle fallback 线性化。scheduler class 拥有 class 内队列、算法字段、private helper、placement 策略、tick decision 和 anomaly 统计。

### 调度实体

`SchedEntity` 保留 shared physical truth：

- `on_runq`：owner CPU runqueue 上的物理排队事实。
- `class`：class-specific payload。

第一版 EEVDF 接入时不再要求 `SchedEntity: Copy`。EEVDF state 放入 class-specific payload，例如：

```rust
struct SchedEntity {
    on_runq: bool,
    class: SchedClassPrv,
}

enum SchedClassPrv {
    Eevdf(EevdfEntity),
    RoundRobin(()),
    Idle(()),
}
```

`EevdfEntity` 至少维护：

- `vruntime`
- `deadline`
- `slice`
- `exec_start`
- initialized / valid 标记
- anomaly count / last anomaly reason 诊断字段或等价统计入口

普通 task、bootstrap task、kthread 和 clone child 都必须通过 `SchedEntity::new_normal()` 或等价 fresh constructor 创建 class payload。clone 只能继承父 task 的 nice / credentials / address-space 等对应 owner state，不能通过 `current_task.sched_entity()` 复制父 task 的 `vruntime`、`deadline`、`exec_start`、`on_runq` 或 initialized 状态。未初始化的 fresh normal entity 在第一次 `enqueue_new()` 时按 `rq_vtime` 附近完成 placement。

bootstrap task、`kthreadd`、timer worker 和 OOM worker 等内核线程不获得隐式 RR 保留点。它们的调度证明与 ordinary task 相同：在 owner CPU 上成为 runnable 后，只依赖 normal EEVDF 的正权重、bounded placement、bounded yield penalty 和 no-eligible fallback forward-progress 规则获得 eventual progress。它们自身的等待、唤醒、停止和每轮主动 `yield_now()` 属于 kthread / worker owner 的 lifecycle 纪律，不提升为 EEVDF 特殊优先级。

`Task::nice()` 是 nice 的唯一真相源。EEVDF 可按 `Task::nice()` 即时计算 weight；第一版不保存长期 `cached_weight`，也不把 nice 移入 `SchedEntity`。

### `Eevdf` Class

`Eevdf` 与 `RoundRobin`、`Idle` 命名对齐，内部持有线性 ready queue、`rq_vtime` 和必要统计。第一版使用 `Vec<Arc<Task>>` 或等价线性容器：

1. duplicate enqueue 必须暴露 bug。
2. dequeue missing 必须暴露 bug。
3. pick 从 eligible tasks 中选择最小 deadline。
4. 如果没有 eligible task，fallback 到最小 `vruntime` 并记录 anomaly。
5. 如果 EEVDF queue 为空，RunQueue 选择 idle task。

`rq_vtime` 是 eligibility 的公平时钟基准，不能只是可回退的 `min(vruntime)` 别名。第一版使用 monotonic min-vruntime floor：

```text
visible_min = min(vruntime of queued runnable tasks,
                  current.vruntime if current is running in EEVDF)
rq_vtime = max(rq_vtime, visible_min)
```

visible set 为空时 `rq_vtime` 保持不变。current task 被 `pick_next_task()` 移出物理 queue 后，仍作为正在运行的 runnable entity 参与 `rq_vtime` 更新，但不参与 queue membership 或 pick scan。enqueue / dequeue / pick 和 `account_current(now)` 后都通过同一类 helper 用当前 visible runnable set 推进 `rq_vtime`；runnable set 变小或 new / wake task 放入较小 `vruntime` 时不得让 `rq_vtime` 回退。

eligibility 使用 `task.vruntime <= rq_vtime`。正常 pick 在 eligible tasks 中选择最小 deadline；若 non-empty queue 中没有 eligible task，fallback 到最小 `vruntime`，记录 anomaly，并把 `rq_vtime` 推进到 fallback task 的 `vruntime`。fallback 只作为 forward-progress 保护；稳定 CPU-bound workload 在 warm-up 后持续增长 fallback anomaly 时，视为 `rq_vtime` / placement 公式失败。

### Runtime Accounting

runtime accounting 的生命周期点由 trait transaction 暴露；EEVDF 的具体执行段结算由 class-private helper 完成：

```text
account_current(now):
    delta_exec = now - exec_start
    curr.vruntime += delta_exec_ns * NICE_0_WEIGHT / curr.weight
    curr.exec_start = now
    if curr.vruntime >= curr.deadline:
        curr.deadline = curr.vruntime + slice_ns * NICE_0_WEIGHT / curr.weight
    update rq_vtime
```

`account_current(now)` 不是 shared trait 方法。其它调度类可以在同一生命周期 transaction 内维护自己的 private accounting，例如 future RR slice、RT budget 或 deadline runtime。EEVDF 必须保证所有调用路径都经过同一个 private helper，并在每次推进后刷新 `exec_start = now`，避免 tick 与 switch-out / requeue 双记。

`Vruntime`、`Deadline` 和 `rq_vtime` 第一版长期存储为 normalized nanoseconds 的 `u64` scalar；nice 0 下 `1ns` actual runtime 对应 `1` virtual ns。不引入额外 fixed-point fractional scale。所有 `delta_exec_ns * NICE_0_WEIGHT / weight` 与 slice/deadline 乘除都在 EEVDF private helper 内使用 `u128` 中间值，落回 `u64` 时统一 saturate 并记录 arithmetic anomaly；正 `delta_exec` 计算结果为 `0` 时必须至少推进 `1`，保证持续运行最终推进 `vruntime`。这些 helper 不向 `Scheduler` trait 或 `RunQueue` surface 扩散 `Result`。

`Instant::now()` 第一版不引入新的 scheduler time abstraction。scheduler core / `RunQueue` 在一个调度事务中读取一次 `Instant` 并传给 class transaction；阶段 0/1 必须审计该路径在 interrupt-disabled / tick / scheduler context 中不分配、不睡眠、不拿复杂锁、不会重入 scheduler。如果审计失败，停止并回到 RFC review，而不是预留抽象绕过。

EEVDF runtime accounting 必须发生在 runnable current requeue、wake handoff requeue、abort-park requeue、block park 或 exit switch 的 class transaction 内，不能依赖 `switch.rs::switch_out()` 中的 task/cpu usage hook 才更新公平状态。`switch.rs::switch_out()` 的 `Task::on_switch_out()` 仍保留为 task / CPU usage 等 context-switch bookkeeping。

### Enqueue、Requeue 与 Preempt Decision

路径语义由 method-first transaction 表达：

- `enqueue_new()`：只接受 fresh normal entity；如果没有有效 `vruntime`，初始化为 `vruntime = rq_vtime`，并按当前 nice weight 与 base slice 计算 `deadline`。
- `enqueue_woken()`：只在 stale-safe wake placement 返回 `Enqueued` 后调用，执行 bounded wake clamp。
- `handoff_woken_current()`：`ParkPending` 由 scheduler 收口并最终 requeue current 时调用，执行 exactly-once wake clamp。
- `requeue_aborted_wait_current()`：wait park 后 scheduler 复查发现 current 已 runnable 时调用，不执行 wake clamp，也不套 yield penalty。
- `requeue_yielded_current()`：先 `account_current(now)`，再执行 bounded yield penalty，再入队。
- `requeue_preempted_current()`：使用 `PendingResched` flags 表示 tick、runnable arrival 或二者同时存在；不套 wake clamp。
- `put_prev_blocked()` / `put_prev_exiting()`：只结算 current，不入队。
- no-switch abort：不调用 scheduler class transaction。

`decide_preempt_current()` 与 enqueue 分离，并且在 owner CPU placement 完成后调用。new task 和 wake task 的 placement 差异已分别由 `enqueue_new()` / `enqueue_woken()` 写入 candidate 的 class state，preempt decision 第一版不接收 `NewTask` / `Wake` source。`decide_preempt_current()` 先结算 current，再仅在 candidate eligible 且 candidate deadline 严格早于 current deadline 时返回 `PreemptDecision::RequestResched`；processor/core 随后设置 `request_resched(ReschedCause::RunnableArrival)`。`task_tick()` 返回 `TickAction::RequestResched` 时，processor/core 设置 `request_resched(ReschedCause::Tick)`。

`deadline = vruntime + slice / weight_normalized` 或等价形式。低 nice / 高权重 task 的 virtual runtime 推进更慢，deadline 间距也随权重归一化。

### Wake Placement Exactly Once

stale-safe wake placement 的结果只由 scheduler core / `RunQueue` facade 用来选择 class transaction，不进入 `Scheduler` trait：

- `Stale`：不调用 class，不改 EEVDF entity。
- `AlreadyCurrent`：不调用 class；current 后续继续执行或走 abort/no-park。
- `ParkPending`：不立刻 clamp；若 scheduler 后续收口入队，调用 `handoff_woken_current()` exactly once 执行 wake clamp。
- `AlreadyQueued`：不调用 class，不二次 clamp。
- `Enqueued`：调用 `enqueue_woken()`，执行普通 wake placement 的 exactly-once clamp。

这一边界不能通过 source-local flag、`WakeToken` debug id、`WakeEnqueueResult` 或 wait-core private state 驱动 EEVDF 算法。scheduler class 只看到已经被 core 线性化后的 transaction 调用。

### Tick 与 Yield

tick 不再等价于“每 tick 强制轮转”。`task_tick(current, now)` 可以更新 class-local state；EEVDF 在其中可调用 private `account_current(now)`，然后只在以下情况返回 `TickAction::RequestResched`：

- 当前任务耗尽 virtual slice，即 `current.vruntime >= current.deadline`。
- 存在 eligible 且 deadline 严格早于 current deadline 的 queued runnable task。

deadline 相等时保持 current，避免无意义抖动；non-eligible task 不得只凭更早 deadline 抢占 current。

`sched_yield()` 使用 bounded penalty。第一版只后推 yielding task 的 deadline，不修改 `vruntime`、nice 或 weight：

```text
deadline = max(deadline, rq_vtime + yield_penalty_window_ns * NICE_0_WEIGHT / weight)
```

如果没有其它 runnable task，yield 后立即重新选回自身是允许的；如果存在其它 eligible runnable task，yield penalty 必须让它们获得运行机会，同时 yielding task 不能被永久惩罚出公平队列。

### 策略常量

以下常量进入 Kconfig：

- base slice
- wake clamp window
- yield penalty window
- anomaly log / counter threshold

nice-to-weight 第一版采用固定 Linux 表，不提供 selector。`Task::nice()` 是唯一 weight truth；`setpriority()` / clone nice inheritance 后，下一次 owner CPU `account_current()`、enqueue、pick 或 preempt decision 读取最新 nice。已存在的 deadline 不因 renice 立即重算，2C 不引入远端 runqueue 重排、class migration 或直接修改 EEVDF payload 的路径。若未来需要替换权重表，应单独走 follow-up。

## 接受边界

接受本草案意味着 Anemone 默认 normal scheduler 可以按 `EEVDF-lite` 方向推进，并且 method-first scheduler class transaction surface、class-specific entity、幂等 accounting、wake placement exactly-once 和线性 EEVDF pick 语义可以进入实现 review。

接受本草案不表示：

- 第一版必须与 Linux EEVDF 完整行为一致。
- `sched_setscheduler()`、`sched_setattr()`、`setpriority()` 等用户 ABI 已经具备真实调度策略切换语义。
- 远端 task 可以直接修改目标 task 的 effective scheduler class、class-specific payload 或 owner CPU runqueue membership；这些修改若未来支持，必须成为 owner CPU `RunQueue` transaction。
- EEVDF 需要解决现有 wait-core / IRQ-off / long non-preemptible kernel path 的独立开放问题。
- `iozone` throughput 数字成为硬性接受目标。
- agent 必须运行 iozone、长 profile、LTP 或用户侧 baseline 分析。
- 接受 `SchedEvent`、`on_event` 或类似 catch-all event bus 作为 scheduler class contract；class-visible 语义必须通过 method-first class-local transaction surface 表达，只有局部算法复用确实需要时才允许窄参数或返回类型。

以下变化必须回到本 RFC 或新增 follow-up RFC：

- 允许 task 在不同 owner CPU 之间迁移。
- 引入 load balancing 或 remote runqueue observation。
- 将 realtime / deadline / idle policy accounting 升级为真实调度类。
- 引入 runtime scheduler policy / class switch，包括 queued/current/blocked task 的 class migration、旧 class accounting 收口和新 class placement 规则。
- 将 runqueue 容器换成多索引树结构并改变 pick / eligibility / fallback 语义。
- 改变 sched-split 的 wait-core `TaskSchedState`、token-bound wait sleep、preempt-defer、stale-safe wake placement 或 `on_runq` 状态所有权。
- 为了通过 benchmark 降低 eligibility、关闭 anomaly、弱化断言或隐藏 fallback。

## 备选方案

### 继续使用 RR 并调大/调小 tick 行为

拒绝作为长期方向。RR 可以作为过渡和对照，但它缺少权重、runtime accounting、sleep/wake placement 和公平时钟。继续调 tick 只能改变轮转频率，不能解决普通任务公平份额与响应性之间的结构问题。

### 引入完整 Linux EEVDF

延期。Linux EEVDF 包含更复杂的 lag、dequeue、decay、latency nice 和大量 scheduler framework 集成。直接追完整 Linux 行为会把第一版扩大成多子系统工程，并且与当前 fixed owner CPU / 无 cgroup / 无 load balance 的边界不匹配。

### 先做可插拔调度框架，EEVDF 以后再接

拒绝作为本 RFC 主线。当前已知问题是默认 normal scheduler 仍是临时 RR。先做空框架会产生接口迁移成本，却不提供新的公平语义。更好的顺序是为了 EEVDF 需要先扩展 method-first scheduler class transaction surface，并让 RR 只做行为保持适配。

### 使用 catch-all scheduler event bus

拒绝。`SchedEvent` / `on_event` 会把 entry permission、pending preempt source、wake placement、current requeue、switch-in/out accounting 等不同层次的语义打包成数据枚举，再让 class 解释事件。这会重新制造裸 `schedule()` 已经暴露过的语义丢失问题。路径语义必须由方法名和 `RunQueue` facade 的调用点表达。

### Deadline-only 调度器

拒绝。短 slice 任务天然拥有更早 virtual deadline，若缺少 eligibility / lag 约束，会长期偏向短 slice 任务。EEVDF 的核心恰好是允许短 slice 改善响应性，但 CPU 时间份额仍受公平性约束。

### 第一版使用 BTreeMap / RB-tree

延期。EEVDF pick 不是单一 key 排序：正常路径需要 eligible tasks 中最小 deadline，fallback 需要全队列最小 vruntime，而 eligibility 又依赖动态 `rq_vtime`。单个 `BTreeMap` 容易退化成 deadline-only；多个索引会抬高第一版复杂度。第一版先用线性扫描验证语义，树索引作为后续性能优化 gate。

## 风险

- `rq_vtime` 定义过弱会让 eligibility 失效，调度器退化成 deadline-only 或 vruntime-only。控制方式是 Gate P2 先验证公式，并观察 fallback anomaly。
- wake clamp 窗口过宽会允许 sleep-wakeup task 刷取过多正 lag；过窄会伤害交互式任务响应性。第一版必须用 Kconfig 常量和 smoke 证据约束。
- `account_current(now)` 若没有严格刷新 `exec_start`，tick / switch-out 会双记；若漏掉 deferred-preempt 不切换语义，又会提前结束执行段。
- 在 stale-safe wake path 中错误执行 wake clamp，可能让 stale wake、already queued task、already-current task、no-switch abort 或 abort-park requeue 获得重复奖励。
- bounded yield penalty 过轻会让 yielding task 立即选回；过重会造成不必要的长期惩罚。
- 单核关抢占下，EEVDF 只能改善到达调度点后的 runnable task 选择；长期 IRQ-off、不可抢占内核循环或 wait-core residual 仍可能造成 hang 或 starvation。
- 若后续 timer worker、OOM worker、`kthreadd` 或其它 service kthread 需要 bounded latency，而不仅是 eventual progress，不能在 EEVDF default switch 中保留隐式 RR 例外；必须回到 RFC review 引入显式 kthread priority / service class / RT-like 后续设计。

## 收口

文档层收口标准：

1. `rq_vtime` 最低约束、eligibility、wake clamp、yield penalty、slice / weight 公式和 anomaly 语义已经写入 canonical 文档或对应 probe gate。
2. `PendingResched` 已覆盖 tick 与 runnable-arrival 抢占来源，deferred preempt 由执行 `take_pending_resched()` 的 caller 恢复同一组 pending bits，且 scheduler class 不接收 `ScheduleMode` 或 wait-core private identity。
3. method-first scheduler class transaction surface 覆盖现有 schedule / requeue / wake path；accepted contract 中不存在 `SchedEvent` / `on_event` / catch-all event bus。
4. implementation plan 能先行为保持适配 RR，再引入 EEVDF，再切 default class。
5. bootstrap / kthread 直接进入 normal EEVDF 的 eventual-progress 证明已经进入 canonical 文本，且不再依赖 focused smoke 作为 accepted contract 决策。
6. `tracking-issues.md` 中影响实现顺序和验收边界的问题已有处理结论。

实现层收口标准：

1. RR 适配阶段行为保持。
2. 除 idle task 外，ordinary task、bootstrap task 和 kthread 默认进入 EEVDF class，且无 production RR 特例。
3. 多 runnable task 不稳定饿死，CPU 时间份额随 nice 权重方向变化。
4. wake/sleep workload 不因陈旧 lag、重复 wake reward、missing parked handoff clamp 或 stale wake clamp 饿死/刷分。
5. bounded yield penalty 让其它 runnable task 获得运行机会，同时 yielding task 不永久饿死。
6. fallback anomaly 有统计或日志观察面，并在稳定 workload 下不持续增长。
7. 用户侧 iozone / LTP / long fairness log 若提供反馈，能被正确归类到 EEVDF、sched-wait-preempt-arming 或其它 owner。
