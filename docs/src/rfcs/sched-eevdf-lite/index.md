# RFC-20260622-sched-eevdf-lite

**状态：** Closed - deferred after Stage 3/R1 runtime acceptance failure
**负责人：** doruche, Codex
**最后更新：** 2026-07-14
**领域：** scheduler / fairness / runtime accounting / scheduler class
**事务日志：** [2026-07-09-sched-eevdf-lite](../../devlog/transactions/2026-07-09-sched-eevdf-lite.md)
**开放问题：** 见 [Tracking Issues](./tracking-issues.md)
**下一步：** 无 active gate。EEVDF 关闭时 default 曾恢复为 RR，随后已由 [Fair / Stride RFC](../sched-fair-stride/index.md) 切换为 Fair；若未来继续 EEVDF，必须先重新打开 RFC review，重新分类 R1 失败并批准新的实施顺序，不能直接从 R2 续跑。

> **共享 scheduler contract supersession（2026-07-14）：** 本 RFC 的 EEVDF 算法仍为 Closed/deferred；其中曾接受的 class-visible `ReschedCause` / `PendingResched` trait 参数已由 [Sched RT Class R1](../sched-rt-class/index.md) 取代。未来重开必须使用 core-only single-bit pending-pick snapshot，class policy continuation 由各 class 自己的状态 / transaction outcome 表达，不能恢复 cause-visible trait。

## 摘要

本 RFC 记录 Anemone 将默认普通任务调度器从 `RoundRobin` 迁移到 `EEVDF-lite` 的一次未完成尝试。第一版原目标不是完整复刻 Linux EEVDF，而是在当前 fixed owner CPU、per-CPU runqueue、无迁移、无 load balance 的边界内，引入带权重的 virtual runtime、virtual deadline、weighted fair clock、eligibility / service-lag 约束、true leave / join placement、bounded yield penalty 和幂等 runtime accounting。Stage 3/R1 runtime acceptance 失败后，本 RFC 延期关闭并把当时的 default 恢复为 RR；后续 Fair / Stride 已成为 production default。下文算法合同保留为未来重开时必须重新审查的设计义务，不代表当前生产实现已经满足。

本 RFC 以 `sched-wait-preempt-arming` / sched-split 为已接受的下层前提。scheduler core 已经通过 scheduler-private `ScheduleMode` 和语义化 wrapper 区分 `schedule_preempt()`、token-bound `schedule_wait_sleep()`、yield entry、idle entry 和 `schedule_zombie_never_return()`；EEVDF-lite 不重新设计 wait-core `PrePark/Parked`、token-bound wait sleep、stale-safe wake placement 或 preempt-defer contract。EEVDF-lite 只在这些入口之后补齐 method-first scheduler class lifecycle transaction、runtime accounting、placement 和 default normal class 语义。

早期动机来自单核 `iozone -t 4` 中 per-child `Min xfer = 0` 或极低的公平性异常。sched-split 已经改变了 wait/preempt 边界，因此该现象只保留为历史动机和用户侧反馈来源；新的 runtime log 若显示 wait-preempt residual，例如长 `PrePark` deferred 窗口或 source-owner nested wait，应路由回 `sched-wait-preempt-arming`，不要求 EEVDF-lite 兜底。

## 背景

阶段 1 至阶段 3 曾落地 method-first scheduler class surface、typed pending、EEVDF payload、EEVDF default normal constructor 和用户态 smoke。R1 runtime acceptance 失败后，当时的 default normal constructor 恢复为 RR；后续 Fair / Stride 又接管 shared default。EEVDF class 与 payload 作为实验实现保留，不再由 production entity constructor 默认创建。

2026-07-11 的用户运行与 exact-yield probe 证明原 Checkpoint 2C 的 monotonic minimum-`vruntime` floor 不是有效 eligibility clock。相同 `read-write` case / failure multiset 下，EEVDF profile 约为 RR 的 3.3 至 3.5 倍；signal profile 中 EEVDF 出现 `1,338,814` 次 min-floor `self_only_eligible` self-pick。对同一 entity snapshot 的 weighted-fair-clock counterfactual 显示，其中 `552,494` 次已有 eligible peer，`786,320` 次仍无 eligible peer。该证据同时否定 min-floor contract 和“每次 yield 强制 handoff”的窄修；完整证据见 [Stage 3 eligibility 与整体吞吐回归证据](./backgrounds/stage3-eligibility-regression-20260711.md)。

因此阶段 3 已停止。R1 虽完成 weighted FairClock 公式替换，但 instrumented signal 仍产生 `1,233,143` 次 yield self-pick 与 `1,232,735` 次 `self_only_eligible`，明确命中 gate failure signal。R2 / R3a / R3b 未执行，旧 2C / 2D 的 neutralized 结论不构成完成依据；本 RFC 现以 Closed/deferred 收口，而不是进入阶段 4 或标记 Completed。

scheduler class 仍使用 method-first class-local atomic transaction surface。路径语义由方法名和 `RunQueue` facade 的调用点表达；只有同一 class transaction 内确实需要算法复用时，才允许窄参数或返回类型，例如 class-private accounting outcome、`TickAction` 和 `PreemptDecision`。core `PendingResched` 不进入 class。若 R2 证明现有 enqueue / preempt surface 不能线性化 current accounting、join placement、enqueue 和 full-set preferred decision，必须先回到 method contract review 并记录 write-set 扩展，不能在旧接口间制造第二套状态。

调度核心已有几个必须保留的边界：

- task 创建后拥有固定 `Task::cpuid()`，该 CPU 是 task 的 owner CPU。
- 每个 CPU 拥有本地 `Processor` 和 `RunQueue`。
- `SchedEntity::on_runq()` 是 owner CPU runqueue 上的物理排队事实，不能被跨 CPU 随意读取或修改。
- `TaskSchedState` 拥有 runnable / waiting / zombie 逻辑状态；scheduler class 不能绕过 wait-core 直接重写这些状态。
- stale-safe wake placement 仍由 wait core 完成逻辑 wake 后调用 `wake_enqueue()`，并以 `WakeEnqueueResult` 表示物理 placement 结果。
- sched-split 后，scheduler owner 外不得重新引入裸 `schedule()` 或公开 `ScheduleCaller` taxonomy。

EEVDF-lite 要解决的是 normal runnable task 到达调度点后的公平选择和响应性，而不是 wait-core、IRQ-off、long non-preemptible kernel path 或 source-owner sleepability 的独立缺陷。用户侧保存和分析的 baseline / 实作后 runtime log 可作为 implementation feedback；agent 不把 iozone、长日志、LTP profile 或 deferred-count trace 作为本 RFC 的必跑验证。

## 目标

以下是本 RFC 的历史目标，当前关闭不宣称已经达成；未来重开时必须逐项重新确认。

- 将 `EEVDF-lite` 作为默认 normal scheduler，替换临时 RR；除 idle task 外，ordinary user task、bootstrap task 和 kthread 第一版都进入 EEVDF normal class。
- bootstrap task、`kthreadd` 和普通 kthread 第一版直接使用 normal EEVDF；本 RFC 只承诺它们在有限 runnable 集合中的 eventual scheduler progress，不承诺 bounded latency 或特殊服务优先级。
- 保持 fixed owner CPU、per-CPU runqueue、无迁移、无 load balance 的调度边界。
- 扩展 `Scheduler` trait 为 method-first class-local atomic transaction surface，使调度类能在明确生命周期点维护 class-local state。
- 明确 `ScheduleMode`、core-only `PendingResched` 与 scheduler class transaction 的分层：entry permission、pending-pick request 和 class-visible accounting / placement transaction 不能互相泄漏。
- 为普通任务维护 class-specific EEVDF state：`vruntime`、`deadline`、`exec_start`、fixed-weight accounting remainder、fresh / competing / sleeping phase、bounded saved service lag 和 anomaly 诊断字段。
- 为 task creation / clone 定义 fresh normal entity 初始化路径；clone 可以继承 nice，但不得复制父 task 的 `SchedEntity` 或 EEVDF runtime state。
- 使用受值域约束的 `Nice` newtype 作为 nice domain；`Task::nice()` 返回 task 持有的唯一长期 nice truth，EEVDF entity 不复制 nice，也不在第一版保存 `cached_weight`。
- 使用固定 Linux nice weight 表；base slice、true-sleep service-credit window、yield penalty window 和 anomaly threshold 进入 Kconfig。
- trait 暴露 current execution accounting 的生命周期点；EEVDF 必须通过 class-private `account_current(now)` 幂等结算当前执行段，tick 和 switch-out / requeue 不得重复推进同一段 `delta_exec`。
- 使用 owner-CPU competition set 的 weighted FairClock 约束 eligibility，避免退化成 deadline-only 或 minimum-`vruntime`-only 调度器；competition set 包含 ready queue 和 class-active current，且二者互斥。
- 定义 true leave / join placement：true block 在 final accounting 后保存有界 service lag，ordinary true wake 在 stale-safe placement 返回 `Enqueued` 时消费一次；yield、preempt 和 `ParkPending` handoff 保持 continuous membership，不保存、恢复或奖励 wake lag。
- 为 `sched_yield()` 定义 bounded yield penalty，避免 yield task 立即无界选回，也避免永久饿死。
- 第一版使用线性 `Eevdf` class 容器和 O(n) pick/dequeue；树索引作为后续优化 gate。
- 为 invalid FairClock、checked arithmetic failure 和其它不应到达的保护路径记录 anomaly；每次记录都通过 `kerrln!` 输出 reason 和累计次数。anomaly 是 EEVDF-lite 本地诊断概念，不是 Linux / EEVDF 标准状态，也不得反向驱动调度决策。valid non-empty FairClock 下没有 eligible entity 是 correctness bug，不是正常 fallback 分支。
- 建立 agent / user 验证责任分层：agent 负责 build、source audit 和 focused smoke；用户侧 runtime log / iozone / LTP 作为反馈材料。

## 非目标

- 不实现 Linux 完整 EEVDF 细节，例如 delayed dequeue、time-based lag decay、latency nice、cgroup scheduling、utilization clamp 或 bandwidth controller；第一版仍必须保存并恢复 true sleep 所需的 bounded service lag。
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

- [背景材料索引](./backgrounds/index.md)
- [Stage 3 eligibility 与整体吞吐回归证据（2026-07-11）](./backgrounds/stage3-eligibility-regression-20260711.md)

## 方案

第一版引入 `Eevdf` 作为默认 normal scheduler class。`RoundRobin` 可以保留为 trait plumbing 的行为保持对照、debug 或 bisect class，但 default switch 完成后不得仍是 production placement path；所有 `SchedClassPrv::RoundRobin` 保留点必须被分类。`Idle` 仍维持 fallback singleton 语义。

### Entry、Pending Request 与 Class Transaction Surface

sched-split 的 `ScheduleMode` 仍保持 scheduler-private，但需要用路径语义命名：

- `WaitSleep`：token-bound explicit wait sleep，有权消费 `PrePark`。
- `Preempt`：trap / preempt tail 的 involuntary preempt，只能抢占 runnable current；遇到 `Waiting/PrePark` 返回 deferred。
- `Yield`：`sched_yield()` / `yield_now()` 入口，current 必须 runnable，调度类可执行 bounded yield penalty。
- `Idle`：idle loop 专用入口，idle task 保持 fallback singleton，不进入 normal requeue。
- `Zombie`：exit no-return 入口。

preempt request 保持 scheduler core / processor-private。当前 shared contract 使用 typed single-bit `PendingResched`，只表示一次 full pick 尚未完成：

```rust
struct PendingResched {
    pending: bool,
}
```

`request_resched()` 只把 bit 置真；多 producer request 合并。trap tail 通过 `take_pending_resched()` 或等价 API 取得 snapshot 后进入 `schedule_preempt(pending)`；idle loop 只把非空 pending 作为离开 idle 并调用 `schedule_idle()` 的触发。若 preempt 因 `Waiting/PrePark` deferred，执行 destructive take 的 caller 必须 union restore 同一 snapshot，避免抢占请求被吞掉。

`PendingResched` 是 core-only 值 snapshot，不是 processor state capability。`schedule_preempt(pending)` 只用非空值证明 entry 合法；pending 不进入 `ScheduleMode`、`ScheduleDecision`、`RunQueue` 或 `requeue_preempted_current()`。EEVDF 若未来需要把 request-completion outcome 延续到后续 class transaction，必须由 EEVDF owner 明确设计 class-local 状态 / outcome，而不是恢复 core cause taxonomy。

`Scheduler` trait 不提供 catch-all event 方法，也不提供通用 `enqueue_runnable()` 默认底座。会改变 runqueue membership 的 transaction 必须由每个 class 显式实现；简单 class 若不关心路径差异，可以在自身 impl 内用私有 helper 复用逻辑。

第一版 class-visible transaction surface 至少包含：

```rust
trait Scheduler {
    fn enqueue_new(&mut self, task: Arc<Task>);
    fn enqueue_woken(&mut self, task: Arc<Task>);
    fn dequeue(&mut self, task: &Arc<Task>) -> bool;

    fn requeue_yielded_current(&mut self, task: Arc<Task>, now: Instant);
    fn requeue_preempted_current(&mut self, task: Arc<Task>, now: Instant);
    fn handoff_woken_current(&mut self, task: Arc<Task>, now: Instant);

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
- runnable arrival 的 current accounting、new / wake placement、enqueue 和 full-set preferred decision 必须在 owner CPU 上共享一个线性化 transaction。现有 `enqueue_*()` / `decide_preempt_current()` 方法可以保留，但不能靠分离方法名掩盖 snapshot 间隙；R2 若无法证明原子性，必须先修订 method surface。
- remote new-task / wake arrival 不能在 source CPU 读取目标 CPU current。若 placement 通过 IPI 发生，FairClock snapshot、join placement 与 preferred decision 也必须在目标 owner CPU 内完成。
- `task_tick()` 是可变生命周期 transaction，只返回 `TickAction`，不直接调用 scheduler core。
- `pick_next_task()` 和 `set_next_task()` 分离；前者选择、移出 ready queue 并在 class 内安装 active association，后者断言 active identity 并记录 next 开始运行，组合顺序由 `RunQueue` / scheduler core 负责。
- switch-in 顺序固定为 `pick_next_task()` 完成 ready-to-active transfer / 清 `on_runq`，scheduler core 随后调用 `set_next_task(task, now)`，再执行地址空间切换准备，例如当前 `switch_mapping(prev, next)`，然后进入现有 `Task::on_switch_in()`、`set_current_task()` 和 architecture switch。`exec_start` 从 `set_next_task()` 开始计入即将运行的 execution segment；若实现期认为 mapping 准备时间必须排除在公平执行段外，必须停下回到 RFC review。未真正切换的路径，例如 no-switch abort 和 deferred preempt，不得调用 `set_next_task()`，也不得结束 current 的 execution segment。

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
- `exec_start`
- fixed-weight accounting remainder 与解释 remainder 单位所需的 historical weight
- fresh / competing / sleeping phase
- sleeping phase 的 bounded exact-rational service lag
- anomaly count / last anomaly reason 诊断字段或等价统计入口

普通 task、bootstrap task、kthread 和 clone child 都必须通过 `SchedEntity::new_normal()` 或等价 fresh constructor 创建 class payload。clone 只能继承父 task 的 nice / credentials / address-space 等对应 owner state，不能通过 `current_task.sched_entity()` 复制父 task 的 `vruntime`、`deadline`、`exec_start`、`on_runq`、phase、remainder 或 saved lag。fresh normal entity 在第一次 `enqueue_new()` 时以 zero service lag 加入当前 competition set。

bootstrap task、`kthreadd`、timer worker 和 OOM worker 等内核线程不获得隐式 RR 保留点。它们的调度证明与 ordinary task 相同：在 owner CPU 上成为 runnable 后，只依赖 normal EEVDF 的正权重、weighted FairClock、bounded service lag 和 bounded yield penalty 获得 eventual progress。它们自身的等待、唤醒、停止和每轮主动 `yield_now()` 属于 kthread / worker owner 的 lifecycle 纪律，不提升为 EEVDF 特殊优先级。

`Nice` newtype 唯一约束 `[-20, 19]` 值域和 weight-table index；Task 内部只保存受约束的原子 nice 表示，`Task::nice()` 返回唯一长期 truth。EEVDF 按该 typed value 即时计算 weight，第一版不保存长期 `cached_weight`，也不把 nice 移入 `SchedEntity`。clone 只允许在发布前通过 `&mut Task` 继承 nice；已发布 task 的 raw setter 不对 syscall 暴露。

### `Eevdf` Class

`Eevdf` 与 `RoundRobin`、`Idle` 命名对齐，内部持有线性 ready queue、class-active current association 和必要统计。第一版使用 `Vec<Arc<Task>>` 或等价线性容器：

1. duplicate enqueue 必须暴露 bug。
2. dequeue missing 必须暴露 bug。
3. pick 从 eligible tasks 中选择最小 deadline。
4. valid non-empty FairClock 下必须至少有一个 eligible entity；checked aggregate 无法构造时才允许 fallback 到最小 `vruntime` 并记录 arithmetic anomaly。
5. 如果 EEVDF queue 为空，RunQueue 选择 idle task。

owner CPU 上的 EEVDF competition set 定义为：

```text
C = ready queue union class-active current
```

ready 与 active 两部分互斥。fresh、true-blocked 和 exiting entity 不在 `C`；yield、preempt 和 `ParkPending` handoff 只在 ready / active 之间移动，不能制造 membership 空洞。`pick_next_task()` 必须在 class 内把 selected entity 从 ready 迁到 active，`set_next_task()` 只确认 active identity 并建立 `exec_start`；若实现需要等价但不同的方法 surface，R2 必须先回到 method contract review。

对同一个 owner-CPU transaction 内固定的 entity / weight snapshot，定义：

```text
v0 = min(v_i)
W  = sum(w_i)
A  = sum((v_i - v0) * w_i)
V  = v0 + A / W
V_floor = v0 + floor(A / W)
```

eligibility 使用 `v_i <= V_floor`，生产实现可以用等价的无除法交叉乘 `A >= (v_i - v0) * W`。`v0` 只是本次 checked aggregate 的无符号算术原点，不能持久化并反向驱动调度；`V` / `V_floor` 是 transaction-derived snapshot，不是第二份长期 truth。join、leave 或 reweight 会改变 weighted average，因此 FairClock 不具备跨 membership change 的全局 monotonic 要求。

线性实现使用无分配 two-pass scan：第一遍为每个 entity 读取一次 `vruntime` / current weight 并构造 `v0/A/W`，第二遍只读取 owner-CPU transaction 内不变的 `vruntime` / `deadline` 做 eligibility 与 pick，不再次读取 weight。这样不需要长期 `avg_vruntime` / `avg_load` cache；非事务性 remote renice 仍只承诺已声明的 weak semantics，不能据此宣称 dynamic-weight lag conservation。

只要 `C` 非空、所有 weight 为正且 aggregate 有效，最小 `vruntime` entity 必然 eligible。valid FairClock 下 no-eligible 使用 release `assert!` 暴露 membership / snapshot bug；checked aggregate 无法构造时才记录独立 arithmetic anomaly 并 fallback 到最小 `vruntime` 保证 scheduler 前进。该 fallback 是失效保护，不是可长期接受的算法分支。

### Runtime Accounting

runtime accounting 的生命周期点由 trait transaction 暴露；EEVDF 的具体执行段结算由 class-private helper 完成：

```text
account_current(now):
    request_completed = false
    delta_exec = now - exec_start
    if curr.weight != curr.remainder_weight:
        curr.remainder = floor(
            curr.remainder * curr.weight / curr.remainder_weight
        )
        curr.remainder_weight = curr.weight
    numerator = curr.remainder + delta_exec_ns * NICE_0_WEIGHT
    delta_vruntime = floor(numerator / curr.weight)
    curr.remainder = numerator % curr.weight
    curr.vruntime += delta_vruntime
    curr.exec_start = now
    if curr.vruntime >= curr.deadline:
        request = max(1, floor(base_slice_ns * NICE_0_WEIGHT / curr.weight))
        count = 1 + floor((curr.vruntime - curr.deadline) / request)
        curr.deadline += count * request
        request_completed = true
    return request_completed
```

`account_current(now)` 不是 shared trait 方法。其它调度类可以在同一生命周期 transaction 内维护自己的 private accounting，例如 future RR slice、RT budget 或 deadline runtime。上面的伪代码先展示 fixed-weight accounting；其中 `curr.weight` 是本次从唯一 `Task::nice()` truth 计算出的局部值，不是长期 `cached_weight`；只有 `remainder_weight` 作为解释 remainder 单位的 historical metadata 被保留。如果观察到当前 weight 与该 denominator 不同，必须先按 `floor(remainder * new_weight / old_weight)` 换基 remainder，并更新其解释 weight，再结算本段。该换基只保持 fractional accounting 的可解释性，不承诺 dynamic-weight lag conservation。EEVDF 必须保证所有调用路径都经过同一个 private helper，并在每次推进后刷新 `exec_start = now`，避免 tick 与 switch-out / requeue 双记。fixed weight 下的 remainder 使任意 accounting 分段得到与合并结算相同的总 `vruntime`；不得继续用每段 `max(1, floor(...))` 对频繁 transaction 重复收费。

deadline catch-up 保留当前 request phase，并在初始化、自然续期或没有 outstanding yield penalty 的 catch-up 完成后，在无 arithmetic failure 时保持 `vruntime < deadline <= vruntime + request`。yield 可以有界地把 deadline 提到 `ceil(V) + penalty`，因此 penalty 尚未耗尽时允许 `deadline - vruntime` 暂时超过一个 request；这不是 request completion，等该 deadline 到期后仍按当前 phase 严格 catch up。deadline 续期会把 `vruntime >= deadline` 重新归一化为假，因此 helper 必须把“本次至少完成一个 request”作为 class-private outcome 显式返回；decision transaction 不能在续期后从 entity snapshot 反推该瞬时事实，也不能为此增加 entity 持久 flag 或 processor-global 第二真相源。

`Vruntime` 和 `Deadline` 第一版长期存储为 normalized nanoseconds 的 `u64` scalar；nice 0 下 `1ns` actual runtime 对应 `1` virtual ns。不引入额外 fixed-point fractional scale。乘加、FairClock aggregate、service-lag placement 和 request catch-up 使用 checked `u128` 或等价更宽 helper。`u64` saturation 不能作为最终算法状态：一旦 `vruntime` / `deadline` 饱和，就无法继续证明 deadline order、lag 或 progress。owner CPU 必须在上、下坐标 headroom 接近声明的最大位移前，对所有 competing entity 做同一个可加或可减的公共平移；公共平移不得改变 `V-v`、`deadline-vruntime`、eligibility 或 pick 结果，并且必须证明当前坐标跨度仍能放入 `u64` 的 interior window。

checked arithmetic failure 可以在 correction / debug 阶段记录独立 anomaly 并 fail forward，但任何真实命中都会让对应 gate 失败。production closure 必须通过 headroom proof、proactive rebase 或更宽 checked representation 证明该路径在接受边界内不可达。这些 helper 不向 `Scheduler` trait 或 `RunQueue` surface 扩散通用 `Result`。

`Instant::now()` 第一版不引入新的 scheduler time abstraction。scheduler core / `RunQueue` 在一个调度事务中读取一次 `Instant` 并传给 class transaction；阶段 0/1 必须审计该路径在 interrupt-disabled / tick / scheduler context 中不分配、不睡眠、不拿复杂锁、不会重入 scheduler。如果审计失败，停止并回到 RFC review，而不是预留抽象绕过。

EEVDF runtime accounting 必须发生在 runnable current requeue、wake handoff requeue、block park 或 exit switch 的 class transaction 内，不能依赖 `switch.rs::switch_out()` 中的 task/cpu usage hook 才更新公平状态。`switch.rs::switch_out()` 的 `Task::on_switch_out()` 仍保留为 task / CPU usage 等 context-switch bookkeeping。

### Enqueue、Requeue 与 Preempt Decision

路径语义由 method-first transaction 表达：

- `enqueue_new()`：只接受 fresh normal entity，以 zero service lag 加入当前 competition set，并建立新 request。
- `enqueue_woken()`：只在 stale-safe wake placement 返回 `Enqueued` 后调用，消费一次 true block 保存的 bounded service lag，并重建 request。
- `handoff_woken_current()`：`ParkPending` 由 scheduler 收口并最终 requeue current 时调用；该路径没有离开 competition set，不保存或恢复 lag，也不执行 wake reward / clamp。
- `requeue_yielded_current()`：先 `account_current(now)`，再执行 bounded yield penalty，再入队。
- `requeue_preempted_current()`：保持 continuous membership，不执行 true join / lag restore；不读取 core pending。request-completion 等瞬时算法结果必须在同一个 EEVDF transaction 内消费，或由未来 review 接受的 class-local contract 延续。
- `put_prev_blocked()`：final accounting 后、移出 `C` 前保存 bounded exact-rational service lag；sleeping entity 的旧绝对 `vruntime` / `deadline` 不再驱动行为。
- `put_prev_exiting()`：final accounting 后离开 `C`，丢弃 lag、deadline 和其它不再需要的调度状态。
- no-switch abort：不调用 scheduler class transaction。

true join placement 必须看到 current accounting 后的同一 competition snapshot。owner CPU 上的 current accounting、new / wake placement、enqueue 和 preferred-entity decision 必须组成一个线性化 transaction；若现有 `enqueue_*()` 后再 `decide_preempt_current(current, candidate, now)` 的 surface 无法证明这一点，R2 必须先修订 method contract。placement 后从完整 `C` 计算 preferred entity，不能只比较 new candidate 与 current；candidate 加入导致既有 ready peer 成为 winner 时同样请求 resched。

tick 与 runnable-arrival decision 使用同一个 full-set preferred-entity 规则：current 只有在自身 eligible，且没有其它 eligible entity 拥有严格更早 deadline 时才能继续；deadline 相等只表示 preferred-entity 比较本身偏向 current，不覆盖 class-private request-completion outcome。processor/core 只合并 full-pick request，不编码或替代该 accounting outcome。

`deadline = vruntime + slice / weight_normalized` 或等价形式。低 nice / 高权重 task 的 virtual runtime 推进更慢，deadline 间距也随权重归一化。

### Competition Membership 与 True Join Placement

stale-safe wake placement 的结果只由 scheduler core / `RunQueue` facade 用来选择 class transaction，不进入 EEVDF entity 的算法状态：

- `Stale`：不调用 class，不改 EEVDF entity。
- `AlreadyCurrent`：不调用 class；current 后续继续执行或走 abort/no-park。
- `ParkPending`：current 从未离开 `C`；若 scheduler 后续收口入队，调用 `handoff_woken_current()` 只完成 active-to-ready transfer，不执行 true wake placement。
- `AlreadyQueued`：不调用 class，不二次恢复 lag。
- `Enqueued`：调用 `enqueue_woken()`，消费一次 sleeping phase 保存的 service lag 并完成 true join。

true block 保存 weight-scaled service lag：

```text
G_i = w_i * (V - v_i)
```

`G_i` 使用规范化 exact-rational 表示并在 service units 上做有界 clamp；不能在 leave 时先量化成整数。true join 使用 join 前 existing set 的 `W0/V0` 和 entity 当前 weight 恢复同一 service amount；整数 `vruntime` 只允许在 placement 时产生有方向约束、严格小于一个 entity-weight quantum 的误差。`W0 == 0` 时相对 lag 无法表达，按 zero lag 加入预留上下 headroom 的固定中性 anchor 并消费 saved lag，不能把 sleeping entity 的旧绝对坐标带回空集合。

这一边界不能通过 source-local flag、`WakeToken` debug id、`WakeEnqueueResult` 或 wait-core private state 驱动 EEVDF 算法。scheduler class 只看到已经被 core 线性化后的 transaction 调用。第一版不实现 time-based lag decay，但不能把 `ParkPending` continuous handoff 伪装成 wake reward。

### Tick 与 Yield

tick 不再等价于“每 tick 强制轮转”。`task_tick(current, now)` 调用 private `account_current(now)` 后，只在以下情况返回 `TickAction::RequestResched`：

- accounting outcome 表示当前任务刚耗尽并续期至少一个 virtual request，且 `C` 中存在其它 runnable peer。
- full-set preferred-entity 判断发现其它 eligible entity 拥有严格更早 deadline，或 current 已不再 eligible。

只有 current 时，request 续期不制造无意义 resched。没有 request-completion outcome 时，deadline 相等的 preferred-entity 比较保持 current，避免无意义抖动；存在其它 runnable peer 时，request completion 仍可独立请求重新选择。non-eligible task 不得只凭更早 deadline 抢占 current。禁止在续期后重新检查 `current.vruntime >= current.deadline` 作为 request-completion 判断，因为正常 checked 路径下该条件已经被续期归一化为假。

`sched_yield()` 使用 bounded penalty。第一版只后推 yielding task 的 deadline，不修改 `vruntime`、nice 或 weight：

```text
deadline = max(deadline, ceil(V) + yield_penalty_window_ns * NICE_0_WEIGHT / weight)
```

该 penalty floor 是有界的 voluntary-yield 状态；在它尚未耗尽时，`deadline - vruntime` 可以暂时超过一个 base request，但不能被当作 request completion。deadline 到期后的 strict catch-up 才恢复 `vruntime < deadline <= vruntime + request` 的 base-request 不变量。

如果没有其它 eligible runnable task，yield 后立即重新选回自身是合法 owed-service 结果；不能用 forced handoff、skip-current 或 case-specific yield 分支伪造 eligibility。如果存在其它 eligible runnable task，bounded penalty 应让更合适的 peer 获得运行机会，同时 yielding task 不能被永久惩罚出公平队列。

### 策略常量

以下常量进入 Kconfig：

- base slice
- true-sleep service-credit window（当前配置名仍为 wake clamp window，R2 负责改义 / 迁移）
- yield penalty window
- anomaly error-summary threshold

现有 `wake clamp window` 在 R2 中改义为 true sleep 可携带的最大正 service credit；它不再表示围绕 legacy floor 修改 `vruntime` 的普通 wake / parked-handoff clamp。R1 可以暂时保留同名配置给 `legacy_placement_floor` bridge，但该 bridge 的行为消费读点必须由 gate 限定；维护其旧坐标的 update 站点不得驱动 eligibility、pick、yield 或 preempt，并在 R2 删除旧语义。

nice-to-weight 第一版采用固定 Linux 表，不提供 selector。`Task::nice()` 是唯一 weight truth；`setpriority()` / clone nice inheritance 后，后续 owner CPU `account_current()`、enqueue、pick 或 preempt decision 读取最新 nice。已存在的 deadline 不因 renice 立即重算，第一版也不承诺 renice 时刻与 current execution segment 的精确切分。`Task::set_nice(Nice)` 是已发布 task 的唯一写入方法；其注释明确当前直接原子写入不提供 runqueue / deadline 强一致性，并在后续 owner CPU `RunQueue` command / IPI 事务接管动态调度属性修改时被替换。第一版不引入远端 runqueue 重排、class migration 或直接修改 EEVDF payload 的路径。若未来需要替换权重表，应单独走 follow-up。

## 接受边界

本 RFC 当前不再授权继续实现。历史接受边界曾允许 method-first scheduler class transaction surface、class-specific entity、weighted FairClock、continuous competition membership、true leave / join service lag、分段不变 accounting 和线性 EEVDF pick 语义进入 correction review；R1 失败后，该授权已关闭。

最终处置为：EEVDF 关闭时先恢复 RR，后来由 Fair / Stride 接管 production default；EEVDF 保留为非默认实验实现；R2 / R3a / R3b 不再是 active gate。未来若继续这条线，必须先重新打开 RFC review，处理 `EEVDF-001` / `EEVDF-018` / `EEVDF-004` / `EEVDF-020`，并重新批准 gate 顺序、write set、验证边界和默认切换条件。

接受本 RFC 不表示：

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
- 把 `legacy_placement_floor` 提升为最终 eligibility / lag truth，或让它在 R2 后继续存在。
- 用 forced handoff、skip-current、扩大 yield penalty 或 case-specific yield 分支掩盖 FairClock / lag 缺陷。
- 把 arithmetic saturation 当作可长期接受的算法状态，或在未证明 headroom / rebase 的情况下关闭 `EEVDF-020`。

## 备选方案

### 继续使用 RR 并调大/调小 tick 行为

EEVDF 关闭时曾选定 RR 作为延期期间的 production default；后来 Fair / Stride 已 supersede 该选择。RR 不提供本 RFC 原计划中的权重、service lag 或 weighted FairClock 语义；恢复 RR 是当时失败后的安全处置，不等于仅靠调整 tick 已解决长期公平性目标。

### 引入完整 Linux EEVDF

延期。Linux EEVDF 包含更复杂的 lag、dequeue、decay、latency nice 和大量 scheduler framework 集成。直接追完整 Linux 行为会把第一版扩大成多子系统工程，并且与当前 fixed owner CPU / 无 cgroup / 无 load balance 的边界不匹配。

### 先做可插拔调度框架，EEVDF 以后再接

未选择另建空框架。已经落地且独立正确的 method-first transaction surface、`RunQueue` facade、typed pending 与 class payload 继续保留；EEVDF 关闭时恢复 RR，后续 production default 迁移到 Fair，EEVDF 是否继续仍由未来 RFC review 决定。

### 使用 catch-all scheduler event bus

拒绝。`SchedEvent` / `on_event` 会把 entry permission、pending preempt source、wake placement、current requeue、switch-in/out accounting 等不同层次的语义打包成数据枚举，再让 class 解释事件。这会重新制造裸 `schedule()` 已经暴露过的语义丢失问题。路径语义必须由方法名和 `RunQueue` facade 的调用点表达。

### Deadline-only 调度器

拒绝。短 slice 任务天然拥有更早 virtual deadline，若缺少 eligibility / lag 约束，会长期偏向短 slice 任务。EEVDF 的核心恰好是允许短 slice 改善响应性，但 CPU 时间份额仍受公平性约束。

### 第一版使用 BTreeMap / RB-tree

延期。EEVDF pick 不是单一 key 排序：正常路径需要 weighted FairClock eligibility 下的最小 deadline，arithmetic-failure fallback 需要最小 `vruntime`，而 join / leave 还会改变 competition snapshot。单个 `BTreeMap` 容易退化成 deadline-only；多个索引会抬高 correction 复杂度。第一版先用线性扫描验证语义，树索引作为后续性能优化 gate。

## 风险

- weighted FairClock 的 competition membership 或 snapshot 不闭合会重新制造错误 eligibility。控制方式是 R1 先隔离 fair-clock 公式，R2 再关闭 ready / active membership 和 true join / leave。
- saved service-lag 界限过宽会允许 sleep-wakeup task 携带过多 credit；过窄会伤害交互式任务响应性。R2 必须在 service units 上定义有界 exact-rational contract，并用 unequal-weight round trip 证明 placement 误差。
- `account_current(now)` 若没有严格刷新 `exec_start`，tick / switch-out 会双记；若漏掉 deferred-preempt 不切换语义，又会提前结束执行段。
- 每段强制推进 `vruntime` 或 deadline 重置会让 transaction 频率改变公平账本。R3a 必须证明 accounting segmentation invariance 和 multi-request phase preservation。
- 在 stale-safe wake path 中错误消费 saved lag，可能让 stale wake、already queued task、already-current task、`ParkPending` handoff 或 no-switch abort 获得重复奖励。
- bounded yield penalty 过轻或过重都可能影响响应性，但不得在 R1 中与 FairClock 同时改动，否则无法识别 singleton feedback 的因果贡献。
- `u64` 坐标 saturation 会破坏 deadline、lag 和 progress 证明；R3b 必须在到达上限前公共 rebase，不能把 fail-forward fallback 当作收口语义。
- 单核关抢占下，EEVDF 只能改善到达调度点后的 runnable task 选择；长期 IRQ-off、不可抢占内核循环或 wait-core residual 仍可能造成 hang 或 starvation。
- 若后续 timer worker、OOM worker、`kthreadd` 或其它 service kthread 需要 bounded latency，而不仅是 eventual progress，不能在 EEVDF default switch 中保留隐式 RR 例外；必须回到 RFC review 引入显式 kthread priority / service class / RT-like 后续设计。

## 收口

本 RFC 以 **Closed - deferred after Stage 3/R1 runtime acceptance failure** 收口，明确不是 Completed。当前实现能够启动、运行普通 workload，并维持既有 LTP result set，但其 eligibility、competition membership、sleep/wake lag 和 accounting contract 尚未闭合，且存在显著吞吐回归与百万级 yield self-pick feedback。因此它是可运行的实验原型，不是可接受的 EEVDF 实现，也不适合作为默认调度器。

最终处置：

1. EEVDF 关闭时 production default 恢复为 RR，随后由 Fair / Stride supersede；`eevdf.rs` 归档源与其中的 focused tests、历史证据保留，但不再接入 production class graph，避免把实验实现误当作已验收策略。
2. 不整体回滚 EEVDF 历史。保留 method-first `Scheduler` trait、`RunQueue` transaction facade、core-only typed `PendingResched`、`SchedEntity` class payload、fresh clone / constructor，以及 `Nice` 与 priority syscall 整理。
3. R1 只证明 actual min-floor 已替换为 weighted FairClock，没有证明主要吞吐因果已经关闭；R2 / R3a / R3b 均未执行。
4. `EEVDF-001` / `EEVDF-018` / `EEVDF-004` / `EEVDF-020` 保持未解决 Keter，不得因 RFC 关闭而 Neutralized。
5. 未来继续 EEVDF 时不得从 R2 直接续跑；必须重新打开 RFC review，根据 R1 failure 重新分类假设、gate 顺序和验收条件。
