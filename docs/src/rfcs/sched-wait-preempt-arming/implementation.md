# Sched Wait Preempt Arming 迁移实施计划

**状态：** Completed
**最后更新：** 2026-07-12
**父 RFC：** [RFC-20260618-sched-wait-preempt-arming](./index.md)
**不变量：** [不变量需求](./invariants.md)

本文选择 schedule entry split / scheduler-private narrow mode 作为第一阶段实现方向。核心目标是让 trap return 等 involuntary preempt 入口不能消费 wait-core `PrePark`，同时保留 explicit wait sleep 对 `PrePark -> Parked` 的正常语义。实现期执行事实记录在 [2026-07-06-sched-wait-preempt-arming](../../devlog/transactions/2026-07-06-sched-wait-preempt-arming.md)。

本计划同时要求补齐 single-active-wait 诊断：如果 `Latch::begin_current()` / `ActiveWait::begin()` 之后的 register scan、predicate precheck 或 source lock 慢路径进入另一个 scheduler wait，wait-core 必须用 release assert / 可定位诊断暴露。fanotify 等 source owner 的具体修复不属于本 RFC 默认 write set；这类 panic 是对应 owner 的 follow-up 反馈，除非违规发生在 scheduler/wait-core 自身新增路径内。

## 迁移原则

- 修复边界放在 scheduler / wait-core park 协议，不放在单个 fs source。
- `TaskSchedState` 仍是当前 task 调度等待状态的唯一真相源。
- scheduler owner 外不得直接调用无语义的裸 `schedule()`。
- 底层 `ScheduleMode` 是 scheduler-private 状态机输入，不是 source-local 状态，也不是 producer trigger 能力。
- involuntary preempt 只能抢占 `Runnable` current task；不能 park `Waiting/PrePark`，不能把 waiting task 重新入队。
- explicit wait sleep 是 wait round 消费 park intent 的入口；它必须绑定 wait token 或 wait-core 私有 park permit，在 wake prerequisites 安装完成或可证明安全后调用。
- post-begin register scan / precheck 不得进入 nested scheduler wait；本 RFC 负责诊断暴露和反馈路由，不默认修复所有 source owner。
- preempt-deferred 不能吞掉已经被 caller 清除的 `need_resched`。
- processor pending-resched 是面向下一次 owner-CPU full pick 的合并 latch：successful pick 统一确认，no-pick path 保留或恢复；不升级为跨 context switch 的事件协议。
- `schedule_preempt()` deferred 返回后没有发生 context switch；trap tail 必须执行 deferred-task disposal。
- preempt-defer 只关闭 not-park-ready wait 被误 park 的 correctness 缺口；它不是任意长 `PrePark` setup 的公平性机制。post-begin setup 必须通过字段级审计和 trace gate 证明短小、不可阻塞、不可嵌套 wait。
- 不把 `PreemptGuard`、irq guard 或等价 guard 跨过 context switch。
- 不把可能 sleep 的 source register scan 放进 non-preemptible 区域。
- 不通过关闭 `kernel_preempt`、busy polling、延长 timeout 或弱化 assert 解决问题。

## 允许带入实现的反馈假设

以下不确定性允许作为 implementation feedback gate 带入阶段 0 到阶段 3。它们只能优化实现路线，不能削弱 [不变量需求](./invariants.md) 的目标或接受边界。

1. 调用面反馈：阶段 0 的源码审计可以发现新增或遗漏的裸 `schedule()` caller、`Latch::begin_current()` direct user、`Event::listen*()` user、direct wait helper、finite timeout wrapper、signal wait 或 clock sleep 调用面。发现结果回写本文件的阶段清单；如果出现无法通过 entry split 表达的 caller，停止并回到 `index.md` / `invariants.md` 补边界。
2. source sleepability / boundedness 反馈：阶段 0 / 阶段 3 可以按真实锁路径和扫描规模把 post-begin register scan / precheck 分为“已证明短小且不会阻塞”、“会触发 single-active-wait 诊断并归属 source owner follow-up”、“需要 write set 扩展”或“必须回到 publish split / park permit 设计”。任何分类都不得让 wait-core 支持 nested active wait，也不得引入 source-local park-ready truth；若 trace 显示 `PrePark` setup 过长或 deferred 过多，必须回到 RFC review。
3. finite timeout 反馈：`Event::listen_with_timeout()`、source-backed finite timeout latch、no-source timeout 和 `wait_current_with_timeout()` 的 timer-installed / source-registered / already-completed no-park / explicit-schedule 关系必须在阶段 2 成为 wait-sleep proof，并由阶段 3 trace 复核。若证据显示 active finite-timeout wait 不能在 explicit wait sleep 前建立 timeout prerequisite，或 already-completed wait 会被误 park / 误 requeue，停止并回到 RFC review。
4. preempt-defer 落点反馈：`schedule_preempt()` deferred 后 `need_resched` 恢复、deferred-task disposal、begin-to-explicit-sleep 窗口长度 / deferred count trace 字段和最小验证 floor 的具体实现点可以由阶段 1 / 阶段 2 代码形状决定；若实现需要改变 entry split、wait identity、completion 线性化点或 signal delivery contract，停止并回到 RFC 文档层。

反馈归属：

- 不改变 accepted contract 的审计、trace、checkpoint 和验证事实记录到 transaction devlog。
- 改变阶段顺序、write set、验证 floor、review gate 或停止条件时，更新本文件并在 transaction devlog 记录原因。
- 改变目标、不变量、状态所有权、ABI / 可见语义或接受边界时，停止当前 gate，更新 `index.md` / `invariants.md` 和 [Tracking Issues](./tracking-issues.md) 后再继续。
- fanotify 等 source owner 的 nested-wait panic 作为对应 owner follow-up 反馈；不要在本 RFC 内用兼容层或 source-specific workaround 静默绕过。

## 阶段 0：Preflight 与 Single-Active-Wait 诊断

前置条件：

- `index.md` 与 `invariants.md` 已明确选择 schedule entry split / preempt-defer。

交付：

- 增强 `ActiveWait::begin()` / `begin_wait()` 的 release assert：如果当前 task 已经处于 active wait，panic 信息必须包含当前 task id、已有 wait id、已有 wait caller location、当前尝试 begin 的 caller location。
- `WaitState` 增加 diagnostic-only origin，第一阶段只保存 `core::panic::Location::caller()`；不引入 `WaitPrimitive`、`operation` 字符串或其它 caller taxonomy。
- origin 是诊断字段，不参与行为决策，不作为 wake identity、park permission、source registration truth 或 scheduler mode。
- `ActiveWait::begin()` / `begin_wait()` 及主要 wait adapter 使用 `#[track_caller]` 或等价显式传参传播 caller location。若某个 adapter 暂时只能记录 adapter 内部 callsite，也必须在阶段 0 清单中说明，不得用手写 operation 字符串补第二套分类。
- 增加 crate 内 no-nested-wait 诊断 helper，例如 `#[track_caller] assert_current_not_in_active_wait()`。该 helper 只读取当前 task sched-state snapshot，panic 信息必须包含 current task、已有 wait id、已有 wait caller location 和 sleep-attempt caller location；它不能修改状态，也不能成为新的 active-wait truth。
- 第一批强制接入点是 `Mutex::lock()`：在 `allow_preempt()` 断言前先检查当前 task 是否已处于 active wait，并在 compare-exchange 失败后、调用 `Event::listen_uninterruptible()` 前再次检查。这样 post-begin 普通锁竞争会在普通同步入口暴露，而不是只等到 `Event` 内部二次 begin。
- `Event::listen*()` 可按实现形状选择是否在入口复用 no-nested-wait helper；correctness 仍由 begin-side assert 保底。不要给 helper 增加 operation 字符串或 primitive taxonomy。
- 不在 `schedule_wait_with_timeout()` 内调用 no-nested-wait helper；它是当前 active wait round 的合法 explicit wait-sleep 点。
- 生成当前所有裸 `schedule()` caller 的分类清单。
- 生成 `Latch::begin_current()` direct users 和 direct wait helper 的字段级清单。
- 生成 `Event::listen*()` user 清单，至少覆盖 futex wait 和 threaded timer completion wait；区分 production syscall path 与测试/内部 wait path。
- 确认 preempt-deferred 后 `need_resched` 的保存/恢复责任落点。
- 生成 post-begin 可睡眠路径和 boundedness 清单，标出普通 `Mutex::lock()`、`Event::listen*()`、`ActiveWait::begin()` 是否可能发生在 active wait 已发布之后，并记录 source scan / predicate precheck 的规模来源、锁边界、第一处 explicit wait-sleep 点和 deferred-window 观测字段；若归属 fanotify 等 source owner，只记录反馈路由，不要求本 RFC 修复。

必须运行的搜索：

```sh
rg -n "schedule\(\)" anemone-kernel/src
rg -n "Latch::begin_current" anemone-kernel/src
rg -n "wait_current_with_timeout|ActiveWait::begin|schedule_wait_with_timeout" anemone-kernel/src
rg -n "fn lock\(|listen_uninterruptible|listen_with_timeout" anemone-kernel/src/sync anemone-kernel/src/sched
rg -n "\.listen\(|\.listen_with_timeout\(" anemone-kernel/src
```

当前最低裸 `schedule()` caller 分类起点：

| Caller | 目标入口 | 说明 |
| --- | --- | --- |
| `arch/riscv64/exception/trap/{utrap,ktrap}.rs` | `schedule_preempt()` | trap return `kernel_preempt && need_resched` |
| `arch/loongarch64/exception/trap/{utrap,ktrap}.rs` | `schedule_preempt()` | trap return `kernel_preempt && need_resched` |
| `sched::higher_level::schedule_wait_with_timeout()` | token-bound `schedule_wait_sleep()` 或等价 no-park / timer-installed proof | wait 已完成则 no-park 返回；wait 仍 active 时先安装 timeout callback 后进入 explicit sleep |
| `sched::event::Event::listen*()` | token-bound / permit-bound `schedule_wait_sleep()` | listener 已注册并完成 predicate/signal recheck 后进入 explicit sleep |
| `sched::higher_level::yield_now()` | `schedule_runnable()` | 入口断言 current 为 `Runnable` |
| `sched::class::idle` idle loop | `schedule_runnable()` 或 `schedule_idle()` wrapper | idle task 只应是 `Runnable`，wrapper 不得消费 wait state |
| `task/api/exit::schedule_never_return()` | `schedule_zombie_never_return()` | exiting task cleanup 已完成；scheduler 在 noirq no-return 事务内发布 `Zombie` 并切走 |

当前最低 `Latch::begin_current()` direct users：

| File | 最低计数 | 调用面 |
| --- | --- | --- |
| `fs/api/iomux/wait.rs` | 2 | source-backed iomux wait；no-source timeout wait |
| `fs/eventfd.rs` | 2 | blocking read；blocking write |
| `fs/fanotify/group.rs` | 1 | blocking read wait |
| `fs/timerfd.rs` | 1 | blocking read wait |

当前最低 `Event::listen*()` users：

| File | 调用面 |
| --- | --- |
| `task/api/futex/futex.rs` | futex wait；`listen()` 和 `listen_with_timeout()` 都是 user-visible blocking syscall path |
| `time/timer/threaded.rs` | threaded timer completion test / internal wait |

每个 direct user 必须记录：

- begin point。
- predicate recheck。
- source registration、timeout install 或 ready / unsupported 分类。
- lock / sleepability boundary。
- post-begin 是否可能阻塞或 nested wait。
- post-begin setup 是否短小、有无用户规模驱动循环、是否可能导致长时间 preempt-deferred。
- first possible explicit wait-sleep entry。
- finish / cancel path。
- 第一阶段 entry split 修复是否覆盖；如果排除，说明理由。
- 若违规归属 source owner follow-up，记录 owner、触发路径和失败信号。

write set：

- `anemone-kernel/src/sched/wait.rs`
- `anemone-kernel/src/sync/mutex.rs`
- 如需让 `Latch::begin_current()` / `ActiveWait::begin()` / `Event::prepare_listener()` 传播 `#[track_caller]` caller location，允许同一 owner 内调整 `anemone-kernel/src/sched/latch.rs`、`anemone-kernel/src/sched/event.rs` 和 wait-core public adapter 签名。
- 如需让 `Mutex::lock()` 调用 crate 内 no-nested-wait helper，允许在 `anemone-kernel/src/sched/mod.rs` 增加窄 re-export；不得为此暴露 wait-core 内部状态结构。
- 文档/审计记录在 transaction devlog。

验证：

- 审计清单必须能解释为什么 no-source timeout、source-backed latch、eventfd、timerfd、fanotify、futex wait、clock sleep 和 signal wait 都不需要 source-local park truth。
- 审计清单必须把 post-begin 可睡眠路径分成三类：已证明短小且不会阻塞、会触发 single-active-wait 诊断并归属 source owner follow-up、需要回到文档层扩展设计。
- `Mutex::lock()` 的 no-nested-wait 检查必须先于 `allow_preempt()` 遮蔽诊断上下文，并覆盖 slow path 进入 `Event::listen_uninterruptible()` 前的位置。

退出条件：

- 所有裸 schedule caller 都能迁移到 `schedule_preempt()`、`schedule_wait_sleep()`、`schedule_runnable()` / `schedule_idle()` 或 `schedule_zombie_never_return()`。
- 如果出现无法通过 entry split 表达的 caller，停止并回到文档层补边界。
- single-active-wait release assert 能定位已有 wait caller location 和新 begin caller location；sleep-attempt helper 能定位已有 wait caller location 和当前 sleep-attempt caller location。
- `Event::listen*()` user 清单已覆盖 futex wait；若新增 Event wait 用户不能归入现有 wait-sleep proof，停止并补设计。
- post-begin setup 的 boundedness 分类已记录；若某路径依赖任意长 source scan 才能到 explicit wait sleep，停止并回到 publish split / park permit 方向。
- 阶段 0 反馈已按“调用面反馈”和“source sleepability 反馈”归类；任何改变 accepted contract 的发现必须先回写 RFC 文档层。

## 阶段 1：Scheduler-Private Mode 与 Wrapper 分流

前置条件：

- 阶段 0 caller 分类完成。

执行反馈：

- 2026-07-06 阶段 1 首轮实现发现：如果只在 `sched/mod.rs` 内拆出 `schedule_inner(mode)`，无法自然实现 token-bound `schedule_wait_sleep()`，因为 `sched/mod.rs` 能看到当前 `TaskSchedState::Waiting { state, .. }`，但 `WakeToken` 没有行为用的 current-wait identity 比较入口。用户批准把 `anemone-kernel/src/sched/wait.rs` 纳入阶段 1 最小扩展 write set，只允许提供 scheduler-private token/current-wait identity check 或等价 wait-core private permit；不得用 diagnostic `wait_id()` / `debug_id()` 或 completion-open `is_armed()` 代替 identity proof。
- 同一轮反馈还批准阶段 1 直接迁移原阶段 2 中依赖裸 `schedule()` 的 schedule-entry call sites，避免为了保持阶段边界而保留不自然的兼容桥。该提前迁移只覆盖 trap preempt、Event explicit wait sleep、finite-timeout helper、yield、idle 和 zombie exit 的 wrapper 接入；不降低阶段 2 / 阶段 3 对 source-backed finite timeout proof、boundedness、trace 和 runtime validation 的要求。
- 2026-07-08 post-close 反馈发现：原 `schedule_zombie_never_return()` 设计把 exit 模块先写 `TaskSchedState::Zombie`、随后再进入 no-return schedule，导致单核 `kernel_preempt` 下 trap-tail preempt 可在两者之间观察到 zombie current，并命中 `schedule_preempt()` 的 release invariant panic。修正后的 contract 是由 scheduler owner 在 `ScheduleMode::Zombie` 的 noirq no-return 事务内完成 `Runnable -> Zombie` 发布并立即切走；`Zombie` current 再次进入该入口是强不变量异常。

交付：

- 将裸 `schedule()` 私有化为 `schedule_inner(mode)` 或等价函数。
- 引入 scheduler-private narrow mode，例如 `ScheduleMode::{WaitSleep, Preempt, Runnable, Zombie}`。
- 对 scheduler owner 外暴露语义化 wrapper：`schedule_wait_sleep()`、`schedule_preempt()`、`schedule_runnable()` / `schedule_idle()`、`schedule_zombie_never_return()` 或等价命名。
- 引入能表达 preempt defer 的返回结果，例如 `SchedulePreemptResult::{Scheduled, Deferred}` 或等价机制。
- 底层 `schedule_inner()` 状态机按 mode 区分是否允许消费 wait-core park state。

建议实现形状：

```rust
enum ScheduleMode {
    WaitSleep,
    Preempt,
    Runnable,
    Zombie,
}

enum SchedulePreemptResult {
    Scheduled,
    Deferred,
}
```

语义表：

| Current `TaskSchedState` | WaitSleep | Runnable | Preempt | Zombie |
| --- | --- | --- | --- | --- |
| `Runnable` | 仅当本 wait token 已经完成时走 no-park / abort-sleep 返回；否则强不变量异常 | requeue/yield 或 idle switch | requeue/yield 或正常抢占 | 发布为 `Zombie` 并进入 no-return switch |
| `Waiting { park: PrePark }` | 推进为 `Parked`，再执行现有 abort-park recheck | 强不变量异常 | 返回 `Deferred`；恢复/保留 `need_resched`；不 park、不 requeue、不 context switch | 强不变量异常 |
| `Waiting { park: Parked }` | 维持 parked 并按现有 wait-core park 路径处理 | 强不变量异常 | 强不变量异常，记录 wait id 并 assert | 强不变量异常 |
| `Zombie` | 强不变量异常 | 强不变量异常 | 强不变量异常，trap preempt 不应抢占 zombie current | 强不变量异常，重复 zombie entry 表示状态提前发布或重复 exit |

`schedule_zombie_never_return()` 是独立 no-return wrapper：只允许已完成 exit cleanup 且仍为 `Runnable` 的 current task 进入退出调度路径，由 scheduler core 在 noirq 事务内发布 `Zombie` 并切走；它不通过可返回 `Runnable` / `WaitSleep` 语义表达。

实现要求：

- 上表中的 `WaitSleep` 不是无参泛用 schedule mode。实现必须让 wait-sleep wrapper 携带 `WakeToken`、`WaitSleepPermit` 或等价 wait-core 私有身份；如果 `ScheduleMode` enum 本身不携带 token，wrapper 也必须在进入 `schedule_inner()` 前后用同一 wait identity 完成 active / already-completed / stale 校验。
- `Deferred` 路径必须在 `switch_out()` 之前返回。
- `Deferred` 路径不能调用 `local_requeue_current()`，因为 current task 仍为 `Waiting/PrePark`，不是 `Runnable`。
- 如果 caller 在进入 scheduler 前已经通过 `fetch_clear_need_resched()` 清掉 resched 请求，`schedule_preempt()` 必须在 `Deferred` 返回前调用 `mark_need_resched()` 或等价恢复。
- `WaitSleep` 路径必须先用 wait token 区分 already-completed no-park 与 active wait：如果 token 命名的 wait round 已经完成且 current 已回到 `Runnable`，直接走 abort-sleep 返回，不能把它当作普通 runnable yield；如果 wait 仍为 `Waiting/PrePark`，先推进为 `Parked`，再执行现有 abort-park recheck；若 wait 在转换后完成为 `Runnable`，走 abort park / requeue。
- `Runnable` / idle wrapper / zombie wrapper 不能获得消费 `Waiting/PrePark` 的权限。zombie wrapper 只能在 scheduler owner 内把 `Runnable` current 发布为 `Zombie`，不能接受已经可观察为 `Zombie` 的 current task 作为幂等输入。
- scheduler-private mode 不写入 `WaitState`，不暴露给 `LatchTrigger`，不由 fs source 保存。
- 不得对 scheduler owner 外暴露无 token / permit 的 `schedule_wait_sleep()`；否则 future caller 可以把普通 `Runnable` yield 或 stale wait 误解释成 wait-sleep abort。

write set：

- `anemone-kernel/src/sched/mod.rs`
- 如需返回值或 helper 暴露，允许同一 owner 内调整 `anemone-kernel/src/sched/processor.rs`
- 经 2026-07-06 用户批准，允许最小调整 `anemone-kernel/src/sched/wait.rs`，只用于提供 scheduler-private token/current-wait identity check 或等价 wait-core private permit。
- 经 2026-07-06 用户批准，允许提前迁移原阶段 2 的裸 `schedule()` call sites：`anemone-kernel/src/arch/riscv64/exception/trap/{utrap,ktrap}.rs`、`anemone-kernel/src/arch/loongarch64/exception/trap/{utrap,ktrap}.rs`、`anemone-kernel/src/sched/event.rs`、`anemone-kernel/src/sched/class/idle.rs`、`anemone-kernel/src/task/api/exit/mod.rs`。不得借此修改 fs source owner、iomux source register contract、signal delivery contract 或 source-local park-ready truth。

验证：

```sh
rg -n "ScheduleMode|SchedulePreemptResult|schedule_inner|schedule_preempt|schedule_wait_sleep|schedule_runnable|schedule_idle|schedule_zombie" anemone-kernel/src/sched anemone-kernel/src/arch
rg -n "local_requeue_current" anemone-kernel/src/sched/mod.rs anemone-kernel/src/sched/processor.rs
```

退出条件：

- scheduler core 中 `Waiting/PrePark` 在 preempt mode 下没有任何 `Parked` 转换或 requeue 路径。
- `Waiting/Parked` 在 preempt mode 下不是 silently accepted。
- preempt-deferred 的 resched request 不会被静默吞掉。
- `schedule_inner(mode)` 是 scheduler-private；scheduler owner 外无裸 `schedule()` 入口。

## 阶段 2：迁移 Schedule Entries 与 Wait-Sleep Proof

前置条件：

- 阶段 1 wrapper 与 private mode 已存在且通过源码审计。

交付：

- trap return 抢占入口改用 `schedule_preempt()`。
- explicit wait sleep、Event listen、yield、idle 和 exit 入口改用各自精确 wrapper。
- 删除或私有化无 mode 的裸 `schedule()` 入口，避免新 caller 默认为错误语义。
- 将 finite timeout 的 timer-installed / already-completed no-park proof 落在 `schedule_wait_with_timeout()` 或等价 token-bound helper 内，避免每个 caller 各自解释 timeout prerequisite。
- 落下阶段 3 要消费的 wait-sleep 观测点：timeout-installed point、already-completed no-park / abort-sleep outcome、`PrePark -> Parked` 的 schedule entry，以及 source-registered / ready / unsupported 分类结果。

调用点规则：

- `arch/*/exception/trap/{utrap,ktrap}.rs`：调用 `schedule_preempt()`。若返回 `Deferred`，wrapper 已恢复/保留 `need_resched`；trap tail 必须执行 `dispose_deferred_tasks()`，不得继续把本轮 wait park。
- `sched::higher_level::schedule_wait_with_timeout()`：必须先用 wait token 证明当前 round 是否仍 active。若 wait 已经完成且 current 已回到 `Runnable`，直接 no-park / abort-sleep 返回；若 wait 仍 active，timeout callback 必须在 `schedule_wait_sleep()` 前安装。
- `Latch::schedule_with_timeout()`：继续只通过 `schedule_wait_with_timeout()` 间接 schedule，不直接接触 scheduler-private mode。
- `Event::listen*()`：listener 注册和 predicate/signal recheck 完成后调用 token-bound / permit-bound `schedule_wait_sleep()`；`listen_with_timeout()` 必须通过同一个 finite-timeout proof 点区分 timer-installed park 与 already-completed no-park。现有 futex wait 作为 `Event::listen*()` 的 user-visible caller 必须纳入 proof。
- `yield_now()`：只允许在 `Runnable` current task 上调用 `schedule_runnable()`。
- idle loop：只允许 `Runnable` idle task 进入 runnable / idle wrapper。
- exit no-return tail：只能在 task / thread-group cleanup 完成后进入 `schedule_zombie_never_return()`；最终 `Zombie` sched-state 由 scheduler wrapper 发布，不能由 exit 模块先写入后再调用 schedule。

write set：

- `anemone-kernel/src/arch/riscv64/exception/trap/utrap.rs`
- `anemone-kernel/src/arch/riscv64/exception/trap/ktrap.rs`
- `anemone-kernel/src/arch/loongarch64/exception/trap/utrap.rs`
- `anemone-kernel/src/arch/loongarch64/exception/trap/ktrap.rs`
- `anemone-kernel/src/sched/mod.rs`
- `anemone-kernel/src/sched/event.rs`
- `anemone-kernel/src/sched/latch.rs`
- `anemone-kernel/src/sched/class/idle.rs`
- `anemone-kernel/src/task/api/exit/mod.rs`
- 如需为 source-backed finite timeout wait 记录 source register outcome 或 caller proof，允许触碰 `anemone-kernel/src/fs/api/iomux/wait.rs`；不得把 scheduler-private mode 暴露给 iomux source。

验证：

```sh
rg -n "schedule\(\)" anemone-kernel/src
rg -n "fetch_clear_need_resched" anemone-kernel/src/arch
rg -n "schedule_wait_with_timeout|listen_with_timeout|WaitOutcome::Armed|PollRegisterResult::Armed" anemone-kernel/src/sched anemone-kernel/src/fs
rg -n "\.listen\(|\.listen_with_timeout\(" anemone-kernel/src
```

退出条件：

- 裸 `schedule()` caller 要么消失，要么全部位于 scheduler-private `schedule_inner` 内。
- trap preempt caller 不再能调用 wait-sleep schedule。
- wait sleep caller 不会误用 preempt schedule。
- `Deferred` 后 trap tail 运行 deferred-task disposal；`Scheduled` 路径仍由 scheduler loop tail 处理 disposal。
- `Event::listen_with_timeout()`、source-backed finite timeout latch、no-source timeout 和 `wait_current_with_timeout()` 都能归入同一个 token-bound finite-timeout proof：wait 已完成则 no-park / abort-sleep 返回，wait 仍 active 则先完成 timeout callback 安装再进入 explicit wait sleep。
- `Event::listen*()` caller 清单中 futex wait 已被归入 token-bound wait-sleep proof；不能因为 futex PI 是非目标而漏掉当前 futex wait path。
- `WaitStateStatus::Armed`、`WaitOutcome::Armed`、`WakeToken::is_armed()` 和 `PollRegisterResult::Armed` 没有被当作 timeout-installed 或 whole-round park-ready 证明。

## 阶段 3：验证与反馈路由

前置条件：

- 阶段 0 到 2 完成，且没有 open scheduler/wait-core Keter blocker。

执行反馈：

- 2026-07-06 阶段 3 收口采用 source-review first：不再为本阶段强行增加新的 debug instrumentation。阶段 3 可以用独立代码审查、用户侧 `kernel_preempt` 下 iozone throughput 复核、以及 transaction devlog 中明确记录的未跑 trace 边界关闭 scheduler/wait-core correctness gate。
- 这只改变阶段 3 的验证执行方式，不改变 accepted contract：`Waiting/PrePark` 仍不能被 involuntary preempt park，wait identity / completion / placement 仍由 wait core 拥有，post-begin nested wait 仍由 core 诊断暴露并路由到 source owner。
- 未运行的定向 preempt-window trace、Event timeout trace、source-backed finite-timeout iomux trace 和 deferred-count fairness trace 不得在收口时写成已通过。若后续 workload 或 trace 显示 `PrePark` setup 长时间反复 deferred，仍必须回到 RFC review 评估 publish split / park permit 或等价设计。

阶段 3 消费前序阶段落下的最低可观测性：

- wait id。
- begin caller location。
- schedule entry / private mode。
- begin-to-explicit-sleep elapsed / source-scan outcome。
- deferred preempt count 或 trace 点。
- `PrePark -> Parked` 的 schedule entry。
- timeout-installed point。
- source-registered count 或 register scan outcome。
- finish outcome。
- wake reason 和 placement 结果。
- nested active wait panic 中的 existing wait caller location 与 new begin / sleep-attempt caller location。

建议验证：

- 源码 gate：

```sh
rg -n "schedule\(\)" anemone-kernel/src
rg -n "ScheduleMode|SchedulePreemptResult|schedule_preempt|schedule_wait_sleep|schedule_runnable" anemone-kernel/src
rg -n "Latch::begin_current" anemone-kernel/src
rg -n "WaitStateStatus::Armed|WaitOutcome::Armed|WakeToken::is_armed|PollRegisterResult::Armed" anemone-kernel/src
```

- 构建 gate：按仓库 build workflow 运行最小内核构建。
- 定向 trace：构造或插桩覆盖 begin 后、timeout/source installed 前触发 `need_resched` 的路径，确认 preempt entry 返回 deferred，且随后 explicit wait-sleep entry 才能 park。
- finite timeout trace：至少覆盖一个 `Event::listen_with_timeout()` 和一个 source-backed finite-timeout iomux path，能区分 timer-installed park 与 early source-completed no-park。
- preempt-defer fairness trace：至少在定向 preempt-window 复现中记录 begin-to-explicit-sleep 窗口和 deferred count；若出现长窗口或反复 deferred，不能把本 RFC 视为公平性闭合。
- 回归 gate：`kernel_preempt` 开启下复核 iozone throughput 触发路径。
- 调用面 gate：clock sleep、signal wait、futex wait、no-source iomux timeout、source-backed iomux、eventfd/timerfd/fanotify blocking wait 至少有源码 proof；风险高时补 smoke。
- nested wait gate：二次 begin 应触发可定位 release assert；`Mutex::lock()` post-begin slow path 应触发 sleep-attempt 诊断。若路径归属 fanotify 或其它 source owner，记录为对应 owner follow-up；若路径归属 wait-core/scheduler 新增代码，则回到本 RFC 修正。

退出条件：

- begin-to-park-ready preempt window 有可复审 trace 或定向测试证明。
- finite timeout wait 有可复审 trace 或字段级 proof，确认 timer-installed park 与 already-completed no-park 不会互相误判。
- post-begin setup boundedness 有可复审源码 proof 或 trace；如果需要任意长 source scan 才能到 explicit wait sleep，回到 publish split / park permit 设计。
- iozone throughput 不再出现 wait begin 后无 timeout/source/finish 的 stuck wait。
- single-active-wait 违规不再静默破坏状态，且 panic 可以定位 existing/new caller location。
- fanotify/source-owner nested wait 触发时已有反馈路由，不阻塞本 RFC 的 scheduler/wait-core closeout。
- RFC / transaction devlog / register 更新边界明确；公开文档不依赖 `etc/` 草案路径作为 canonical source。

## Post-close correction gate：Pending Resched Pick Acknowledgement

**触发：** typed `PendingResched` 落地后，trap tail 与 idle loop 会通过 `take_pending_resched()` 显式消费 slot，但 block、yield、zombie 和其它直接进入 `switch_out()` 的路径不会先清 slot。修正前的 `local_pick_next()` 完成 full pick 后也没有统一 acknowledgement，旧 `Tick` / `RunnableArrival` 因而可能跨已完成的重新选择继续滞留。

**Hypothesis:** `PendingResched` 是请求 owner CPU 重新选择的合并 latch；只要 scheduler loop 成功完成一次 `pick_next_task()`，此前 cause 已被满足，不论最终是否换成不同 task。没有 pick 的 schedule path 不满足请求，必须保留或恢复。

**Protected Goal / Invariant:** 保持 caller-owned deferred restore、wait no-switch abort、scheduler-class preempt transaction snapshot 和 owner-CPU runqueue selection 边界不变。pick 后新产生的 cause 必须属于下一轮，不能被旧事务尾部清除。

**Minimum Write Set:**

- `anemone-kernel/src/sched/processor.rs`：在 `local_pick_next()` 的 owner 边界直接清 processor slot。
- 本 RFC 的 `index.md`、`invariants.md`、`implementation.md`、`tracking-issues.md`，既有 transaction devlog 与当前双周 devlog。

**Non-goals:**

- 不修改 EEVDF / RR class policy 或 `PendingResched` cause 集合。
- 不修改 trap / idle 的 destructive take 和 caller-owned restore 协议。
- 不引入 epoch、token、序列号、事件队列或跨 context-switch cause history。
- 不把 acknowledgement 分散到 yield / block / zombie / preempt wrapper，也不让 task-owned state 保存 processor pending truth。
- 不为单次字段赋值增加具名 clear / acknowledgement helper，也不增加只验证该赋值本身的同义反复测试。

**实现形状：**

- `local_pick_next()` 在 `RunQueue::pick_next_task()` 成功返回后直接把 `proc.pending_resched` 置空，再执行 `set_next_task(task, now)`；该顺序让 selection 确认此前请求，同时把 clear 限定在 next-task switch-in transaction 之前。
- `DeferredPreempt` 与 wait no-switch abort 在 `switch_out()` 前返回，不进入 scheduler loop，因此不执行此 clear。执行 `take_pending_resched()` 的 trap caller 仍只在 deferred 时 union restore snapshot。
- full pick 即使得到同一 task 也完成 acknowledgement；当前调度事务已经按值捕获的 `PendingResched` snapshot 不受 processor slot 清除影响。

**Validation Floor:**

- Source audit：确认 `pending_resched = PendingResched::empty()` 紧跟 production `pick_next_task()`，枚举 `take_pending_resched()` / `restore_pending_resched()` 的全部 caller；确认两个 no-switch return 不调用 `switch_out()` / `local_pick_next()`，所有真实 switch-out 最终只经 scheduler loop full pick。
- 不用抽出的 helper 或 test-only wrapper 制造定向单元测试；当前直接赋值的正确性证据是 owner-boundary placement audit。构建与启动 smoke 负责发现编译、链接和启动回归。
- `just fmt kernel --check`、`just build`、`git diff --check`、`mdbook build docs`。formatter 必须不再报告本 gate 的 `processor.rs`；若仅剩仓库生成文件 drift，记录精确文件与边界，不扩大 write set 修改生成物。

**Failure Signals:** acknowledgement 必须移出 `Processor` owner；某条 no-pick path 仍进入 `local_pick_next()`；pick 与 acknowledgement 之间允许异步插入并被误删的新 cause；或 class 必须依赖旧 processor slot 而不是 call-local snapshot。出现任一情况即停止代码 gate，回到 RFC review，不用新事件协议掩盖边界错误。

**Write-back:** accepted lifecycle 写回 `index.md` / `invariants.md`；gate 形状写在本节；执行事实与验证写入既有 transaction devlog；`KETER-008` 在验证完成后移入 Neutralized。若修复后没有残余缺口，不新增 register / current limitation。

**Exit:** processor pick-time clear、source audit 和 validation floor 均有可复核结果；没有为直接赋值保留过度封装或同义反复测试；本 gate 源码格式通过，任何生成文件限定的全仓 formatter drift 已在 transaction 中单独记录；tracking issue neutralized，transaction 与双周 devlog 记录 agent-run / unrun 边界。

## 旁路审计清单

- 直接调用裸 `schedule()` 并可能消费 wait-core state 的路径。
- 直接写 `TaskSchedState` 或重新引入 `TaskStatus` 写侧真相的路径。
- 绕过 `schedule_wait_with_timeout()` 的 timeout sleep helper。
- source 侧直接 wake/enqueue 或保存 task 强引用的路径。
- `WakeToken::is_armed()`、`WaitStateStatus::Armed` 或 `WaitOutcome::Armed` 被当成 timeout/source installed 或 park-ready 证明的路径。
- `PollRegisterResult::Armed` 被越界解释成 wait-core completion-open 或 whole-round park-ready，而不是 source trigger registered 的路径。
- `task_enqueue()` / `local_enqueue()` / `remote_enqueue()` 被用于 wait-tail placement 的路径。

## 停止边界

- 如果 implementation feedback 需要缩小目标、降低验证 floor、弱化不变量、隐藏失败路径或接受更弱语义，停止并回到 RFC 文档层。
- 如果 `schedule_preempt()` 无法在 deferred 后保留/恢复 resched 请求，停止并回到 RFC 文档层。
- 如果某个 schedule caller 无法迁移到 entry split wrapper，停止并补设计。
- 如果实现需要改变 wait identity、completion 线性化点、task sched-state owner、source register contract 或 signal delivery contract，停止并回到 RFC 文档层。
- 如果修复只能解释 no-source timeout，但不能解释 direct wait helper 或 source-backed `Latch::begin_current()` direct users，不能声明 contract 关闭。
- 如果需要扩大 write set，先写扩展申请；不得在旧 write set 内绕路制造长期 compatibility layer。
- 如果 fanotify 等 source owner 的 nested wait panic 被误用来要求 wait-core 支持 nested active wait，停止并回到 RFC review。

## 实现期反馈记录

- 2026-07-06：阶段 1 implementation worker 命中 write-set stop condition。token-bound `schedule_wait_sleep()` 需要行为级 current-wait identity proof；用户批准把 `sched/wait.rs` 纳入最小扩展写集，并禁止使用 diagnostic id 或 `is_armed()` 作为行为依据。目标、不变量、状态所有权和 completion 线性化点不变；执行事实见 transaction devlog。
- 2026-07-06：用户批准阶段 1 吸收原阶段 2 的 schedule-entry call-site 迁移子集，避免留下裸 `schedule()` 兼容桥。该反馈改变阶段 write set / 执行顺序，但不改变 accepted contract、验证 floor 或后续 trace/runtime gate；执行事实见 transaction devlog。
- 2026-07-08：`kernel_preempt` 单核试运行暴露 zombie exit 尾部窗口：exit 模块提前发布 `Zombie` 后、`schedule_zombie_never_return()` 前可能被 trap-tail involuntary preempt 打断。该反馈改变 zombie wrapper 的 accepted contract：`Runnable -> Zombie` 发布归 scheduler core 的 noirq no-return entry，`schedule_preempt()` 仍不接受 zombie current；执行事实见 transaction devlog。
- 2026-07-12：post-close source audit 发现 processor pending-resched 缺少 successful-pick acknowledgement；block / yield 等未执行 destructive take 的路径完成 full pick 后仍可能留下旧 cause。该反馈补充 scheduler-core latch 生命周期：full pick 确认，no-pick 保留或恢复，pick 后新 cause 进入下一轮；不引入事件协议。执行 gate 与证据见 transaction devlog。

## Write Set 扩展记录

- 2026-07-06：阶段 1 新增 `anemone-kernel/src/sched/wait.rs`，范围限定为 scheduler-private wait identity check / private permit。
- 2026-07-06：阶段 1 提前纳入原阶段 2 的 schedule-entry call sites：arch trap preempt、`sched/event.rs` explicit wait sleep、`sched/class/idle.rs` idle schedule、`task/api/exit/mod.rs` zombie no-return。未批准 fs source owner、iomux source register、signal delivery 或 source-local parkability 状态变更。

## 结构维护记录

- None。
