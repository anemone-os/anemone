# RFC-20260618-sched-wait-preempt-arming

**状态：** Closed
**负责人：** doruche, Codex
**最后更新：** 2026-07-06
**领域：** scheduler / wait core / kernel preempt / latch / iomux / timer / signal
**事务日志：** [2026-07-06-sched-wait-preempt-arming](../../devlog/transactions/2026-07-06-sched-wait-preempt-arming.md)
**开放问题：** None；未运行的 trace / fairness evidence gap 见 [Tracking Issues](./tracking-issues.md) 与事务日志。
**下一步：** 后续只在 source-owner nested wait 真实触发、finite-timeout trace 发现误判，或 deferred-count / workload 显示 `PrePark` setup 公平性风险时重开 RFC review。

## 摘要

本文是 `sched-wait-refactor` 和 `sched-latch` 之后的公开 follow-up RFC。它记录一个新的 wait-core/preemption contract 缺口：当前等待方可以先把 task 发布为 completion-open 的 wait identity / `TaskSchedState::Waiting { park: PrePark }`，再安装 timeout callback 或注册 source trigger；如果这段窗口里发生内核抢占，trap return 侧的 involuntary `schedule()` 可能把尚未 park-ready 的 wait round 直接消费成 `Parked`，而本轮返回语义所需的 wake prerequisites 还没有安装完成。

本文选择第一阶段修复方向：采用 Linux-style schedule entry split。外部调用点不直接传公开 `ScheduleCaller` taxonomy，而是调用语义化 scheduler wrapper；scheduler owner 内部可以使用私有窄 `ScheduleMode` 驱动 `schedule_inner()` 状态机。显式等待睡眠入口仍可以在本轮 wake prerequisites 已安装或已分类后消费 wait 的 park intent，把 `Waiting/PrePark` 推进为 `Waiting/Parked`；如果本轮 wait 已在 explicit sleep 前完成，则必须走 no-park / abort-sleep 返回，不能把它当作普通 runnable yield。trap return 等抢占入口不能消费 not-park-ready wait round。若抢占入口观察到当前 task 处于 `Waiting/PrePark`，它必须 defer 本次抢占，不 park、不把 waiting task 重新入队、不 context switch，并在返回前恢复或保留 resched 请求。

同时，本文把责任边界拆成两条线：

- wait-core / scheduler 负责 wait identity、completion、cancel、finish、physical placement，以及“哪个 schedule entry 有权消费 `PrePark`”的 park 权限边界。
- caller / source 路径负责遵守 single-active-wait：`ActiveWait::begin()` / `Latch::begin_current()` 发布 active wait 后，到 finish / cancel / retire 前，同一 task 绝不能进入第二个 scheduler wait。wait-core 必须用 release assert / 诊断暴露这类违规；第一阶段 wait origin 只记录创建 caller location，不引入 primitive / operation taxonomy；具体 source owner 的修复不自动落入本 RFC 的 write set。

## 背景

现象来自 `iozone` throughput 路径：打开 `kernel_preempt` 后，`select(0, ..., timeout=1us)` 高频进入 no-source iomux wait，某些 task 只留下 wait-core begin 记录，随后被调度器视为 wait-core parked；同一 wait id 没有对应 latch begin、timeout wake 或 finish。关闭内核抢占后，同一路径可以正常通过。

当前相关路径的形状是：

- `ActiveWait::begin()` / `begin_wait()` 会把当前 task 发布为 `TaskSchedState::Waiting { park: PrePark }`。
- `Latch::begin_current()` 在 source register scan 和 timeout callback 安装之前调用 wait-core begin。
- `wait_current_with_timeout()` 这类 direct wait 用户也存在先 begin、后 precheck、再 schedule timeout 的窗口。
- trap return 侧在 `kernel_preempt && allow_preempt() && need_resched` 时可以直接调用 `schedule()`。
- `schedule()` 当前不区分 explicit wait sleep、runnable reschedule / idle schedule / zombie no-return 与 involuntary kernel preemption；看到 `Waiting/PrePark` 就可以转为 `Parked` 并不重新入队。
- 部分 source register / poll 路径可能经过普通 `Mutex`。如果该锁走慢路径，会通过 `Event::listen_uninterruptible()` 进入另一个 wait round；这不是 preempt 误 park，而是 caller/source 在 active wait 内部嵌套 scheduler wait。

术语约束：本文不使用 `armed` / `arming` 表示 timeout/source 已安装或 wait 可 park。当前代码里的 wait-core `WaitStateStatus::Armed` / `WaitOutcome::Armed` / `WakeToken::is_armed()` 只表示本轮 completion 仍开放；它不是 timeout callback 已安装、source trigger 已注册，也不是 task 已经 park-ready 的证明。`PollRegisterResult::Armed` 属于 iomux source register 术语，只表示某个 source 已接受并保存 trigger；它可以是 wake prerequisite 的一部分，但不能单独证明整个 wait round 已 park-ready。本文后续分别使用 completion-open、wake prerequisites installed 和 park-ready 描述这三层状态。

因此，这不是某个 fd source 私有 lost wake，也不只是空 iomux timeout 的局部问题。它触碰两个共享契约：一个已经发布给 task sched-state 的 wait round，在本轮必要 wake prerequisites 尚未安装完成、且 wait round 尚未 park-ready 前，是否允许被任意 schedule entry park；以及 caller 发布 active wait 后，是否仍允许进入会创建第二个 scheduler wait 的可睡眠路径。

## 目标

- 明确 wait-core 在内核抢占下的 wake-prerequisite / parkability 契约。
- 覆盖 `Latch::begin_current()` 直接用户（iomux、no-source timeout、eventfd、timerfd、fanotify 等）、`Event::listen*()` users（包括现有 futex wait）、`wait_current_with_timeout()` direct wait 用户、signal wait 和 clock sleep 等共享等待路径。
- 用 schedule entry split 区分 explicit wait sleep 与 involuntary preempt 对 wait round 的权限。
- 将 single-active-wait 变成 release assert / 可定位诊断：begin 后第二次 wait 必须暴露已有 wait caller 和新 begin caller。
- 约束修复不得依赖 source-specific workaround，也不得把 sleepable source scan 粗暴放进关抢占区域。
- 保留足够可观测性，让后续 iozone 和定向 preempt-window 验证能证明缺口关闭。

## 非目标

- 不重写调度策略、时间片、负载均衡或抢占触发策略。
- 不重新设计 `Latch` 的 OR wait source 注册协议。
- 不把 `Event`、`Latch`、timer 或 signal 各自扩展出一套独立 park 状态。
- 不用 busy polling、延长 timeout 或关闭 `kernel_preempt` 作为语义修复。
- 不把 `PreemptGuard` 或等价 guard 跨过真正的 context switch。
- 不让 wait-core 支持同一 task 的 nested active waits。
- 不在本 RFC 内修复 fanotify 等 source owner 的 post-begin sleepability 问题；本 RFC 只负责暴露并路由这类违规。

## 文档地图

Canonical：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)
- [Tracking Issues](./tracking-issues.md)

相关历史：

- [Sched Wait Refactor RFC](../sched-wait-refactor/index.md)
- [Sched Latch RFC](../sched-latch/index.md)
- [空 iomux 超时睡眠小迭代记录](../../devlog/changes/2026-06-08-iomux-empty-timeout-sleep.md)

## 当前问题边界

本文只先接受以下问题陈述：

1. wait-core begin 之后、timeout callback 安装 / source trigger 注册之前存在一个可被 kernel preempt 打断的窗口。
2. 当前 `schedule()` 把 `Waiting/PrePark` 视为可 park 状态，不区分调用者是 explicit wait sleep 还是 involuntary preempt。
3. 如果本轮返回语义依赖尚未安装完成的 timeout/source wake prerequisites，提前 park 会让 task 没有对应 wake source，从而可能永久睡眠。
4. 有 fd source 的 iomux register scan、no-source timeout wait、eventfd / timerfd / fanotify 这类 `Latch::begin_current()` direct users，以及 direct timeout wait 用户都可能需要同一 wait-core 级 contract，不能只修 `iozone` 的 no-source 路径。
5. schedule entry split / preempt-defer 只覆盖 involuntary preempt 误 park；post-begin 阶段发生 ordinary blocking lock slow path 或其它 voluntary nested wait 时，必须由 caller/source owner 修正或作为对应 owner 的 follow-up 反馈，不能归类为 wait-core 应该兜底的场景。

## 已选方向：Schedule Entry Split + Private Scheduler Mode

本 RFC 采用类似 Linux `schedule()` / `preempt_schedule*()` 入口分流、内部 `__schedule(sched_mode)` 的方向，但保持 Anemone 当前 wait-core 单一状态所有权：外部调用点通过语义化 wrapper 表达意图，底层 scheduler-private `ScheduleMode` 只服务 `schedule_inner()` 状态机，不成为跨 subsystem contract。

第一阶段语义：

1. 裸 `schedule()` 必须私有化；scheduler owner 外只能调用语义化 wrapper。
2. `schedule_wait_sleep()` 是唯一正常消费 `Waiting/PrePark -> Waiting/Parked` 的入口；它必须是 token-bound 或 wait-core permit-bound 入口，先证明 caller 仍命名当前 wait round，且必须在本轮 wake prerequisites 已安装或 ready / registered / unsupported 分类完成后调用。不得暴露不携带 wait identity 的泛用 wait-sleep helper。
3. `schedule_preempt()` 只表达公平性抢占：`Runnable` 可以被换出；`Waiting/PrePark` 表示当前 task 正处于 wait setup 窗口，抢占入口必须返回 deferred 结果，不能 park、不能 requeue、不能 context switch；`Waiting/Parked` 出现在 current task 的抢占入口是强不变量异常。
4. `schedule_preempt()` 不得吞掉 `need_resched`。由于 trap 入口通常先 `fetch_clear_need_resched()`，preempt wrapper 必须在 deferred 返回前重新标记 resched 或证明请求仍被保留。
5. `schedule_preempt()` 返回 deferred 时没有发生 context switch；arch trap tail 必须执行原本 no-schedule 分支的 deferred-task disposal。真实 scheduled 路径仍由 scheduler loop tail 处理 disposal。
6. `schedule_runnable()` 或等价 wrapper 覆盖 yield / idle 这类 current 仍应为 `Runnable` 的 reschedule 路径；它不能消费 `Waiting/PrePark`。
7. `schedule_zombie_never_return()` 是单独 no-return wrapper；它不应和普通 runnable reschedule 共用“可返回”语义。
8. scheduler-private `ScheduleMode` 第一阶段只需要表达 `WaitSleep`、`Preempt`、`Runnable` 这类底层状态机差异；zombie no-return 可以由 wrapper 内部单独处理。该 mode 不写入 `WaitState`，不暴露给 `LatchTrigger`，不由 fs source 保存。
9. wait identity、completion outcome、cancel、finish 和 wake placement 仍由 wait core / `TaskSchedState` 统一管理；schedule mode 是 scheduler owner 内部输入，不是第二套 wait truth。
10. 第一阶段不引入通用机制阻止任意长 `PrePark` setup。`schedule_preempt()` deferred 只关闭 lost-wake correctness，不证明长 source scan 的调度公平性。每条 post-begin register / precheck 路径必须由字段级审计证明不会阻塞、不会嵌套 wait，且窗口短小可接受；如果 trace 或 workload 显示 deferred 长窗口造成可见饥饿，必须停止并回到 publish split / park permit 或等价更重设计。

## 非首选方向

- 公开 `ScheduleCaller` taxonomy：拒绝作为正文方案。早先的 `ScheduleCaller::{WaitSleep, RunnableYield, Idle, Zombie, Preempt}` 已收窄为 schedule entry split；底层 mode 是 scheduler-private，不是跨 subsystem contract。
- `NotParkReady` / `ParkReady` 常驻状态：暂不作为第一阶段必需状态。只有当 entry split 无法给出可审查 proof 时再回到该方向。
- begin / publish / park-ready 拆分：暂不作为第一阶段主线，避免重写 `ActiveWait` / `Latch` 的身份发布协议。
- 粗暴 guard：拒绝覆盖 source register scan；只允许作为 wait-core 内部极窄 critical section 的局部实现细节。
- no-source timeout 局部止血：只可用于复现缩小或 bisect，不能声明 contract 关闭。

## 接受边界

本 RFC 被接受表示：wait-core 需要补充 wake-prerequisite / parkability contract，且第一阶段按 schedule entry split / preempt-defer 方向实现。

本 RFC 被接受不表示：

- 可以只在 `fs::iomux` 或 `sched::latch` 内做 source-specific 修复。
- 可以跳过 `Latch::begin_current()` 全量直接用户、`Event::listen*()` users、direct wait 用户、signal wait、clock sleep 等 wait-core 调用面审计。
- 可以保留 scheduler owner 外的裸 `schedule()` caller。
- 可以把 `Waiting/PrePark` 在 preempt-deferred 路径中重新入队。
- 可以在 preempt-deferred 路径中静默丢失 `need_resched`。
- 可以让 post-begin source register scan 继续静默进入普通 sleepable lock 慢路径或其它 nested scheduler wait。
- 可以把 fanotify/source-owner nested-wait panic 当作 wait-core 需要支持 nested wait 的理由。
- 可以把 preempt-defer 当作任意长 `PrePark` setup 的公平性证明。
- 可以提供不携带 wait identity 的泛用 `schedule_wait_sleep()`，让 caller 以 `Runnable` 或 stale wait round 误入 wait-sleep 语义。

进入实现前，必须先把会改变 accepted contract、状态所有权、ABI / 可见语义或验收边界的 blocker 收口，并按正式 RFC workflow 建立 transaction devlog。只依赖真实调用面、锁路径或 trace 证据才能确认的不确定性，可以作为 implementation feedback gate 带入实现，但每项都必须在 [迁移实施计划](./implementation.md) 中写明受保护目标、验证方式、停止条件和 RFC 回写路径。

## 受控反馈边界

本文允许实现阶段反馈优化路线，但不允许反馈改写目标。

可以作为 feedback gate 带入实现的内容：

1. `Latch::begin_current()` direct users、`Event::listen*()` users（包括 futex wait）、direct wait helper、finite timeout wrapper 和 signal / clock wait 的精确调用面清单。
2. 每个 source register / predicate precheck 的 sleepability proof，以及需要 begin 前准备、register fail closed / retry、write set 扩展或归属其它 owner follow-up 的具体归类。
3. `Event::listen_with_timeout()`、source-backed finite timeout latch 和 no-source timeout 的 timer-installed / source-registered / already-completed no-park / park-ready trace 证据。
4. `schedule_preempt()` deferred 后 `need_resched` 恢复、deferred-task disposal、begin-to-explicit-sleep 窗口长度 / deferred count trace 字段集合和最小验证 floor 的实现落点。
5. fanotify 等 source owner 触发 single-active-wait 诊断时的反馈路由；这类反馈不允许削弱 wait-core 不变量。

不能作为反馈削弱的内容：

- schedule entry split / preempt-defer 是第一阶段方向；若真实证据推翻该方向，必须停止当前 gate 并回到 RFC review，而不是在实现中静默切换。
- `Waiting/PrePark` 不能被 involuntary preempt park，不能在 preempt-deferred 路径重新入队，也不能静默丢失 `need_resched`。
- wait identity、completion、cancel、finish、physical placement 和 parkability truth 仍由 wait core / task sched-state 拥有。
- wait-core 不支持同一 task 的 nested active waits；post-begin 可睡眠路径必须由 caller/source 修正或触发所属 owner 的 follow-up。
- `WaitSleep` 必须绑定当前 wait identity；already-completed no-park 和 active wait park 不能通过无 token 的普通 runnable/yield 语义推导。
- preempt-defer 不能被包装成长期公平性方案；若真实证据显示 `PrePark` setup 过长，本 RFC 必须回到设计层，而不是降低验证 floor 或接受饥饿。
- 不得用 source-specific workaround、降低验证 floor、隐藏失败路径、弱化 assert 或长期兼容桥把 feedback gate 伪装成关闭。

## 备选方案

### 继续在 iozone profile 中关闭内核抢占

拒绝作为语义修复。它可以作为定位证据，说明 hang 与 involuntary preempt window 有关；但关闭抢占会掩盖 wait-core 在真实配置下的 contract 缺口。

### 粗暴扩大 preempt-disabled 区域覆盖所有 iomux register scan

拒绝作为默认方向。部分 source register / poll 路径会经过普通 `Mutex` 或其它 sleepable 边界；把整个 register scan 放进 preempt-disabled 区域会违反现有锁语义，也会把调度层缺口推给 fs source。

### 只修空 iomux timeout

延期为可能的诊断止血。当前证据由 no-source timeout 高频触发，但同类 begin-before-park-ready 窗口存在于更通用的 wait-current-with-timeout 和 `Latch::begin_current()` direct users。最终关闭条件必须落在 wait-core contract 或等价证明上。

## 风险

- 虽然方向已选定，但如果实现只改 trap 入口或只改 no-source path，仍会把本问题收窄成 `iozone` 局部 bug，遗漏 `Latch::begin_current()` direct users 或 direct wait 用户。
- 如果把 wake-prerequisite / parkability truth 放进 `Latch` 或 source 侧，会制造第二套 parkability 真相源。
- 如果 scheduler wrapper 边界不清，会破坏 explicit wait sleep 的 park 语义或让 preempt fairness 回退；preempt-deferred 还必须避免吞掉 `need_resched`。
- 如果用关抢占止血但越过 sleepable lock 或 context switch，会引入更隐蔽的调度/锁错误。
- 如果只做 schedule entry split，而不增加 single-active-wait 诊断，post-begin nested wait 会继续以难定位的状态破坏形式存在。
- 如果把 fanotify 等 source owner 的 nested-wait panic 误归为本 RFC 必须现场修复，会把 scheduler/wait-core 修复扩成跨 subsystem source redesign。

## 收口

当前已提升为公开 RFC。收口顺序建议是：

1. 文档层确认 schedule entry split / preempt-defer 不变量。
2. 在 [Tracking Issues](./tracking-issues.md) 中关闭已被选型中和的问题，保留实现阶段 gate。
3. 若进入实现，按 [迁移实施计划](./implementation.md) 建立事务日志。
4. 完成后再更新旧 RFC 的 follow-up 链接、register 和双周 devlog。
