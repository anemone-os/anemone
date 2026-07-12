# Sched EEVDF-lite 不变量需求

**状态：** Canonical
**最后更新：** 2026-07-12
**父 RFC：** [RFC-20260622-sched-eevdf-lite](./index.md)

本文定义 `EEVDF-lite` 作为默认 normal scheduler 时必须保持的状态所有权、公平性和 scheduler/wait 边界。当前版本以 sched-split 为已接受前提，并吸收 2026-07-11 Stage 3 runtime feedback：monotonic minimum-`vruntime` floor 已被证据否定，原 Checkpoint 2C / 2D closure 不再是完成依据。若后续实现需要改变 `TaskSchedState`、`PrePark/Parked`、token-bound wait sleep、preempt-defer 或 stale-safe wake placement，必须回到 `sched-wait-preempt-arming` RFC review，而不是在 EEVDF-lite 内局部绕过。

## 闭合条件

迁移完成后必须同时满足：

1. 除 idle task 外，ordinary user task、bootstrap task 和 kthread 默认使用 `EEVDF-lite`，不再依赖临时 RR 作为 production normal class。
2. task 的真实执行归属仍由 `Task::cpuid()` 表示，第一版不做跨核迁移。
3. `SchedEntity::on_runq()` 仍只表示 owner CPU runqueue 上的物理排队事实。
4. `SchedEntity` 不再被强行保持为 `Copy POD`；EEVDF state 必须位于 class-specific payload，不能污染 idle/RR 的语义。
5. task creation / clone 必须创建 fresh normal entity；clone 不得复制父 task 的 `SchedEntity` 或 EEVDF runtime state。
6. EEVDF pick 不是 deadline-only；eligible 约束必须来自当前 competition set 的 weighted FairClock，而不是 monotonic minimum-`vruntime` floor。
7. valid non-empty FairClock 必须至少有一个 eligible entity；no-eligible 是 membership / snapshot correctness bug，checked aggregate failure 才允许可观测的 fail-forward fallback。
8. runtime accounting 对每段实际执行时间只记一次，不重复推进 `vruntime`。
9. true block 必须在 final accounting 后保存有界 service lag，ordinary true wake 只消费一次；yield、preempt、`ParkPending` handoff 和 abort-wait requeue 保持 continuous membership，不得获得 wake reward。
10. `Nice` newtype 必须排除 `[-20, 19]` 外的内部状态；`Task::nice()` 返回唯一长期 nice truth，EEVDF entity 不得保存另一份长期 nice state。
11. pending resched request 必须用 `PendingResched` flags 区分 tick 与 runnable arrival，不得继续用 bool 静默压扁抢占来源。
12. scheduler class contract 必须是 method-first class-local atomic transaction surface；不得使用 `SchedEvent` / `on_event` / catch-all event bus 承载路径语义。
13. RR 行为保持适配阶段不得改变现有 wait/wake stale-safe placement 语义。
14. sched-split 的 `ScheduleMode` 和 processor-private `PendingResched` 不能泄漏为 scheduler class 所有的状态；class 层只在 preempted-current transaction 中按值读取 `PendingResched` flags。
15. bootstrap task、`kthreadd` 和普通 kthread 的第一版进展证明只要求 normal EEVDF eventual progress；不得用隐式 RR 例外、特殊优先级或单独 class 补齐该证明。
16. deadline 续期不得吞掉 current request completion；该瞬时事实必须由 EEVDF private accounting outcome 传给当前 class transaction，不能在续期后的 entity snapshot 中反推，也不能缓存为长期 entity / processor 状态。
17. ready queue 与 class-active current 必须互斥组成完整 competition set；pick / set-next 之间不得让 selected entity 从抽象集合中消失。
18. fixed-weight runtime accounting 必须具有分段不变性；transaction 切分不能通过每段最小推进重复增加 `vruntime`。
19. deadline catch-up 必须保持当前 request phase；初始化、自然续期或没有 outstanding yield penalty 的 catch-up 完成后，在无 arithmetic failure 时维持 `vruntime < deadline <= vruntime + request`。yield penalty 尚未耗尽时允许 deadline 暂时超过一个 base request，但该状态不代表 request completion。
20. `u64` saturation 不是可接受的长期算法状态；production closure 必须通过 checked arithmetic、headroom proof 与公共 coordinate rebase 防止饱和破坏 lag / deadline / progress。
21. runnable arrival 的 accounting、placement、enqueue 和 preferred-entity decision 必须在 owner CPU 上以同一 competition snapshot 线性化；decision 必须覆盖完整 `C`，不能只比较 current 与 new candidate。

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
4. `Nice` newtype 拥有 nice 值域和 weight-table index 不变量；Task 内部受约束的原子 nice 表示是 nice / weight source 的唯一长期真相源。
5. 普通任务服务账本、phase、saved service lag 和 accounting remainder 由 `SchedClassPrv::Eevdf(EevdfEntity)` 或等价 class-specific payload 拥有。
6. ready queue、class-active current association、competition membership 事务和公平算法由 owner CPU 的 `Eevdf` class 拥有；`Processor::running_task` 仍是执行上下文 truth，两者只允许在 scheduler handoff 的受控顺序内短暂不同。
7. task 的 effective scheduler class、class-specific payload 和 queue membership 只能由 owner CPU 的 `RunQueue` transaction 修改；远端 task 不得直接拿 `sched_entity` 锁改 class 或 EEVDF payload。
8. scheduler-private `ScheduleMode` 只属于 core entry permission，不得由 scheduler class 保存、缓存或解释为算法状态。
9. processor-private `PendingResched` 只属于 pending preempt request；class 可以按值读取它作为 `requeue_preempted_current()` 的 cause flags，但不得负责 restore 或驱动 processor pending state。执行 destructive `take_pending_resched()` 的 scheduler-core caller 负责在 deferred preempt 时恢复同一组 flags。
10. 用户态 scheduling ABI accounting 不是本 RFC 的行为真相源；未来 `sched_*` syscall 只能通过明确的后续 RFC 接入真实调度策略。

FairClock 的 `v0/A/W/V/V_floor` 是 owner-CPU transaction 派生 snapshot，不长期存储。R1 为隔离单变量 intervention，可以临时保留 `legacy_placement_floor`；该字段是只服务旧 fresh / ordinary wake / `ParkPending` placement 的 behavioral compatibility bridge，不是 eligibility、lag、pick、yield、preempt 或诊断 truth。字段声明必须写明允许的行为消费读点和 R2 删除条件；为维持旧坐标而执行的 update 站点不得成为算法决策读点，R2 source audit 必须证明 bridge 及其旧语义已完全删除。

entity 必须互斥表达至少以下逻辑 phase：

- fresh：尚未加入 competition set，`vruntime` / `deadline` 尚无行为意义；
- competing：位于 ready 或 class-active current，`vruntime` / `deadline` 有效；
- sleeping：不在 competition set，保存下一次 true join 要消费的 bounded exact-rational service lag，以及 accounting continuity 所需 remainder / remainder-weight metadata；旧绝对 `vruntime` / `deadline` 不再驱动行为。

clone inheritance 只能在 child 发布前通过 `&mut Task` 继承 typed nice 等对应 owner state，不能继承父 task 的 scheduler entity。新 task publication 必须从 fresh `SchedEntity::new_normal()` 或等价构造器进入 `enqueue_new()` placement，让 `vruntime` / `deadline` / `exec_start` 由 owner CPU EEVDF class 初始化。

第一版允许 `Task::set_nice(Nice)` 对已发布 task 做非事务性原子更新；syscall 和其它 caller 不得直接访问 raw atomic storage。方法注释必须说明它不提供 runqueue / deadline 强一致性，以及后续 owner CPU transaction 对直接原子写入的替换条件；无需为这一临时边界额外打印 renice 日志。未来动态 renice 必须像 remote enqueue 一样提交给 target owner CPU，在 `RunQueue` transaction 内按旧 nice 结算 current segment、发布新 nice 并完成 class-local update。

未来若支持 scheduler policy / class switch，source CPU 只能完成 ABI / permission / target identity 校验并向 target owner CPU 提交 command；queued、current、blocked 和 exiting task 的 class 迁移必须在 owner CPU `RunQueue` transaction 内线性化。本 RFC 不引入这项能力，只固定 future extension 不得绕过 owner boundary。

允许诊断字段记录 anomaly count、last anomaly reason、last class transaction 或 runtime snapshots。`anomaly` 是 EEVDF-lite 本地诊断概念，不是 Linux / EEVDF 标准状态；它用于记录 checked aggregate / representation failure 等不应在真实 workload 中命中的保护路径。每次 anomaly 记录必须通过 `kerrln!` 输出 reason 和累计次数；valid FairClock 下 no-eligible 由 release assertion 暴露，不降级成可持续 fallback。诊断字段和日志不得反向驱动调度选择，除非它们被提升为正式协议状态并进入本文。

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
6. `ReschedCause::RunnableArrival` 不代表 true join placement，不得触发 current 的 lag restore；它只说明 runnable-set change 使 current 需要在 preempt tail 重新调度。
7. `Yield`、tick preempt、runnable-arrival preempt、abort-park requeue 和 parked wake handoff 不能被合并为同一个 generic requeue transaction。
8. `Scheduler` trait 不提供通用 `enqueue_runnable()` 默认底座；会改变 membership 的 transaction 必须由每个 class 显式实现。

## Class-Local Atomic Transaction

`Scheduler` 虚表方法是 class-local atomic transaction：

1. 一个方法可以包含多个 class-private 步骤，例如 accounting、FairClock snapshot、lag placement、yield penalty、统计更新和内部队列操作。
2. scheduler core / `RunQueue` 不能拆开组合这些 class-owned 步骤，只能选择调用哪一个 transaction。
3. scheduler core / `RunQueue` 负责 transaction 之间的全局线性化顺序、owner CPU/noirq 事务、class dispatch、`on_runq` 和 `ntasks`。
4. runnable arrival 必须把 current accounting、join placement、enqueue 和 full-set preferred-entity decision 线性化到同一个 owner-CPU transaction。现有 `enqueue_new()` / `enqueue_woken()` 与 `decide_preempt_current()` surface 只有在 source proof 能建立该原子性时才可保留；否则 R2 必须先修订 method contract 和 write set。
5. preferred-entity decision 必须读取 placement 完成后的完整 `C`。它不能假定 new candidate 是唯一可能 winner，也不能在 current 已 ineligible 时只因 current deadline 更早而让其继续运行。
6. remote runnable arrival 的 preempt decision 必须在目标 task 的 owner CPU placement transaction 内执行；source CPU 不能读取或比较目标 CPU current。
7. `task_tick()` 可以更新 class-local state，但只能消费 class-private accounting outcome 并返回 `TickAction`，不能直接调用 scheduler core。
8. `pick_next_task()` 与 `set_next_task()` 保持分离；前者必须在 class 内完成 ready-to-active membership transfer，后者断言 active identity 并记录 next 开始运行。selected entity 在两者之间不得离开抽象 competition set。
9. 简单 class 若忽略路径差异，应在自己的 impl 内用私有 helper 复用逻辑，而不是依赖 trait 默认 generic enqueue。

## 线性化点

必须显式定义以下线性化点：

1. runtime accounting：EEVDF private `account_current(now)` 何时推进当前 task 的 `vruntime`，并刷新 `exec_start`。
2. switch-in / membership：`pick_next_task()` 选择并移出 class ready queue / 清 `on_runq` 的同一 class transaction 必须安装 active association；scheduler core 随后调用 class `set_next_task(task, now)` 断言 active identity，再执行地址空间切换准备，例如当前 `switch_mapping(prev, next)`，然后进入现有 `Task::on_switch_in()`、`set_current_task()` 和 architecture switch。
3. runnable requeue：当前 task 在 class requeue transaction 内已经通过 `account_current(now)` 更新 `vruntime` / `deadline`。
4. true join placement：current accounting、join 前 `W0/V0` snapshot、saved lag 恢复、deadline 重建、enqueue、`on_runq = true` 和 full-set preferred decision 的顺序。
5. wake placement：wait-core 已经把 task 逻辑状态转为 runnable 后，物理 placement 如何进入 owner CPU runqueue。
6. parked handoff：`ParkPending` 由 scheduler 收口为 physical requeue 时，如何在不离开 `C` 的前提下完成 active-to-ready transfer，且不保存 / 恢复 lag 或执行 wake reward。
7. true leave / abort-park requeue：true block 在 final accounting 后、离开 `C` 前保存 service lag；wait park 后 scheduler 复查发现 current 已 runnable 时保持 continuous membership、不带 wake reward 地重新入队。
8. FairClock failure：checked aggregate 无法构造时记录 arithmetic anomaly、选择最小 `vruntime` fail-forward task 的顺序；valid FairClock 下 no-eligible 直接暴露 correctness bug。

硬性要求：

1. `on_runq` 与 queue membership 不得长期不一致；同一个 lock / interrupt-disabled 事务内可以有受控过渡。
2. `vruntime` 更新必须发生在当前 task 被重新 enqueue、park switch 或 exit switch 之前。
3. `deadline` 更新必须基于已经推进后的 `vruntime`。
4. FairClock snapshot 只能由 owner CPU transaction 内的 ready / active competition state 派生，不能依赖跨 CPU 读取的 task 状态或长期 cached aggregate。
5. pick 不能选择非 runnable、waiting、zombie 或非当前 owner CPU 的 task。
6. `DeferredPreempt` 不得结束当前执行段；它只返回 deferred result，让执行 destructive take 的 caller 恢复 / 保留 resched 请求。
7. no-switch abort 不调用 scheduler class transaction。
8. 未真正切换到 next task 的路径不得调用 `set_next_task()`；bootstrap first task、idle fallback、block、yield 和 zombie 切换路径都必须能按同一顺序 source-audit。已由 `pick_next_task()` 安装 active association 但最终无法 switch 的任何新失败路径都必须在同一 owner transaction 撤销或回滚，不得留下 membership leak。
9. `exec_start` 从 `set_next_task()` 开始计入即将运行的 execution segment；如果实现期认为 mapping 准备时间必须排除在公平执行段外，必须回到 RFC review，而不是局部移动 `set_next_task()`。

## Runtime Accounting

EEVDF-lite 必须使用唯一幂等 helper 推进当前执行段：

1. EEVDF private `account_current(now)` 是推进 `vruntime`、更新 fixed-weight remainder、catch up `deadline` 和刷新 `exec_start` 的唯一入口；FairClock 由 transaction snapshot 派生，不在 helper 中维护第二份长期 aggregate。
2. `account_current(now)` 不是 shared trait 方法；trait 只暴露 current execution accounting 的生命周期点。
3. tick 和 switch-out / requeue 可以通过对应 class transaction 调用同一个 helper，但不得各自维护独立 `delta_exec` 计算。
4. 每次 `account_current(now)` 成功推进后必须刷新 `exec_start = now`，避免后续 switch-out 双记。
5. 如果 `now <= exec_start`，入口必须 no-op 或 fail closed，不能让 `vruntime` 倒退。
6. `Task::on_switch_out()` 仍是 task / CPU usage hook，不是 EEVDF fairness accounting truth。
7. 第一版直接使用 `Instant`；`Instant::now()` 必须通过阶段审计证明适合 scheduler noirq / tick path，不预设新的 scheduler time abstraction。
8. `Vruntime` 和 `Deadline` 的长期存储使用 normalized nanoseconds 的 `u64` scalar；nice 0 下 `1ns` actual runtime 对应 `1` virtual ns。FairClock aggregate 和 saved service lag 使用 checked `u128` / signed-magnitude rational 或等价可审查表示。
9. fixed weight 下使用 `remainder + delta_exec_ns * NICE_0_WEIGHT` 做除法，保存余数并满足 `0 <= remainder < weight`；任意 accounting 分段必须与合并结算产生相同总 `vruntime`。
10. 正 runtime 可以先积累在 remainder 中，不得为每个非零 segment 强制 `delta_vruntime >= 1`。持续执行最终推进由 remainder contract 证明。
11. deadline renewal 必须显式返回 `renewed` 与 arithmetic-failure 两个正交事实；有副作用的 renewal 不能放入会被 arithmetic anomaly 短路的布尔表达式。
12. `account_current(now)` 必须返回本次 accounting 是否完成并续期 current request。tick / runnable-arrival decision 在同一 class transaction 内立即消费该 outcome；已经承诺 switch / requeue / block / exit 的 transaction 可以显式丢弃，因为调度边界已经成立。
13. true join 后的 deadline rebuild / normalization 只服务 placement，不得被解释为 running request completion 或额外 resched 请求。
14. deadline renewal 必须按当前 request phase 严格 catch up：若 `vruntime >= deadline`，推进 `1 + floor((vruntime - deadline) / request)` 个 request；无 arithmetic failure 时保持 `vruntime < deadline <= vruntime + request`。yield penalty 可在到期前暂时放宽 `deadline - vruntime`，但不能被解释为 request completion。
15. `u64` saturation 不是可接受的最终状态。owner CPU 必须在上、下 headroom 不足前对全部 competing entity 的 `vruntime` / `deadline` 做同一个可加或可减的公共平移；必须证明当前坐标跨度仍能放入 `u64` 的 interior window，且 rebase 前后 `V-v`、`deadline-vruntime`、eligibility、service lag 和 pick 结果相同。
16. checked arithmetic failure 在 correction / debug 阶段可以记录独立 anomaly 并 fail forward，但任何真实命中都会让 gate 失败；production closure 必须证明该路径在接受边界内不可达。
17. 当前非事务性 runnable renice 只承诺 weak semantics；若要承诺 dynamic-weight lag conservation，必须由后续 owner-linearized reweight 同时处理旧 weight accounting、`vruntime` / `deadline` 变换和 remainder 换基，不能在 R1-R3b 中偷渡。
18. 即使保留 weak renice，accounting remainder 也不能把旧 denominator 直接套给新 weight；entity 必须保存解释 remainder 的 historical weight，并在观察到 weight 变化时按 `floor(remainder * new_weight / old_weight)` 向零换基，随后满足 `remainder < new_weight`。完整 lag-preserving reweight 属于独立 follow-up RFC / gate，不属于本次 R1-R3b 或阶段 4 收口。

## Competition Membership 与 True Leave / Join

competition set 为 `C = ready queue union class-active current`，两部分互斥：

1. fresh entity 与 true wake 是 join；true block 与 exit 是 leave。
2. yield、preempt、`ParkPending` handoff 和 abort-wait requeue 是 continuous membership，只在 ready / active 两个物理位置间移动。
3. `pick_next_task()` 必须完成 ready-to-active transfer；`set_next_task()` 只确认 active identity 并建立 `exec_start`。
4. `put_prev_blocked()` 必须在 final accounting 后、移出 `C` 前保存 service lag；`put_prev_exiting()` 直接丢弃离开后的调度状态。
5. 无路径语义的 generic `dequeue(task)` 不能默认解释为 true block；最终实现必须删除无调用者入口，或拆为明确的 leave / transfer / exit transaction。

true block 保存 weight-scaled service lag：

```text
G_i = w_i * (V - v_i)
```

`V` 是有理数，saved lag 必须使用规范化 exact-rational signed-magnitude 表示或等价类型，不能在 leave 时先 cast 到有符号窄类型或量化成整数。positive credit / negative debt 均在 service units 上做有界 clamp；checked representation failure 记录独立 anomaly，只允许以 zero-lag placement 保证 scheduler 前进，并让 R2 gate 失败。该 fallback 不能成为 production fairness 语义。

第一版不做 time-based lag decay，service bound 固定为：

```text
G_credit_max = NICE_0_WEIGHT * wake_clamp_window_ns
G_debt_max   = NICE_0_WEIGHT * max(2 * base_slice_ns, tick_period_ns)
```

`wake_clamp_window` 在 R2 改义为 maximum positive service credit，并且不得超过结构性 `max(2 * base_slice, tick_period)` 上界。positive credit / negative debt 的有理比较必须先约分或交叉乘，不能隐式向零取整。

设 join 前 existing set 的总 weight / FairClock 为 `W0/V0`，joining entity 当前 weight 为 `w`，待恢复 service lag 为 `G`。`W0 > 0` 时实数目标位置满足：

```text
v* = V0 - G * (W0 + w) / (w * W0)
```

整数 placement 对 positive credit 向上取整、对 negative debt 向下取整，不能增加 credit 或 debt；post-join service-lag 误差必须严格小于 `w * W0 / (W0 + w)`，因此也严格小于一个 entity-weight quantum。`G = 0` 时取 `floor(V0)`，保证 fresh entity eligible；`W0 == 0` 时相对 lag 无法表达，按 zero lag 加入预留上下 headroom 的固定中性 anchor 并消费 saved lag，不能把 sleeping entity 的旧绝对坐标带回空集合。

stale-safe wake 结果只选择 transaction：

1. `WakeEnqueueResult::Enqueued` 调用 `enqueue_woken()`，消费一次 sleeping phase 的 saved lag 并完成 true join。
2. `WakeEnqueueResult::{Stale, AlreadyCurrent, AlreadyQueued}` 不消费 saved lag，不修改 EEVDF placement。
3. `WakeEnqueueResult::ParkPending` 不消费 saved lag；后续 `handoff_woken_current()` 只完成 continuous active-to-ready transfer。
4. no-switch abort 不调用 class；`requeue_aborted_wait_current()` 保持 continuous membership，不走 wake placement，也不套 yield penalty。

禁止用 source-local flag、diagnostic wait id、`WakeToken::is_armed()`、`PollRegisterResult::Armed` 或 `WakeEnqueueResult` 作为 EEVDF lag / eligibility truth。

## 公平性与 Eligibility

EEVDF-lite 必须保持以下公平性边界：

1. 权重越高，单位实际执行时间推进的 `vruntime` 越慢。
2. 对同一 owner-CPU transaction 内固定的 competition snapshot，FairClock 定义为：

   ```text
   v0 = min(v_i)
   W  = sum(w_i)
   A  = sum((v_i - v0) * w_i)
   V  = v0 + A / W
   V_floor = v0 + floor(A / W)
   ```

3. eligible 判断使用 `v_i <= V_floor`，或严格等价的交叉乘 `A >= (v_i - v0) * W`。`v0/A/W/V/V_floor` 只属于本次 snapshot，不长期缓存。
4. join、leave 和 reweight 可以改变 weighted average，因此 FairClock 不要求全局 monotonic。minimum-`vruntime` 只可作为本次无符号算术原点，不能重新成为 behavioral floor。
5. non-empty positive-weight set 必有 eligible entity。valid FairClock 下 no-eligible 使用 release `assert!`；只有 checked aggregate invalid 时才允许记录 arithmetic anomaly 并选择最小 `vruntime` fail forward。
6. `deadline = vruntime + request`，其中 `request = max(1, floor(base_slice_ns * NICE_0_WEIGHT / weight))` 或等价形式；初始化、自然续期和没有 outstanding yield penalty 的 catch-up 保持当前 phase。yield 可以暂时把 deadline 推过一个 base request，但不产生 request-completion outcome；current accounting 触发续期时必须同时产生该 outcome，不能让续期后的 deadline 覆盖调度原因。
7. pick 只在 eligible entities 中选择最小 deadline；没有 request-completion outcome 时，deadline tie 的 preferred-entity 比较保持 current。current 已 ineligible 时，只要 ready set 非空就必须选择其它 eligible entity。
8. tick 与 runnable arrival 在 accounting / placement 完成后从完整 `C` 使用同一个 preferred-entity rule；new candidate 没有特殊 winner 身份。request-completion outcome 与该比较正交：存在其它 runnable peer 时，它可以独立请求重新选择，不能被 deadline tie 的 keep-current 规则覆盖。
9. fresh entity 以 zero service lag 加入；true wake 恢复 bounded saved service lag。fresh / wake placement 不得读取 wall-clock 作为公平坐标，也不得围绕 `legacy_placement_floor` 形成最终语义。
10. `sched_yield()` 只把 yielding task 的 deadline 后推到至少 `ceil(V) + yield_penalty_window_vruntime(weight)`，不得修改 `vruntime`、service lag、nice 或 weight，也不能把 yielding task 永久惩罚出公平队列。
11. 没有 eligible peer 时 yield self-pick 是合法 owed-service 结果；禁止 forced handoff、skip-current、扩大 penalty 或 case-specific yield 旁路替代 FairClock / lag 修复。
12. `Task::nice()` 是唯一 current weight truth。当前第一版保留 runnable renice weak semantics；不能宣称 dynamic-weight lag conservation，也不得为此引入远端 EEVDF payload 修改或 class migration。完整 reweight 另走独立 owner-CPU command / IPI follow-up gate。

禁止退化为：

- 只按 `deadline` 排序。
- 只按 `vruntime` 排序且完全忽略 `deadline`。
- 用 monotonic minimum-`vruntime` floor 作为 eligibility clock。
- 每 tick 无条件轮转。
- wake 时无条件把任务放到队首或最小 `vruntime`。
- yield 时无条件 forced handoff、无条件立即选回自己，或无界推后到长期饥饿。

## Bootstrap / Kthread Progress 边界

bootstrap task、`kthreadd` 和普通 kthread 第一版直接使用 normal EEVDF。本文证明目标是 owner CPU 上的 eventual scheduler progress，不是服务线程的 bounded latency：

1. 除 idle task 外，默认 task publication 只能创建 fresh normal entity；保留的 RR entity 必须是 debug / bisect 对照，不能参与 production placement。
2. owner CPU 的 normal runnable 集合是有限集合；每个 normal runnable task 的 `Task::nice()` 对应正且有限的 weight。
3. 如果一个 normal task 持续 runnable，segmentation-invariant accounting、weighted FairClock eligible pick、bounded service lag 和 bounded yield penalty 必须共同保证它不会被永久排除在 pick 之外。
4. fresh zero-lag join 和 true-wake bounded service-lag restore 不能制造永久领先或永久落后；`ParkPending` 等 continuous paths 不得凭空获得 wake credit。
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
8. strategy constants 中的 base slice、true-sleep service-credit bound、yield penalty 和 anomaly threshold 必须来自 Kconfig；现有 wake-clamp 配置名在 R2 改义或迁移时必须保留清晰的兼容 / 删除说明。
9. idle task 保持 fallback singleton；不通过 `requeue_*_current()` 物理入队。

阶段 1 可以主动做同一 scheduler owner 内的结构拆分，例如把 `RunQueue` facade 移到 `sched/class/runqueue.rs`，把 `SchedEntity` / `SchedClassPrv` 移到 `sched/class/entity.rs`，同时让 `Scheduler` trait 留在 `sched/class/mod.rs`。该拆分属于结构维护：行为保持、不扩大 public API、不改变 wait-core API、不改变 task topology。阶段 1 拆出 `entity.rs` 时，`SchedEntity` 的 RR/Idle 形状和 `Copy` 行为保持不变；阶段 2 再引入 EEVDF payload。

将 `SchedEntity` 拆为 class-specific payload、增加 `sched/class/eevdf.rs`、或引入 EEVDF-specific state，若保持同一 scheduler owner、行为保持、public API 不扩大，属于结构维护。若拆分改变 wait-core API、task topology、public scheduler policy 或 shared contract，必须回到 RFC review。

## 禁止退化项

以下做法不能作为最终关闭条件：

1. 用 benchmark 特化逻辑处理 `iozone` worker。
2. 为了让 `iozone` 数字好看关闭 eligibility。
3. 隐藏、降级或禁用 FairClock / arithmetic anomaly 的 `kerrln!` 报告和诊断记录。
4. 把 RR 保留为 production normal class，只把 EEVDF 作为未使用实验类。
5. 在 `sched_setaffinity()`、`sched_setscheduler()` 或 future syscall 中绕过本 RFC 的 owner CPU 不变量。
6. 为避免 starvation 放宽 wait-core stale-safe wake assertions。
7. 把 `rq_vtime` / `legacy_placement_floor` 作为不可审查的第二份 eligibility truth，或在 R2 后继续保留 behavioral bridge。
8. 用单个 `BTreeMap<deadline, task>` 替代算法语义。
9. 把 nice 值复制到多个长期字段并允许冲突。
10. 把 tree/index 优化和算法迁移混在同一不可回滚阶段。
11. 重新引入 scheduler owner 外的裸 `schedule()` 或公开 `ScheduleCaller` taxonomy。
12. 把 sched-split residual fairness gap 包装成 EEVDF 应兜底的目标。
13. 使用 `SchedEvent` / `on_event` 或类似 catch-all event bus 承载 scheduler class path semantics。
14. 通过 generic `enqueue_runnable()` 默认实现绕过 `enqueue_new()`、`enqueue_woken()`、`requeue_*_current()` 等 transaction。
15. 用 forced handoff、skip-current、扩大 penalty 或 testcase-specific yield 分支掩盖 min-floor singleton feedback。
16. 用每段 `max(1, floor(delta/weight))` 破坏 accounting 分段不变性。
17. 把 `u64` saturation 当作长期算法状态，或依赖 saturation 后的 deadline / lag 顺序继续调度。
18. 把 `ParkPending` handoff 当成 true wake，并保存 / 恢复 service lag 或施加 wake reward。
19. 在 placement 后只比较 current 与 new candidate，忽略既有 ready peer 可能成为 full-set winner。

## 完成标准

文档层完成标准：

1. weighted FairClock、ready / active membership、true leave / join service lag、full-set preferred decision、accounting remainder、deadline catch-up、coordinate rebase、bounded yield 和 anomaly 语义已经写入 canonical 文档与 correction gates。
2. method-first scheduler class transaction surface 覆盖所有现有 schedule / requeue / wake path，并与 scheduler-private `ScheduleMode` 分层。
3. accepted contract 中不存在 `SchedEvent` / `on_event` / catch-all event bus。
4. 实施计划保留 RR-to-EEVDF migration 历史，并把 Stage 3 correction 拆为 R1 / R2 / R3a / R3b；每门都有 hypothesis、write set、validation floor、failure signal、write-back 和 exit。
5. bootstrap / kthread direct EEVDF 的 eventual-progress 证明已经写入本文，且不依赖 focused smoke 作为 contract 决策。
6. `tracking-issues.md` 中影响实现顺序和验收边界的问题已有处理结论。

实现层完成标准：

1. RR 适配阶段行为保持。
2. 除 idle task 外，ordinary task、bootstrap task 和 kthread 默认进入 EEVDF class，且无 production RR 特例。
3. 多 runnable task 不稳定饿死，CPU 时间份额随 nice 权重方向变化。
4. true block / wake 保存并恢复有界 service lag；`ParkPending`、abort、preempt 和 yield 保持 continuous membership，不获得 wake reward。
5. bounded yield penalty 在 weighted FairClock 上有 focused smoke 与 instrumented runtime 证据；其它 eligible task 能运行，合法 owed-service self-pick 不被误修，yielding task 不永久饿死。
6. valid non-empty FairClock 必有 eligible entity；arithmetic / representation anomaly 有统计或日志观察面，并在真实 workload 中不命中。
7. fixed-weight accounting 拆分与合并结果一致；deadline catch-up 保持当前 phase（yield penalty 的有界暂态除外）；common rebase 前后 eligibility、lag、deadline 差和 pick 结果不变。
8. instrumented signal run 证明 min-floor singleton feedback 不再驱动 actual pick，clean tree 的 signal / read-write 对照完成 Stage 3 吞吐验收或按 R1 failure signal 回到 RFC review。
9. 用户侧 runtime log 若显示 wait-preempt residual，按 owner 路由回对应 RFC，不降低本 RFC 的 eligibility / accounting / placement 不变量。
