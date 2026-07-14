# Sched Wait Preempt Arming 不变量需求

**状态：** Canonical
**最后更新：** 2026-07-14
**父 RFC：** [RFC-20260618-sched-wait-preempt-arming](./index.md)

本文定义内核抢占下 wait-core wake-prerequisite / parkability 必须满足的协议边界。当前 RFC 选择 schedule entry split / preempt-defer 作为第一阶段方向；如果后续实现引入新状态、新 capability 或 begin 拆分，必须回到本文确认是否仍满足这些边界。

## 闭合条件

迁移完成后必须同时满足：

1. 一个 wait round 在本轮必要 timeout callback 安装和 source trigger 注册 / ready / unsupported 分类完成前，不会被 involuntary preempt schedule 消费成不可运行的 parked task。
2. explicit wait sleep 与 involuntary kernel preempt 在 wait-core park 权限上有清晰边界。
3. `Waiting/PrePark` 或其后继状态的含义不再同时表示“等待已发布”和“任意 schedule entry 都可 park”。
4. timeout、source trigger、signal/force/cancel 与 final finish 仍竞争同一个 wait identity；不得拆出第二套 completion truth。
5. 所有 `Latch::begin_current()` direct users（iomux、eventfd、timerfd、fanotify 等）、timer、signal 和 direct wait helper 不各自维护独立 parkability 状态。
6. 任何使用 preempt-disable / irq-disable 的修复都不能跨真正 context switch，不能覆盖可能 sleep 的 source register scan。
7. cleanup / drop / source pruning 不能成为防止 not-park-ready wait 被 park 的正确性支柱。
8. involuntary preempt schedule 看到 current task 为 `Waiting/PrePark` 时必须 defer，不得把该 task 转成 `Parked`，不得把仍为 `Waiting` 的 task 重新放入 runqueue。
9. preempt-deferred 不得静默丢失 resched 请求；如果 caller 在进入 scheduler 前已经清除 `need_resched`，`schedule_preempt()` 或等价 wrapper 必须在 deferred 返回前恢复或证明该请求仍被保留。
10. `ActiveWait::begin()` / `Latch::begin_current()` 发布当前 task 的 wait identity 后，到 finish / cancel / retire 前，同一 task 不得进入另一个 scheduler wait。post-begin 普通 `Mutex::lock()` 慢路径、`Event::listen*()` 或等价 nested active wait 都是 caller/source 责任边界错误，不是 wait-core 兜底对象。
11. nested active wait 必须有 release assert / 诊断暴露。诊断必须至少能定位已有 wait identity / caller location 与新 begin caller location。
12. 第一阶段 preempt-defer 只保证 not-park-ready wait 不会被 involuntary preempt park；它不是任意长 `PrePark` setup 的公平性保证。post-begin setup 窗口必须有字段级审计和 trace gate 证明短小、不可阻塞、不可嵌套 wait；若真实证据显示窗口过长或 deferred 过多导致可见饥饿，必须回到设计层评估 publish split / park permit 等更重方案。
13. processor pending-resched slot 是请求下一次 owner-CPU 完整选择的合并 latch。一次成功的 full pick 必须确认并清除选择前已经存在的全部 cause；即使 `prev == next`，也不能把旧 cause 带到下一次调度事务。
14. 未发生 full pick 时不得确认 processor pending-resched slot。destructive take 的 caller 在 `DeferredPreempt` 后恢复同一 snapshot；wait no-switch abort 不修改 slot。pick-time acknowledgement 之后产生的 cause 必须保留给下一次选择。

如果任一条件不成立，当前实现只能视为止血或迁移中间态，不能声明 `kernel_preempt` 下 wait-core wait/sleep 语义闭合。

## 非目标

本需求不包含：

1. 调度策略、时间片、负载均衡或 resched 触发策略重写。
2. `Latch` OR wait source 注册协议重写。
3. 完整 Linux waitqueue、epoll、futex PI 或异步通知框架。
4. 通过弱化 scheduler assert 或吞掉 wake/placement 诊断来绕过竞态。
5. 用关闭 `kernel_preempt` 或 busy polling 作为正确性边界。
6. 在本 RFC 内修复 fanotify 等 source owner 的 post-begin sleepability 问题。

## 已选方案边界

第一阶段采用 schedule entry split：

1. scheduler owner 外不得直接调用无语义的裸 `schedule()`；外部调用点必须通过语义化 wrapper 表达 entry intent。
2. 底层 `schedule_inner(mode)` 或等价函数可以使用 scheduler-private narrow mode，但该 mode 不得成为跨 subsystem contract。
3. `schedule_wait_sleep()` 是 `PrePark -> Parked` 的唯一正常消费点；该入口必须绑定 wait token 或 wait-core 私有 park permit，先证明 caller 仍命名当前 wait round，并且只在本轮 wake prerequisites 已安装或完成 ready / registered / unsupported 分类后使用。不得存在无 wait identity 的泛用 wait-sleep 入口。
4. involuntary preempt schedule 只表示抢占公平性，不表达 wait sleep intent。它只能抢占仍为 `Runnable` 的 current task。
5. `Waiting/PrePark` 是 wait identity 已发布、但当前 task 仍在 CPU 上执行 wait setup 的状态。它不是任意 schedule entry 都可消费的 park-ready 状态。
6. trap return、preempt-enable 或其它 involuntary preempt 入口如果观察到 `Waiting/PrePark`，必须返回 deferred 结果，或者以等价机制保持当前 task 继续执行。
7. `Waiting/Parked` 出现在 current task 的 involuntary preempt schedule 入口是强不变量异常；实现应记录上下文并用 release-build assertion 暴露错误。
8. `schedule_runnable()` 或等价 wrapper 只处理 current 仍为 `Runnable` 的 yield / idle reschedule 路径；它不得消费 `Waiting/PrePark`。
9. `schedule_zombie_never_return()` 是 no-return 退出路径；它不得让 zombie schedule 与普通可返回 reschedule 混用语义。退出清理完成后，`TaskSchedState::Zombie` 的最终发布属于 scheduler core：`Runnable -> Zombie` 必须和不可返回 `switch_out()` 位于同一 noirq scheduler 事务内，避免 trap-tail preempt 观察到已是 `Zombie` 但尚未切走的 current task。
10. scheduler-private mode 是调用入口上下文，不是 wait round 的第二套状态。它不得被 source 保存，不得跨 wait round 缓存，也不得作为 producer trigger 的能力。

## 状态所有权

当前 task 的等待状态仍必须由 wait core / task sched-state 拥有。

硬性要求：

1. wait identity、completion outcome、cancel、finish 和 physical placement 仍由 wait core 统一管理。
2. 如果引入 `NotParkReady`、`ParkReady`、`ParkIntent`、`ParkPermit` 或等价状态，它必须属于 wait-core/task-sched-state 协议，而不是 `Latch`、iomux source、timer callback 或 signal subsystem 的私有真相。
3. `Latch` 只能封装一轮 wait identity 和 producer trigger capability，不能通过私有 boolean 决定 task 是否允许被 scheduler park。
4. timeout callback 安装、source trigger 注册或 source ready / unsupported 分类可以影响是否进入 `schedule_wait_sleep()`，但不能成为绕过 wait-core completion 的第二套 wake state machine。
5. `task.status()` 仍只能作为投影视图或诊断入口，不得重新成为 wait/sleep 协议写入口。

允许保留诊断字段，例如 wait id、begin caller location、timeout-installed point、source-registered count、park-ready point、schedule entry/mode 和 timeout id。诊断字段不能反向驱动状态机。若 origin 字段只用于 panic、review 或排障，字段旁必须说明它不参与行为决策。第一阶段 wait origin 只保存 `core::panic::Location::caller()`，不增加 primitive / operation taxonomy；如果后续日志证明 caller location 不够读，再回到本文审查是否需要新的诊断维度。

processor pending-resched 的行为真相只保存在本地 `Processor` slot。`take_pending_resched()` 返回 typed single-bit snapshot 并清 slot；`schedule_preempt(pending)` 只用非空值证明 entry 合法，destructive-take caller 在 deferred 后以 union restore 同一 snapshot。pending 不进入 preempted-current class transaction，也不是 slot capability 或第二份长期真相。successful-pick acknowledgement 清除此前 slot，take 后新产生的 request 仍属于下一轮。

`WaitStateStatus::Armed`、`WaitOutcome::Armed` 和 `WakeToken::is_armed()` 只能被解释为 completion-open identity state：本轮 wait 仍允许 timeout、source trigger、signal/force/cancel 竞争 completion。任何实现或审查不得把它们当作 timeout callback 已安装、source trigger 已注册，或 task 可被 scheduler park 的证明。

`PollRegisterResult::Armed` 是 iomux/source register outcome，只表示该 source 已在自己的 owner 边界保存 trigger。它不是 wait-core Armed，也不是 park-ready 证明；source-backed wait 仍必须完成所有 source 的 ready / registered / unsupported 分类，并在 finite timeout 场景中完成 timeout prerequisite 后，才能进入 explicit wait sleep。

## 身份与能力模型

一个 wait round 的身份仍来自 wait core 的 `WaitState` / `WakeToken` 或等价能力对象。

要求：

1. parkability 变化不能改变 wait identity，也不能让旧 token 完成新 wait round。
2. 如果新增 park permit 或 explicit wait-schedule capability，它必须是 wait core 私有构造的短寿命能力，不能由 source 伪造，不能跨 wait round 缓存。
3. producer-held trigger 仍只能尝试完成对应 wait round；不能获得“把 task park 起来”或“重新入队”的能力。
4. waiter-owned guard 负责 finish / cancel / retire；不得把 finish 责任转移给 timeout/source cleanup。
5. signal / force 如果通过 active wait 完成当前 round，仍必须使用 wait core 的 active-wait wake 入口，而不是读取 source-local registration 或 parkability 状态。
6. wait origin / begin caller location 只服务诊断；它不能作为 wake identity、park permission 或 source registration truth。

## 线性化点

本文需要三个边界在设计中被显式说明：

1. completion 线性化点：timeout、source trigger、signal、force、cancel 谁赢得本轮 wait outcome。
2. parkability 线性化点：本轮 wait 何时从“已创建/已发布但不可被 involuntary preempt park”变成“explicit wait sleep 可以 park”。
3. resched acknowledgement 线性化点：owner CPU 何时已经用当前 runqueue 状态完成一次 full pick，从而确认此前合并在 processor slot 中的重新选择请求。

硬性要求：

1. completion 线性化点仍在 task sched-state / wait-core 事务内。
2. parkability 线性化点必须与 wait identity 绑定，不能只靠当前 CPU 临时变量或 source-local flag。
3. involuntary preempt schedule 看到 `Waiting/PrePark` 时，必须拒绝消费本轮 park intent，并保持 current task 继续运行；不得把 waiting task 重新入队。
4. explicit wait sleep 只能在本轮必要 wake prerequisites 已安装完成，或方案能证明本轮 wait 已经完成并走 no-park / abort-sleep 返回时，才能消费 wait round；不得在 active wait 缺少必要 wake source 时把 task 变成 parked。
5. 如果实现选择拆分 begin/publish，必须定义 early trigger、signal、cancel 与 timeout expiry 在 publish 前后的线性化规则。
6. resched acknowledgement 必须紧跟成功的 `pick_next_task()` selection，并位于 next-task switch-in transaction 之前；它不能散落在 schedule entry wrapper，也不能推迟到旧 task 恢复执行之后。
7. `DeferredPreempt` 与 wait no-switch abort 没有执行 full pick，因此不能到达 acknowledgement。acknowledgement 后新插入 processor slot 的 cause 属于下一轮，必须保持 pending。

在已选方案下，parkability 线性化点是 token-bound / permit-bound `schedule_wait_sleep()` 入口，而不是 `WaitStateStatus::Armed`、`Latch::begin_current()` 或 source-local register flag。对 finite timeout wait，`schedule_wait_with_timeout()` 必须先确认 token 命名的 wait round 是否仍 active：若 source / signal / force 已经完成本轮 wait，走 no-park / abort-sleep 返回；若本轮 wait 仍 active，必须先安装 timeout callback，再通过 explicit wait-sleep entry 消费 `PrePark`。对 source-backed latch wait，调用方必须先完成 register scan 的 ready / registered / unsupported 分类，再进入 explicit wait-sleep entry。若 current 已回到 `Runnable`，只有 matching token / permit 能把该状态解释为本轮 already-completed abort；普通 runnable reschedule 不得借此进入 wait-sleep 分支。

## 锁序与生命周期规则

### Wake-Prerequisite / Park-Ready 窗口

begin-to-park-ready 窗口必须被显式关闭或证明安全。

要求：

1. finite timeout wait 必须在 task 可被 explicit wait sleep park 前完成 timeout callback 安装；如果 timeout 安装前 wait 已经由 source / signal / force 完成，explicit wait-sleep 入口必须证明该轮已经完成并 no-park 返回。
2. source-backed wait 必须在所有参与阻塞判定的 source 都 ready、trigger registered 或明确 unsupported/fallback 后，才允许进入 explicit wait sleep。
3. signal / force 对 active wait 的可见性不能被新 park-ready 状态破坏；如果某阶段 wait 已发布但不可 park，signal/force 的完成规则必须明确。
4. begin 后的任何 early return 都必须 cancel/finish/retire 本轮 wait identity。
5. begin 后的 register scan / predicate precheck 不得进入普通 blocking lock 慢路径或任何会调用 `ActiveWait::begin()` / `Event::listen*()` 的路径。若某 source 无法保证这一点，本 RFC 只要求 wait-core 诊断暴露违规；source owner 的具体修复可路由到所属 RFC / follow-up，除非违规发生在 scheduler/wait-core 自身新增路径内。
6. begin 后到 explicit wait-sleep 前的 source scan / predicate precheck 必须能被审计为短小、不可阻塞、不可嵌套 wait。第一阶段不设计通用 runtime 机制替代这个 proof；如果实际 trace 显示 `schedule_preempt()` 长时间反复 deferred，或 workload 因 `PrePark` setup 产生可见饥饿，当前方案只能视为 correctness 止血，不能声明公平性闭合。

### Preempt / IRQ guard

如果方案使用 guard 关闭窗口：

1. guard 范围必须短到只覆盖不可被 preempt 打断的 wait-core critical section。
2. guard 不得跨 `schedule()` 或任何真实 context switch。
3. guard 不得覆盖可能阻塞的 source register scan 或普通 `Mutex::lock()` 路径。
4. guard 释放点必须在代码和注释中说明为什么本轮 wait 已经 park-ready 或不再需要 park。

已选方案不需要用大 guard 覆盖 begin-to-register scan。若实现为了局部原子性在 wait core 内使用 guard，该 guard 只能覆盖 scheduler state 事务本身；一旦要进入 source scan、普通 `Mutex::lock()`、timer callback 分配或 context switch，guard 必须已经释放。

### Source register scan

source register scan 仍按 `sched-latch` 的 source-side lock 规则审计；本 RFC 不改变 source predicate + trigger detach 的线性化要求。

额外要求：

1. 不能为了关闭 wait-core preempt window 而让 fd/device source 直接操作 `TaskSchedState`。
2. 不能要求所有 source register scan 都运行在 non-preemptible context。
3. 对 sleepable source，修复必须允许它们继续使用现有 sleepable lock 语义，但使用点必须位于 active wait 发布前，或在 register 阶段 fail closed / retry；不能在 post-begin 窗口里实际阻塞。
4. wait-core 不支持同一 task 同时拥有两个 active wait identities。任何看似需要 nested wait 的 source 注册路径，都必须先回到调用点或 source owner 边界重排。
5. 若 fanotify 或其它 source owner 当前触发 nested active wait 诊断，本 RFC 将该 panic 视为对应 owner 的反馈证据，而不是 wait-core 支持 nested wait 的理由。
6. 对 source-backed iomux 这类可能扫描多个 fd/source 的路径，审计必须记录 post-begin scan 的规模来源、锁边界和第一处可能 explicit wait-sleep 点；不能把 preempt-defer 当作无限 source scan 的调度公平性兜底。

## 禁止退化项

以下做法不能作为最终关闭条件：

1. 全局关闭 `kernel_preempt` 或只在 iozone profile 里关闭抢占。
2. 把整个 iomux register scan 放进关抢占区域。
3. 让 `Latch` 或某个 fs source 私有保存“wake-prerequisite / park-ready” correctness truth。
4. 在 `schedule()` 中无条件把所有 `Waiting/PrePark` 都重新入队，同时不证明 explicit wait sleep 仍能 park。
5. 只给 `wait_without_iomux_sources()` 加局部 guard，并声明 wait-core contract 已关闭。
6. 依赖 timeout cleanup、source cleanup、Drop 或 stale pruning 防止 lost wake。
7. 放宽 `task_enqueue()` / wait-core assert 来掩盖旧 wake、提前 park或 stale placement。
8. 用更长 timeout、busy loop 或额外 yield 避开触发窗口。
9. preempt-deferred 后不恢复 `need_resched`，导致抢占请求被静默吞掉。
10. 让 preempt entry 调用显式 wait sleep helper，使 `Latch::schedule_with_timeout()` 或 `wait_current_with_timeout()` 失去 park 能力。
11. 把 post-begin nested scheduler wait 描述成 wait-core 需要支持的 corner case，或用第二套 source-local park-ready flag 掩盖它。
12. 将公开 `ScheduleCaller` taxonomy 作为跨 subsystem contract 重新引入。
13. 让 tick / runnable-arrival cause 在 block、yield、zombie 或 same-task full pick 后继续滞留到下一轮调度事务。
14. 为区分旧 cause 与新 cause 引入 epoch、token 或事件队列，而当前语义只需要一次 successful-pick acknowledgement。

## 完成标准

文档层完成标准：

1. [Tracking Issues](./tracking-issues.md) 中会改变 accepted contract、状态所有权或验收边界的 Apollyon / Keter 已有明确处理结论；已被 schedule entry split / preempt-defer 中和的问题标为 Neutralized。
2. 允许带入实现的 Apollyon / Keter 必须被降为 implementation feedback gate：对应目标和不变量不可削弱，验证方式、失败信号、停止条件和 RFC 回写路径必须写入 [迁移实施计划](./implementation.md)。
3. 已选方案能覆盖所有 `Latch::begin_current()` direct users、`Event::listen*()` users（包括现有 futex wait）、no-source timeout、direct timeout wait、signal wait 和 clock sleep 等调用面，而不要求本 RFC 修复各 source owner 的全部 sleepability 缺陷。
4. `implementation.md` 从占位状态更新为分阶段计划，并记录 write set、review gate、验证 floor、反馈 gate 和停止边界。
5. post-begin nested wait 的处理结论已进入 canonical plan：wait-core/scheduler 必须诊断暴露；source owner 违规可路由为对应 owner 的 follow-up，不能留作 wait-core 兜底。

实现层完成标准待方案确定后补充。最低验证应包含：

1. 定向覆盖 begin-to-park-ready 被 kernel preempt 打断的测试或可复审 trace。
2. `kernel_preempt` 开启下的 iozone throughput 复核。
3. wait-core direct timeout 用户的源码审计或定向 smoke。
4. source-backed `Latch` path 不因修复引入 sleepable lock / preempt guard 冲突的审计。
5. `Event::listen*()` user gate：futex wait、threaded timer test wait 等现有 Event wait 用户至少有源码 proof；风险高时补 smoke。
6. preempt-deferred fairness gate：trace 至少能观察 begin-to-explicit-sleep 窗口和 deferred count；如果窗口过长或 deferred 过多，回到设计层而不是扩大 preempt-defer 语义。
7. nested active wait 诊断 gate：二次 begin 时 release assert 能定位已有 wait caller location 和新 begin caller location；fanotify/source-owner 触发该 assert 时按反馈路由记录，不作为本 RFC 的 scheduler/wait-core blocker。
8. pending-resched lifecycle gate：source audit 证明 production clear 紧跟 full-pick selection、所有 destructive take caller 的 deferred restore 仍闭合，且 no-switch path 不调用 full pick；不得为一次字段赋值引入只服务测试的 wrapper。
