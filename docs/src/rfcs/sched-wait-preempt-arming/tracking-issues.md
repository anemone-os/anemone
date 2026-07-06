# Sched Wait Preempt Arming Tracking Issues

**状态：** Active
**最后更新：** 2026-07-06
**父 RFC：** [RFC-20260618-sched-wait-preempt-arming](./index.md)
**事务日志：** [2026-07-06-sched-wait-preempt-arming](../../devlog/transactions/2026-07-06-sched-wait-preempt-arming.md)

本文只跟踪当前仍影响方案选择、review gate、停止边界或验收判断的 RFC 层问题。已被正文接受的问题陈述、单纯 implementation pending、已 neutralized 的备选方案和纯命名延期不在这里重复记录；若本 RFC 进入实现，应建立 transaction devlog。

阶段 1 反馈对 tracker 做过一次收口：`KETER-001` 已由 scheduler entry split 和裸 `schedule()` call-site 清理中和，移入 `Neutralized`。active tracker 只保留仍需要真实 source-owner 路径、字段级 proof 或 trace 证据闭合的 gate：`KETER-004`、`KETER-005`、`KETER-006`。已经折回 canonical contract、implementation gate 或阶段验证清单的问题移入 `Neutralized`，不再作为开放缺陷重复跟踪。

状态后缀说明：

- Implementation feedback gate：允许带入实现阶段，但必须有受保护目标、验证方式、失败信号、停止条件和 RFC 回写路径；反馈只能优化路线，不能削弱目标或不变量。
- Caller-source feedback gate：允许通过真实 source owner / lock 路径决定修复归属；如果需要改变 owner boundary、source register contract 或 wait-core 不变量，停止并回到 RFC review。

## Keter

### KETER-004：post-begin nested wait 必须诊断暴露，但 source owner 修复不默认属于本 RFC

**状态：** Open / Core diagnostic implemented / Caller-source feedback pending

**问题：** schedule entry split / preempt-defer 只覆盖 involuntary preempt。若 `Latch::begin_current()` 发布当前 task 的 active wait 后，source register scan 再进入普通 sleepable lock 的慢路径，它可能通过 `Event::listen_uninterruptible()` 创建第二个 `ActiveWait`。这不是抢占入口误 park，而是已发布 wait round 内部发生 voluntary / nested wait，会破坏 single active wait 和 wait-core 状态所有权。

**当前证据：** `fanotify_poll()` 会进入 `FanGroup::poll()`，而 `FanGroup::state` 是普通 `Mutex<FanGroupState>`；`Mutex::lock()` 慢路径会通过 `Event::listen_uninterruptible()` 阻塞。只说 “source-backed iomux register scan 可经过 sleepable boundary，期间 trap preempt 只能 defer” 还不足以证明该路径安全。

**结论：** 这不是 wait-core 应该兜底的 corner case。wait-core 继续维持 single active wait，并必须用 release assert / 诊断暴露二次 begin。fanotify 等 source owner 的具体修复作为所属 RFC / follow-up 反馈处理，不默认进入本 RFC write set。

**需要收口：** 阶段 0 必须补 single-active-wait 诊断，panic 信息至少包含 current task、已有 wait id、已有 wait caller location 和新 begin / sleep-attempt caller location。第一阶段 origin 只保存 `core::panic::Location::caller()`，不引入 `WaitPrimitive`、`operation` 字符串或其它 caller taxonomy。阶段 3 必须定义反馈路由：若 panic 来自 fanotify 或其它 source owner，记录为对应 owner follow-up；若 panic 来自 wait-core/scheduler 新增路径，则作为本 RFC blocker 修正。

**阶段 1 反馈：** core 侧诊断已经落地：`begin_wait()` 的 nested active wait assert 会报告 existing/new caller location，`assert_current_not_in_active_wait()` 会报告 existing begin 与 sleep-attempt caller location，`Mutex::lock()` 在 `allow_preempt()` 诊断被遮蔽前和 slow path 进入 `Event::listen_uninterruptible()` 前都会检查当前 task 是否已有 active wait。阶段 1 没有修改 fanotify 或其它 source owner；因此本项不再阻塞 scheduler entry split，但仍保留为 source-owner feedback gate。后续需要用真实触发或源码 proof 确认 fanotify/source-owner 路径如何归档：若触发诊断，记录为对应 owner follow-up；若 wait-core/scheduler 新增路径触发，则回到本 RFC 修正。

**反馈边界：** 具体 source 的修复形状允许由真实 lock / owner 路径反馈决定；但 single-active-wait、不引入 source-local parkability truth、sleepable scan 不放入 non-preemptible 区域是不可削弱的不变量。

### KETER-005：finite timeout wrapper 的 explicit-sleep 前 proof

**状态：** Open / Implementation proof landed / Trace proof pending

**问题：** 当前 proof 清单覆盖 no-source timeout、source-backed latch、eventfd / timerfd / fanotify blocking wait 和 `wait_current_with_timeout()`，但还没有显式覆盖 `Event::listen_with_timeout()` 与 source-backed iomux finite timeout。它们都可能先 publish wait identity，再到 `schedule_wait_with_timeout()` 内部才安装 timer callback。

**影响：** 如果实现只证明 no-source timeout 或 direct wait helper，仍可能漏掉 begin/register/listener setup 到 timer-installed 之间的抢占窗口。source trigger 已注册并不等于 finite timeout prerequisite 已安装；两者都要进入 park-ready proof。另一方面，source / signal / force 也可能在 timeout 安装前已经完成本 wait round；此时 explicit sleep 入口必须识别 already-completed wait 并直接 abort，不得把它当作普通 runnable yield，也不得重新 park。

**需要收口：** `schedule_wait_with_timeout()` 必须成为 finite timeout 的唯一 proof 点：进入 explicit wait-sleep 前，要么证明 token 命名的 wait round 已经完成并走 no-park / abort-sleep 返回，要么在 wait 仍 active 时先安装 timeout callback，再调用 token-bound / permit-bound explicit `schedule_wait_sleep()` 消费 `PrePark`。任何 `WaitSleep` wrapper 都不得无 token / permit 暴露给 scheduler owner 外部。阶段 0 / 阶段 3 必须补 `Event::listen_with_timeout()` 和 source-backed finite-timeout latch 的字段级 proof，并说明 preempt-defer 覆盖到 `schedule_wait_with_timeout()` 完成 timer install 或确认 already-completed 为止。trace gate 至少覆盖一个 Event timeout 和一个 finite-timeout iomux source path，且要能区分 timer-installed park 与 early source-completed no-park。

**阶段 1 反馈：** 代码层 proof 已经落地：`schedule_wait_with_timeout()` 在 wait 仍 active 时先安装 timeout callback，再通过 token-bound `schedule_wait_sleep(&WakeToken)` 消费 `PrePark`；`schedule_wait_sleep()` 只用 wait identity pointer check 证明 token 命名当前 wait round，不把 `Armed` 当作 park-ready。若 token 命名的 wait 已经完成且 current 回到 `Runnable`，走 no-park / abort-sleep 返回而不切换。`Event::listen()` / `listen_uninterruptible()` 的 direct sleep 已迁移到 token-bound wait-sleep，`Event::listen_with_timeout()`、`Latch::schedule_with_timeout()`、no-source timeout 和 `wait_current_with_timeout()` 均通过同一个 finite-timeout proof 点。剩余缺口是阶段 3 trace / 字段级 proof：至少覆盖一个 Event timeout 和一个 source-backed finite-timeout iomux path，并能区分 timer-installed park 与 early source-completed no-park。

**反馈边界：** timer-installed / source-registered / explicit-schedule / already-completed abort 的精确线性化证据允许由 trace 反馈确认；如果证据显示 active finite-timeout wait 无法在 explicit wait sleep 前建立 timeout prerequisite，或 already-completed wait 会被误 park / 误 requeue，必须回到 RFC review，而不能靠延长 timeout、busy loop 或 source-local flag 关闭。

### KETER-006：`PrePark` setup 窗口不能变成无界公平性盲区

**状态：** Open / Implementation feedback gate

**问题：** schedule entry split / preempt-defer 能防止 not-park-ready wait 被 involuntary preempt park，但它也会让 current task 在 `Waiting/PrePark` setup 窗口内继续运行。如果 source-backed register scan、predicate precheck 或其它 post-begin setup 是用户规模驱动的大循环，`schedule_preempt()` 可能持续 deferred。这样 lost-wake correctness 被修掉了，但调度公平性风险会被隐藏在 `PrePark` 窗口里。

**结论：** 第一阶段不设计通用 hard-prevention 机制；begin/publish 拆分、常驻 `NotParkReady/ParkReady` 或 park permit 都属于更重设计，只有在真实证据证明 preempt-defer 不足时再回到 RFC review。本阶段依赖字段级审计、single-active-wait 诊断和 trace gate 管住风险。

**需要收口：** 阶段 0 必须为每条 post-begin source scan / predicate precheck 记录规模来源、锁边界、是否可能阻塞、是否可能 nested wait、first explicit wait-sleep point，以及 begin-to-explicit-sleep / deferred-count 的观测字段。阶段 3 必须用源码 proof 或 trace 证明这些窗口短小可接受。若真实 workload 或定向 trace 显示 `PrePark` setup 长时间反复 deferred，必须停止并回到 publish split / park permit 或等价设计，而不能把 preempt-defer 宣称为公平性闭合。

**阶段 1 反馈：** `schedule_preempt()` 的 deferred 语义已经实现，`Waiting/PrePark` 不再被 involuntary preempt park、requeue 或 switch out，且 deferred path 会恢复 `need_resched`。这关闭了 lost-wake correctness 的核心入口，但没有证明 post-begin setup 窗口短小，也没有提供 begin-to-explicit-sleep elapsed / deferred-count trace。因此本项仍保持 Open；后续必须通过阶段 3 源码 proof 或 trace 证明 boundedness，不能把 preempt-defer 当成公平性闭合。

## Euclid

None。当前剩余 Euclid 已折入 implementation gate 或阶段验证清单，不作为独立 open tracker 保留。

## Neutralized

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
