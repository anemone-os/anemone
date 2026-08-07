# POSIX Timer Thread-ID Notification 目标与不变量

**状态：** Closed
**最后更新：** 2026-08-06
**父 RFC：** [RFC-20260804-posix-timer-thread-id-notification](./index.md)
**适用修订：** R0

本文只定义本 RFC 的 target 与 contract proof obligations。Gate 2/3 closure 后，effective 规则以
[`POSIX-TIMER-001`](../../contracts/time/posix-timer.md) 和
[`SIGNAL-PENDING-001`](../../contracts/signal/pending-routing.md) 为准。

## 规则分类

- **Correctness Invariant：** 唯一 owner、并发、生命周期、cleanup、内存安全和 ABI 诚实性；
  不可通过工程妥协降低。
- **Target Guarantee / Capability：** R0 接受的能力；只能通过 Target Renegotiation 改变。
- internal capability 的具体类型、slot 容器、锁类型、模块和文件布局属于 implementation preference。

## Target Invariants

### PT-TID-001 — Native sigevent 解码停留在 ABI 边界

**规则：** RV64/LA64 native `SigEvent` 的 size/alignment/offset 保持 Linux asm-generic 布局；
`SIGEV_THREAD_ID` target 从尾部 union 的首个 `i32` 读取，并在进入 timer/signal core 前转换为
exact task identity capability。`sigev_notify` 只接受完整枚举值，未知/混合 bits 不能部分解释；core
object 不保存 raw union，不根据架构猜 offset。
**Owner：** `anemone-abi` layout 与 POSIX timer syscall decode boundary。
**依赖：** `xref:linux-6.6.32:include/uapi/asm-generic/siginfo.h#sigevent_t`。
**违反表现：** layout 漂移、直接把 `_sigev_un` 数组传入 core、在 expiry 时再次解释 userspace
union、接受值 4 以外的混合 bits，或将 `SIGEV_THREAD` callback 字段当作内核输入。
**Cutover / Proof：** Gate 0 layout/offset KUnit 与 raw RV64 syscall oracle（LA64 runtime由维护者 waiver）；Gate 2 cutover。

### PT-TID-002 — Registration 绑定 exact live task identity

**规则：** create 只能在 target 与 caller 属于同一 live `ThreadGroup`，且 target 的 private timer
registration admission 尚未关闭时成功。registration 保存 non-rebinding identity capability；该 identity
可以独立稳定存活，但不能延长 target `Task` 的可执行/成员生命周期，数值 TID 也不参与后续行为决策。
target 退出、TID 被复用或 member 集合改变后，notification 不得改投其它 task。稳定的 invalid/foreign/
closed target 返回 `EINVAL`；concurrent exit 可以先成功绑定原 identity或让create以`EINVAL`失败，但不能
产生第三种半发布状态。
**Owner：** task topology/membership 解析 target；target task 的 Signal owner 决定 registration admission。
**依赖：** [`TASK-LIFE-002`](../../contracts/task/thread-group-lifecycle.md#task-life-002--最后-member-detach-后才能发布-exited)。
**违反表现：** foreign target 被接受、expiry 重新按 TID lookup、`Task` 被 registration 保活、leader/
任意 member fallback，或新 task 因 TID reuse 收到旧 timer signal。
**Cutover / Proof：** create/exit race KUnit，foreign/exited/TID-reuse runtime oracle；Gate 2 cutover。

### PT-TID-003 — Exact timer occurrence 只有 task-private pending owner

**规则：** `SIGEV_THREAD_ID` 的每个 registration 在 exact target 的 `Task::sig_pending` 中拥有独立
slot。timer expiry 只提交 timer ID、generation、episode、overrun 与 `sigval`；Signal owner 决定
ignored admission、control generation、mask、wake、普通trap-return delivery、`rt_sigtimedwait`同步消费、
flush 与 frame。不同 timer 的同号 standard signal 不合并；同一 registration 已 pending 时只更新该 slot
的当前 episode/overrun规则。普通 task-private standard signal 的既有单 slot 合并不变；realtime timer slot
与同号 ordinary task-directed realtime occurrence 保持 Linux 的 arrival ordering。
userspace `SI_TIMER` 中的`si_tid`始终是timer ID而不是target TID，`si_value`保留create输入；target TID只在
registration admission时选择private owner，不进入delivery identity。
`Queued`/`AlreadyPending`等待dequeue且不立即rearm；`Dequeued`提交最近交付overrun并锁外rearm；`Ignored`
不提交delivery snapshot但保留timer-local overrun accrual并立即继续periodic arm，恢复disposition后的首次
真实delivery再提交累计；`Flushed`/`TargetExited`不rearm。`Consumed`只允许现行
`SIGSTOP` scoped exception使用，并按既有control consumption提交overrun、继续periodic arm，不能扩成普通
notification shortcut。
**Owner：** exact target 的 Signal private pending owner。
**依赖：** target `SIGNAL-PENDING-001` Refine、`SIGNAL-ACTION-001/002`、`JOBCTL-SIGNAL-001`。
**违反表现：** private/shared 同时持有 occurrence、timer object 读取 pending bit/slot、notify 成为
delivery truth、同号不同 timer 合并、同步wait绕过timer completion、ignored periodic timer停止、Consumed
越出`SIGSTOP` scoped exception、ignored expiry清空尚未交付的overrun accrual，或task-directed timer被任意
member fetch。
**Cutover / Proof：** Signal owner-local KUnit、exact-task frame/`rt_sigtimedwait` oracle、同号ordinary
task-directed realtime ordering与普通signal regression；Gate 2 cutover。

### PT-TID-004 — Task exit 先关闭 registration admission 再 detach

**规则：** task exit 必须在 membership detach 前关闭新的 task-private timer registration admission，
并 retire 该 task 的 timer registrations、pending occurrences 与 reserved deliveries。Signal guard 内只摘除
Signal-owned state并取得 immutable completion；registration drop 与 timer-owner callback 必须在 guard 外执行。
未到期的物理 arm 不因 exit 提前取消，到期 exact enqueue 失败后停止；已经 pending 的 occurrence 被 flush
时不执行 delivery-driven rearm，因此没有下一次物理 arm；已经 dequeue/rearm 的 occurrence 保留新物理 arm，
到期 enqueue 失败后停止。三种状态都不能 retarget，且 flush 不更新最近交付 overrun。
并发 create 要么在关闭前完整建立可由 exit 找到的 registration，要么返回 `EINVAL` 且不发布 timer ID。
**Owner：** target task 的 exit cleanup 关闭 private admission；Signal owner retire slot/pending；timer owner
消费 typed completion；timer owner在发生 failed expiry enqueue 时停止对应物理 arm。
**依赖：** `TASK-LIFE-002` 与 `SIGNAL-TEMP-MASK-002/003`。
**违反表现：** exit 后仍可注册、detach 后遗留不可达 slot、pending flush触发rearm、回调在 pending lock 内
重入 timer、create成功但 registration 不属于任一 cleanup owner，或 deferred `Task::Drop` 偶然承担行为 cleanup。
**Cutover / Proof：** Gate 1 exit/create/pending-reservation race tests 与 lock-order audit；Gate 2 runtime race oracle。

### PT-TID-005 — Delete、pending 与 target exit 的可见顺序

**规则：** timer deletion 继续先从 `ThreadGroup` table 移除 ID，再推进 generation、物理取消 queued
request、撤销 signal registration。已经由Signal拥有的private occurrence不被delete recall，仍可交付原
`SI_TIMER`；其stale completion只能结束旧handoff，不能rearm/revive已删除或复用ID。target exit会flush
该task已经pending/reserved的occurrence，但不提前取消尚未到期的arm。未到期或已经dequeue/rearm的物理
arm在下一次expiry对closed exact identity的enqueue失败后停止；pending occurrence被flush后不产生新物理
arm。periodic timer即使没有物理request，`timer_gettime()`仍须按Linux requeue-pending规则保留interval并
投影未来value；oneshot在flush或failed expiry后value为零。
**Owner：** `ThreadGroup` POSIX timer object/table。
**依赖：** `POSIX-TIMER-001` 与 `SOFT-TIMER-REQUEST-001`。
**违反表现：** delete撤回已排队signal、callback恢复已删除ID、target exit立即取消尚未到期arm、flush
错误rearm、旧target-exit结果disarm新generation、registration withdrawal先于ID removal使lookup观察半删除
对象、failed exact delivery后periodic timer仍物理重排，或把`timer_gettime()`投影误当作物理request。
**Cutover / Proof：** delete/replace/exit/in-flight callback KUnit 与 stress oracle；Gate 2 cutover。

### PT-TID-006 — Signal guard 不跨越 timer-owner callback

**规则：** pending publication、普通fetch、`rt_sigtimedwait`同步fetch、flush、target exit 与 registration withdrawal 可以在 Signal owner
guard 内移动 Signal state，但所有 timer-owner callbacks、registration destructor、scheduler wake side effect
及可能释放最后一个 captured owner reference 的 drop 必须在 Signal/ThreadGroup membership guard 外执行。
Timer callback 不取得 Signal 私有 lock，不形成 Signal -> timer -> Signal 回环。
**Owner：** Signal/timer handoff protocol。
**依赖：** `SIGNAL-PENDING-001` 的现行锁外回告规则。
**违反表现：** exit/flush 在 private pending lock 内调用 timer object、timer delete 等待持有 pending lock 的
callback、或 Drop 路径先断言再撤销 published registration。
**Cutover / Proof：** Gate 1 source/lock-order audit，KUnit callback 重入测试；Gate 3 final audit。

### PT-TID-007 — Dequeue 与 flush 使用不同 completion reason

**规则：** Signal owner在guard外回告timer时必须区分`Dequeued`与`Flushed`。普通trap-return fetch和
`rt_sigtimedwait`同步fetch属于dequeue：在dequeued siginfo通过frame/copyout对userspace可见前，timer owner
必须把pending期间新增、可归属于该delivery的expirations合入finalized overrun；Signal据此更新
`si_overrun`，timer owner在同一次handoff中提交`timer_getoverrun()`最近交付snapshot，然后才允许periodic
rearm。没有replace/republication的单episode中两者必须是同一个钳位值；若occurrence经历Linux式
already-pending republication，`si_overrun`可以额外包含occurrence-local base count，但仍不得停留在首次
enqueue值。
该handoff可以返回值、受控可变payload或等价的窄协议实现，但不能是只通知timer owner、无法修正dequeued
siginfo的单向callback。即使后续signal-frame或siginfo copyout失败，也按Linux的dequeue顺序保留该提交。
target exit、exec或disposition cleanup在未被consumer取得前移除occurrence属于flush：它只解除pending
ownership，不能更新最近交付snapshot、改写userspace `si_overrun`或执行delivery-driven rearm。timer已删除时
两种stale completion都只能结束旧handoff；已排队occurrence仍可按其已有siginfo交付，但不能恢复已删除timer。
**Owner：** Signal owner分类completion；POSIX timer owner按typed reason提交或丢弃delivery snapshot。
**依赖：** `xref:linux-6.6.32:kernel/signal.c#dequeue_signal`、
`xref:linux-6.6.32:kernel/signal.c#send_sigqueue`、
`xref:linux-6.6.32:kernel/signal.c#flush_sigqueue`、
`xref:linux-6.6.32:kernel/time/posix-timers.c#posixtimer_rearm`与`POSIX-TIMER-001`的delivery snapshot规则。
**违反表现：** userspace取得enqueue-time `si_overrun`而`timer_getoverrun()`取得dequeue-time新值、exit flush
增加`timer_getoverrun()`、flush重新安排periodic timer、copyout失败回滚已经dequeue的occurrence，或timer
owner根据callback发生位置猜测completion reason。
**Cutover / Proof：** Gate 1 dequeue/flush owner-local KUnit；Gate 2 raw synchronous-wait/exit oracle与cutover。

## 状态所有权与生命周期

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 终止条件 |
| --- | --- | --- | --- |
| timer ID / generation / episode / arm | 创建者 `ThreadGroup` POSIX timer object | Signal 持 immutable episode identity | delete、exec、last-member exit；物理arm还可因dequeue rearm、flush无rearm或failed exact enqueue转换 |
| target membership 与 task lifecycle | topology / target task exit | timer 持不延长`Task`生命周期的non-rebinding identity capability | target exit 关闭 registration admission 后 detach |
| task-private timer registration / pending | exact `Task::sig_pending` | timer 持窄 enqueue/withdraw capability | fetch、flush、target exit 或 registration withdrawal |
| wake / reserved delivery / signal frame | Signal 与 exact task delivery path | timer 不持有 | action commit、no-frame cleanup 或 task exit |
| timer occurrence completion / delivery overrun | Signal分类`Dequeued`/`Flushed`；timer计算live delivery snapshot | 锁外窄handoff携带identity并把finalized overrun贡献返回dequeued occurrence | 对同一episode完成一次；flush无delivery snapshot |

Create 的成功提交要求 timer reservation 和 task-private registration 同时可由各自 cleanup owner 找到；
timer ID copyout/publication 的既有 fail-forward/rollback 顺序不因 target mode 改变。Expiry publication 后
Signal 临时拥有 occurrence；timer delete 只能撤销未来 enqueue，不能从 Signal 私有容器抢回已发布 occurrence。
task exit对pending occurrence的flush只完成该episode且不更新最近交付overrun，也不产生下一次物理arm；
尚未到期或已经dequeue/rearm的arm继续到expiry，再以exact-target failure停止。periodic timer之后保留的
`timer_gettime()`未来投影不是新的调度所有权或物理request。

## RFC-local Proof Obligations

- Gate 1 可以建立尚未被 syscall 使用的 internal private-registration capability，但它必须在 Gate 2 被真实
  POSIX timer consumer 使用；若 Gate 2 不 cut over，该 capability 与测试 facade 必须删除，不能沉淀为
  dormant production API。
- 在 `PT-THREAD-ID-CUTOVER` 前，`SIGEV_THREAD_ID` 必须继续返回显式 unsupported error；不能在 Signal
  内部能力尚未闭合时提前接受 ABI。
- current contracts 在 Gate 2 的代码、RFC 专属测试和 RV64 oracle 满足后原子 Refine；LA64 runtime 本轮按
  维护者授权为 `Not Run / waived`，不伪写成通过；Gate 0/1 不建立 transitional current contract。
- Gate 2 的 exact-route oracle 必须让target/decoy都保持signal blocked，decoy在expiry时独占等待、target
  存活但延后fetch；两者同时竞争不能排除错误的shared publication。target-exit oracle必须观察新的type-4
  registration稳定返回`EINVAL`后再判断exit阶段，
  不能把`SYS_exit`前的用户态flag或固定sleep当作lifecycle线性化证据。
- Gate 2 的periodic oracle必须让occurrence跨过多个interval后dequeue，并在同一次delivery上比较
  `siginfo.si_overrun`与紧随其后的`timer_getoverrun()`；case不得包含replace/republication，且必须得到相同
  非零值。只断言零值不能证明dequeue-finalization协议。
- Gate 2 的ignored-recovery oracle必须区分accrual和snapshot：`SIG_IGN`期间跨过多个interval时
  `timer_getoverrun()`保持上一次delivery值，恢复disposition后的首次真实delivery才提交包含ignored周期的
  非零overrun。

## 禁止退化项

- 不得把 exact target 只保存为数值 TID并在 expiry 时 lookup。
- 不得让 timer object缓存 target 的 signal mask、disposition、membership 或 pending 状态。
- 不得为复用现有 shared slot 而先发布 shared occurrence，再由 member selection 偏向目标 task。
- 不得用强 `Arc<Task>` 掩盖 exit race；允许稳定 identity capability 自身被拥有，但它不能维持 `Task`
  可执行/成员生命周期，也不得把 task-private timer cleanup推迟到 `Task::Drop`。
- 不得把 raw `SIGEV_THREAD` 静默解释为 `SIGEV_THREAD_ID`，或在内核调用 userspace callback pointer。
- 不得以单一应用 smoke 或 source inspection 取代 raw ABI/error/lifecycle oracle；本次单架构限制仅因维护者
  明确 waiver 生效，不改变 R0 target 或 correctness invariant。
