# Sched Wait Preempt Arming Tracking Issues

**状态：** Closed
**最后更新：** 2026-07-14
**父 RFC：** [RFC-20260618-sched-wait-preempt-arming](./index.md)
**事务日志：** [2026-07-06-sched-wait-preempt-arming](../../devlog/transactions/2026-07-06-sched-wait-preempt-arming.md)

本文只跟踪当前仍影响方案选择、review gate、停止边界或验收判断的 RFC 层问题。已被正文接受的问题陈述、单纯 implementation pending、已 neutralized 的备选方案和纯命名延期不在这里重复记录；若本 RFC 进入实现，应建立 transaction devlog。

2026-07-14 shared contract 更正：下方关于 Tick / RunnableArrival cause 集合或 class 读取 pending snapshot 的历史问题记录保持原状态与证据，但该接口已由 [Sched RT Class R1](../sched-rt-class/index.md) supersede。当前 wait-preempt contract 只保留 core take、caller-owned union restore 和 successful full-pick acknowledgement；pending 是 core-only single bit，不进入 class transaction。

阶段 3 反馈对 tracker 做过一次收口：`KETER-004`、`KETER-005` 和 `KETER-006` 已由 source review、用户侧 iozone 复核和明确的 trace 未运行边界路由，不再作为 open scheduler/wait-core blocker。2026-07-12 post-close correction 已把 `KETER-008` 的 processor pending-resched lifecycle 收口到 successful full pick，并通过 `KETER-009` 删除不可达的 aborted-wait requeue surface。未运行的定向 trace 与 deferred-count fairness trace 仍是 residual evidence gap；后续若 workload 或 trace 显示 `PrePark` setup 长时间反复 deferred，必须回到 RFC review。

状态后缀说明：

- Implementation feedback gate：允许带入实现阶段，但必须有受保护目标、验证方式、失败信号、停止条件和 RFC 回写路径；反馈只能优化路线，不能削弱目标或不变量。
- Caller-source feedback gate：允许通过真实 source owner / lock 路径决定修复归属；如果需要改变 owner boundary、source register contract 或 wait-core 不变量，停止并回到 RFC review。

## Keter

None。当前没有仍会阻塞 scheduler/wait-core implementation closeout 的 Apollyon / Keter；剩余 trace 与 workload 证据缺口已记录为 residual risk 和后续重开条件。

## Euclid

None。当前剩余 Euclid 已折入 implementation gate 或阶段验证清单，不作为独立 open tracker 保留。

## Neutralized

### KETER-009：wait abort 不能保留不可达的第三条 class transaction

**状态：** Neutralized / Post-close correction / 2026-07-12

**问题：** scheduler class surface 曾保留 `requeue_aborted_wait_current()`，假设 wait park 后可能存在一条既非 no-switch abort、也非 parked handoff 的“无 wake reward requeue”路径。真实 `schedule_inner()` 状态机没有该 transition：already-completed wait 直接返回并保持 current，park 后完成只走 handoff。保留无 caller 的第三条 transaction 会把不存在的 scheduler-core 状态迁移固化进所有 class，并让后续 EEVDF / RT 设计错误消费它。

**处理：** 删除 processor facade、`RunQueue` transaction variant、`Scheduler` trait method 和 EEVDF / RR / Idle 实现。EEVDF RFC 的当前 contract、invariant、R2 gate 与 source-audit 清单同步只保留 no-switch abort 和 parked handoff；历史 transaction 记录不改写，由 2026-07-12 correction 条目 supersede。未修改 `schedule_inner()`、wait-core state、wake placement、pending-resched 或任何 reachable scheduling behavior。

**证据：** source audit 证明 `AbortWaitSleep` 在 `switch_out()` 前返回 `DidNotSwitch`，`WaitCoreParked` 复查到 `Runnable` 时调用 `local_handoff_woken_current()`，全树 production source 不存在 `requeue_aborted_wait_current` / `AbortedWait` 代码符号。`just build`、`mdbook build docs` 与 `git diff --check` 通过；`just fmt kernel --check` 只报告未触碰的 generated `kconfig_defs.rs` / `platform_defs.rs` whitespace drift，未命中本次 scheduler 源文件。未运行 QEMU / LTP；详细 host-build 前置边界见同日 scheduler-core transaction closure。

### KETER-008：processor pending-resched 必须由 successful full pick 统一确认

**状态：** Neutralized / Post-close correction / 2026-07-12

**问题：** `take_pending_resched()` 只由 trap tail 与 idle loop 显式调用；block、yield、zombie 等路径可以直接 switch out 并完成下一任务选择，却不会清 processor slot。旧 `Tick` / `RunnableArrival` 因而可能跨一次已经完成的 runqueue 重新选择继续滞留，之后被误当成新一轮 preempt trigger。

**处理：** `Processor` 现在把 pending slot 明确定义为面向下一次 owner-CPU full pick 的合并 latch；`local_pick_next()` 在 `pick_next_task()` 成功后、`set_next_task()` 前直接把 slot 置空，并用就地注释解释 full-pick satisfaction、IRQ serialization 和后续请求保留。full pick 即使重新选中同一 task 也确认；`DeferredPreempt` 与 wait no-switch abort 不到达该点，caller-owned restore 与 call-local preempt snapshot 保持不变。未引入具名 clear helper、同义反复 KUnit、epoch、token、事件队列或 task-owned cause history。

**证据：** source audit 确认 production clear 只有 `local_pick_next()` 中紧跟 `pick_next_task()` 的直接赋值；`switch_out()` 只有 `schedule_inner()` 一个 caller，两个 no-switch return 都位于 `switch_out()` 前，四个 trap tail 仍在 deferred 时 restore。当前代码的 `just build` 通过；有时限的 rv64 pretest 启动完成 113 项既有 KUnit 并打印 `All tests passed!`，随后进入 init / user-test，主机 timeout 只用于终止超出本 gate 的 signal LTP。`git diff --check` 与 `mdbook build docs` 通过；`just fmt kernel --check` 只被既有生成文件 `kconfig_defs.rs` / `platform_defs.rs` whitespace drift 阻塞，输出不包含本次 `processor.rs`。

### KETER-007：zombie sched-state 发布不能先于 no-return scheduler entry 暴露

**状态：** Neutralized / Implementation feedback folded back / 2026-07-08

**问题：** 阶段 1 迁移把 zombie exit 接到 `schedule_zombie_never_return()`，但旧形状仍由 exit 模块先写 `TaskSchedState::Zombie`，随后才进入 no-return schedule。单核 `kernel_preempt` 试运行证明这两个动作之间存在 trap-tail preempt 窗口：`schedule_preempt()` 可观察到 zombie current，并正确触发 release invariant panic。

**处理：** 已折回 canonical 文本和实现计划：exit 模块只负责完成 task / thread-group cleanup；最终 `Runnable -> Zombie` 发布由 scheduler owner 在 `ScheduleMode::Zombie` 的 noirq no-return 事务内完成，并立即切走。`schedule_preempt()` 仍不接受 zombie current；若未来再次出现 `schedule_preempt cannot preempt zombie current task`，应视为某处提前发布 `Zombie` 或重复 zombie entry 的 owner-boundary 破坏，而不是放宽 preempt 入口语义。

### KETER-004：post-begin nested wait 必须诊断暴露，但 source owner 修复不默认属于本 RFC

**状态：** Neutralized / Routed / 2026-07-06

**问题：** schedule entry split / preempt-defer 只覆盖 involuntary preempt。若 `Latch::begin_current()` 发布当前 task 的 active wait 后，source register scan 或 direct source wait 再进入普通 sleepable lock 的慢路径，它可能通过 `Event::listen_uninterruptible()` 创建第二个 `ActiveWait`。这不是抢占入口误 park，而是 caller/source owner 边界错误。

**处理：** core 侧诊断已经落地：`begin_wait()` 的 nested active wait assert 报告 existing/new caller location，`assert_current_not_in_active_wait()` 报告 existing begin 与 sleep-attempt caller location，`Mutex::lock()` 在 `allow_preempt()` 诊断被遮蔽前和 slow path 进入 `Event::listen_uninterruptible()` 前都会检查当前 task 是否已有 active wait。阶段 3 source review 确认 fanotify `poll()` / blocking read 仍经过普通 `Mutex<FanGroupState>`；若该路径真实触发诊断，按 fanotify/source owner follow-up 记录，不在本 RFC 内用 source-specific workaround 或 nested wait 支持静默绕过。若 wait-core/scheduler 新增路径触发同类诊断，则重新打开本 RFC blocker。

### KETER-005：finite timeout wrapper 的 explicit-sleep 前 proof

**状态：** Neutralized / Source proof accepted / Trace not run / 2026-07-06

**问题：** `Event::listen_with_timeout()` 与 source-backed iomux finite timeout 都可能先 publish wait identity，再到 `schedule_wait_with_timeout()` 内部安装 timer callback；source trigger 已注册不等于 finite timeout prerequisite 已安装。

**处理：** 阶段 3 source review 确认 `schedule_wait_with_timeout()` 是统一 finite-timeout proof 点：已完成 token 走 `no-park before timeout install` / abort-sleep，不安装 stale timeout；active wait 先安装 timeout callback，再调用 token-bound `schedule_wait_sleep()`；`Event::listen_with_timeout()`、`Latch::schedule_with_timeout()`、no-source timeout 和 `wait_current_with_timeout()` 都汇入该 proof。`WakeToken::is_armed()` 只作为 completion-open 检查，行为 identity 由 `Arc::ptr_eq` / `matches_wait_state()` 证明。未运行 Event timeout trace 或 source-backed finite-timeout iomux trace，因此不声明 trace gate 已通过；若后续证据显示 timer-installed park 与 already-completed no-park 误判，必须回到 RFC review。

### KETER-006：`PrePark` setup 窗口不能变成无界公平性盲区

**状态：** Neutralized for correctness / Fairness trace not run / 2026-07-06

**问题：** preempt-defer 能防止 not-park-ready wait 被 involuntary preempt park，但不能自动证明任意 post-begin setup 窗口的调度公平性。

**处理：** 阶段 3 source review 确认 correctness gate 仍闭合：`schedule_preempt()` 对 `Waiting/PrePark` 只 deferred 并恢复 `need_resched`，不 park、不 requeue、不 switch；explicit wait sleep 仍由 token-bound wrapper 消费 `PrePark`。post-begin source-backed iomux register scan 的规模被 syscall 入口限制到 `MAX_FD_PER_PROCESS` / `FD_SETSIZE`，当前均为 1024；pipe/eventfd/timerfd source register 使用短临界区保存 trigger，unsupported source fail closed。未运行 begin-to-explicit-sleep elapsed / deferred-count trace，因此不声明 fairness trace 闭合；若后续 workload 或定向 trace 显示长窗口或反复 deferred，必须回到 publish split / park permit 或等价设计，而不是扩大 preempt-defer 语义。

### KETER-001：方案不能只落在 iomux 或空 no-source path

**状态：** Neutralized / 2026-07-06

**问题：** 当前触发样本来自 `select(0, ..., 1us)`，但 begin-before-park-ready 的时序不只存在于 no-source path。所有 `Latch::begin_current()` direct users（当前至少包括 `fs/api/iomux/wait.rs` 两处、`fs/eventfd.rs` 两处、`fs/fanotify/group.rs` 一处、`fs/timerfd.rs` 一处）、`Event::listen*()` users（当前至少包括 futex wait）、以及 `wait_current_with_timeout()` precheck 到 `schedule_wait_with_timeout()` 都需要纳入同一 contract。

**处理：** 阶段 0 已生成受影响调用面清单，覆盖 `Latch::begin_current()` direct users、`Event::listen*()` users、direct wait helper、finite timeout helper、clock sleep 和 signal wait。阶段 1 实现没有收窄为 no-source / iomux 局部修复：裸 `schedule()` caller 已清零，trap preempt、Event direct sleep、finite-timeout helper、yield、idle 和 zombie exit 都迁移到语义化 scheduler wrapper；`Event::listen_with_timeout()`、source-backed latch、no-source timeout 和 `wait_current_with_timeout()` 都汇入 token-bound `schedule_wait_with_timeout()` / `schedule_wait_sleep()` proof 点。后续 KETER-004/005/006 仍跟踪 source-owner nested wait、finite-timeout trace 和 `PrePark` boundedness，但“方案只落在 iomux 或空 no-source path”的方向性风险已经被当前 implementation neutralized。

### KETER-002：schedule entry split 必须精确到 park 权限

**状态：** Neutralized / 2026-07-05

**问题：** 当前 `schedule()` 不区分 explicit wait sleep、ordinary runnable reschedule / idle / zombie no-return 与 involuntary preempt。若未来只用宽泛 `Voluntary` vs `Preempt`，非 wait-sleep caller 仍可能获得消费 `Waiting/PrePark -> Parked` 的权限。

**处理：** 已折回 canonical 文本：`index.md` 接受 schedule entry split / preempt-defer，并拒绝公开 `ScheduleCaller` taxonomy；`invariants.md` 明确 scheduler owner 外不得直接调用裸 `schedule()`，只有 token-bound / permit-bound `schedule_wait_sleep()` 可以消费 `Waiting/PrePark`；`implementation.md` 阶段 1 / 阶段 2 已列出 scheduler-private `ScheduleMode`、wrapper 迁移、preempt deferred 返回和裸 caller 清理 gate。后续 wrapper 命名和私有 mode 细节属于 implementation feedback，但 park 权限边界不能削弱。

### KETER-003：parkability truth 不能变成第二套状态所有权

**状态：** Neutralized / 2026-07-05

**问题：** 如果 `Latch`、iomux source、timer 或 signal 各自保存 wake-prerequisite / park-ready correctness truth，会破坏 wait-core 单一状态所有权。

**处理：** 已折回 canonical 文本：`index.md` 和 `invariants.md` 明确 wait identity、completion outcome、cancel、finish、physical placement 和 parkability truth 仍由 wait core / task sched-state 拥有；scheduler-private mode 只能作为 scheduler owner 内部状态机输入，不能写入 `WaitState`、不能暴露给 source、不能成为 producer trigger 能力。后续实现反馈只能发现调用点或 source 需要修正，不能把 parkability truth 迁移到 `Latch`、fd source、timer callback 或 signal subsystem。

### EUCLID-001：需要补齐 park-ready window 可观测性

**状态：** Neutralized / 2026-07-05

**问题：** 当前日志能看到 begin 后被 park，但不一定能稳定重建 begin、timeout-installed、source-registered、park-ready、schedule entry、timeout id、source count 和 finish outcome 的完整因果链。

**处理：** 已折入 `implementation.md` 阶段 3 的最低可观测性和 trace gate：wait id、begin caller location、schedule entry / private mode、deferred preempt count、`PrePark -> Parked` entry、timeout-installed point、source-registered count / register outcome、finish outcome、wake reason / placement，以及 nested active wait panic 的 existing/new caller location。该项不再单独作为 Euclid open issue。

### EUCLID-002：调用面清单需要字段级分类

**状态：** Neutralized / 2026-07-05

**问题：** KETER-001 要求全量调用面清单，但仅列出文件不足以支撑实现判断。每个 direct user 都需要说明 begin point、predicate recheck、source registration 或 timeout install、lock/sleepability boundary、post-begin nested-wait risk、first possible explicit sleep entry、finish/cancel path，以及当前方案是否覆盖。

**处理：** 已并入 `KETER-001` 和 `implementation.md` 阶段 0：阶段 0 明确要求生成 `Latch::begin_current()` direct users、direct wait helper、post-begin 可睡眠路径清单，并逐项记录 begin point、predicate recheck、source registration / timeout install、lock / sleepability boundary、nested-wait risk、first explicit wait-sleep entry、finish / cancel path 和覆盖结论。该项不再作为独立 tracker 重复。

### EUCLID-003：preempt-deferred 后 deferred-task disposal 责任需要说明

**状态：** Neutralized / 2026-07-05

**问题：** 当前 trap return 路径在 `fetch_clear_need_resched()` 为真时调用 `schedule()`，并跳过 `dispose_deferred_tasks()`；正常 context switch 后 scheduler loop 还能处理 deferred tasks。若新 `schedule_preempt()` 在 switch 前返回 `Deferred`，既必须恢复 `need_resched`，也不会进入 scheduler loop，因此 deferred-task disposal 的责任点需要明确。

**处理：** 已折入 `implementation.md` 阶段 1 / 阶段 2：`schedule_preempt()` 的 `Deferred` 路径必须在 `switch_out()` 前返回并恢复 / 保留 resched request；arch trap tail 在 `Deferred` 后执行 no-schedule 分支的 deferred-task disposal；真实 scheduled 路径仍由 scheduler loop tail 处理 disposal。具体落点可由 wrapper / trap-tail 代码形状反馈，但不能吞掉 resched request 或长期跳过 deferred disposal。

### EUCLID-004：armed 术语 gate 需要按上下文细化

**状态：** Neutralized / 2026-06-18

**问题：** canonical text 已把 wait-core `Armed` 定义为 completion-open，但实现 gate 只搜索 `WaitStateStatus::Armed|WakeToken::is_armed`，漏掉 `WaitOutcome::Armed`；同时 `PollRegisterResult::Armed` 在 sched-latch 语义里合法表示 source trigger 已注册，不能被 blanket ban。

**处理：** 已折回 canonical 文本：`index.md` 明确 wait-core `WaitStateStatus::Armed` / `WaitOutcome::Armed` / `WakeToken::is_armed()` 只表示 completion-open，`PollRegisterResult::Armed` 只表示 source trigger registered；`invariants.md` 明确 `PollRegisterResult::Armed` 不是 wait-core Armed，也不是 whole-round park-ready 证明；`implementation.md` 的搜索 gate 已加入 `WaitOutcome::Armed|PollRegisterResult::Armed`，并要求按上下文审查。
