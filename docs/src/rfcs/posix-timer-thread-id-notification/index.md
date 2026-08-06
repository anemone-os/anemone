# RFC-20260804-posix-timer-thread-id-notification

**状态：** Closed
**修订：** R0
**负责人：** doruche, Codex
**最后更新：** 2026-08-06
**领域：** time / POSIX timer / task / signal / syscall ABI
**影响契约：** Refine [`POSIX-TIMER-001`](../../contracts/time/posix-timer.md#posix-timer-001--threadgroup唯一拥有timer对象id与通知episode) 与
[`SIGNAL-PENDING-001`](../../contracts/signal/pending-routing.md#signal-pending-001--directed-occurrence-只进入对应-pending-owner)
**执行记录：** None

## 摘要

当前 `timer_create()` 只接受 `SIGEV_NONE` 与 `SIGEV_SIGNAL`。请求 Linux notification type 4
(`SIGEV_THREAD_ID`) 的程序会得到 `EOPNOTSUPP`；Vim 9.1 的
`E1286: Could not set timeout: Operation not supported` 是其中一个可见症状，但不作为语义权威。

本 RFC 增加 Linux native 64-bit `SIGEV_THREAD_ID`：timer 仍由创建者的
`ThreadGroup` 拥有，通知 registration 则精确绑定同一 `ThreadGroup` 内的一个 task
identity，到期 signal 只进入该 task 的 private pending owner。内核不执行用户回调；
本 RFC 只定义 Linux syscall、timer 与 signal delivery 的内核可见语义。

## 背景

现行 [`POSIX-TIMER-001`](../../contracts/time/posix-timer.md) 已定义 timer ID、arm、
generation、overrun、删除与 shared `SI_TIMER` notification episode；
[`SIGNAL-PENDING-001`](../../contracts/signal/pending-routing.md) 则把现有 POSIX timer slot
放在 `ThreadGroup` shared pending owner 中。旧
[Clock Timekeeping 与 POSIX Timers RFC](../clock-timekeeping-posix-timers/index.md) 已按其
R0 目标关闭，并明确把 `SIGEV_THREAD(_ID)` 排除在当前能力之外。本 RFC 是独立 follow-up，
不修改旧 RFC 的 closure 或历史证据。

Linux native `struct sigevent` 把 `SIGEV_THREAD_ID` 的 target TID 放在尾部 union 的第一个
`i32`；Linux 创建路径解析该 TID，并要求目标与调用者属于同一 thread group。到期路径按
exact task 投递，不能退化为 process-directed signal。ABI 与可见行为依据分别见
`xref:linux-6.6.32:include/uapi/asm-generic/siginfo.h#sigevent_t`、
`xref:linux-6.6.32:kernel/time/posix-timers.c#good_sigevent`、
`xref:linux-6.6.32:kernel/time/posix-timers.c#posix_timer_event` 与
`xref:linux-6.6.32:kernel/signal.c#send_sigqueue`。

对本地固定 Linux 6.6.32 source 的核对还确认：timer 保存的是不会维持 `task_struct` 存活的稳定
PID identity；`send_sigqueue()` 以 `PIDTYPE_PID` 选择 task-private pending；周期 timer 成功发布
notification 后停止物理排队，只有 `dequeue_signal()` 才在 signal lock 外 rearm；若 live disposition
直接忽略 notification，`posix_timer_fn()` 则立即推进 interval 并 rearm。目标在 expiry 前退出时，
当前物理 arm 保留到 expiry，exact send 失败后停止；目标在 occurrence 已 pending 时退出，
`__exit_signal()` 只 flush private queue，不发生 dequeue/rearm，因此没有“下一次物理 expiry”；若
occurrence 已 dequeue/rearm，下一次物理 expiry 才会因 exact target 不存在而失败。对于未物理排队但
仍处于 periodic requeue-pending 的 timer，`common_timer_get()` 仍按 interval 向未来投影
`timer_gettime()`，不能把该投影误写成真实 rearm。已经排队的 notification 在 `timer_delete()` 后仍可由
Signal 完成交付，只是 dequeue 时不能重新激活已删除 timer。Linux raw syscall 还会接受
`SIGEV_THREAD` 并按 process-directed signal 处理；本 RFC 明确保留 Anemone 的 unsupported 边界，不把它
伪装成 `SIGEV_THREAD_ID`。

`posix_timer_fn()` 的 ignored 路径虽然不提交delivery snapshot，但每次`hrtimer_forward()`仍累计
`it_overrun`；恢复非ignored disposition后的第一次真实dequeue才把该累计值提交到`si_overrun`与最近交付
snapshot。因此“ignored立即rearm”不能实现成清空本轮累计；在真实delivery前，`timer_getoverrun()`仍保持
上一次delivery值。依据见`xref:linux-6.6.32:kernel/time/posix-timers.c#posix_timer_fn`与
`xref:linux-6.6.32:kernel/time/posix-timers.c#posixtimer_rearm`。

Linux 的 dequeue 还有一个容易被 owner handoff 隐藏的可见要求：`dequeue_signal()` 先从 private/shared
pending 取出 `kernel_siginfo`，再在 signal lock 外调用 `posixtimer_rearm()`；后者推进 periodic target，
同时更新 timer 的最近交付 overrun snapshot 和即将暴露给 userspace 的 `info->si_overrun`。因此 Anemone
不必复制 Linux 的 `sigqueue` 或锁实现，但 live periodic timer 的一次 dequeue 必须在 siginfo copyout/frame
之前得到 dequeue-finalized overrun，让 `si_overrun` 与 `timer_getoverrun()` 观察同一次delivery提交；Linux
允许`si_overrun`再叠加already-pending sigqueue base count，因此本RFC只在没有replace/republication的受控
单episode中要求两者数值相等。依据见
`xref:linux-6.6.32:kernel/signal.c#dequeue_signal` 与
`xref:linux-6.6.32:kernel/signal.c#send_sigqueue`、
`xref:linux-6.6.32:kernel/time/posix-timers.c#timer_overrun_to_int`、
`xref:linux-6.6.32:kernel/time/posix-timers.c#posixtimer_rearm`。

## 目标

- `timer_create()` 接受 native 64-bit `SIGEV_THREAD_ID`，解析 union 中的 target TID，
  并要求它解析到调用者当前 `ThreadGroup` 中仍可建立 registration 的 exact member。
- 每个 thread-directed POSIX timer registration 在目标 task 的 private pending owner 中
  拥有独立 `SI_TIMER` slot；同 signal number 的不同 timer 不互相合并。
- 到期只通知注册时选定的 exact task；mask、disposition、job-control generation、wake、普通
  trap-return delivery、`rt_sigtimedwait` 同步消费、signal frame 与 temporary-mask handoff 继续
  由 Signal owner 决定。
- 目标 task 退出、timer 删除、exec、last-member exit、pending fetch/flush 与 in-flight expiry
  在明确顺序下完成，不能泄漏 slot、悬挂 target 或恢复已删除 timer。
- 用 raw syscall oracle 在 RV64、LA64 上证明 ABI 与 Linux 可见语义，并用 Vim 作为非权威集成 smoke。

## 非目标

- 不支持 raw `SIGEV_THREAD`，不在内核调用 `sigev_notify_function`，也不在内核创建 userspace
  callback thread。Linux 6.6.32 raw syscall 会接受该值并按 process-directed signal 处理；保留
  `EOPNOTSUPP` 是本 RFC 有意、可观测且带 notice 的范围差异。
- 不增加 compat/time32 timer syscall、CPU-time timer、alarm clock、timerfd notification
  mode 或新的 timer ID namespace。
- 不改变 `SIGEV_SIGNAL` 的 process-directed shared pending 语义，也不把普通 task-directed
  standard signal 改成 per-source queue。
- 不借此 RFC 定义完整 `rt_sigtimedwait` contract；只闭合 exact-task timer slot 被现有同步 wait
  消费时的 identity、completion 与 copyout 可见顺序。
- 不在目标退出后把 notification 改投 `ThreadGroup`、leader、任意未屏蔽 member 或后来复用
  相同数值 TID 的 task。

## Owner 与协议边界

- **Timer owner：** 创建者的 `ThreadGroup` POSIX timer table/object 继续唯一拥有 ID、clock、
  arm、generation、episode、overrun 与 delete/exec/last-member cleanup。
- **Target identity / membership owner：** task topology 与 `ThreadGroup` membership 决定 TID
  是否解析到调用者同组的 exact `Task`。registration 保存 non-rebinding identity capability；该
  capability 自身可以有稳定生命周期，但不能延长 `Task` 的可执行/成员生命周期。数值 TID 只服务
  ABI 解析与诊断，不能在到期时重新查找 target。
- **Pending owner：** `SIGEV_SIGNAL` registration 继续使用 `ThreadGroup` shared pending；
  `SIGEV_THREAD_ID` registration 与 pending episode 由 exact `Task::sig_pending` 唯一拥有。
  Timer owner 只能持有窄 registration capability 和 typed enqueue/completion outcome；普通
  delivery 与 `rt_sigtimedwait` 都消费同一 private occurrence。
- **Handoff / 线性化点：** `timer_create()` 在同组 membership 与 task-private registration
  admission 同时仍有效时建立 slot；timer expiry 把 immutable timer identity 交给该 slot；
  pending publication 后 Signal 拥有 occurrence，fetch/flush/exit retirement 在释放 Signal
  guard 后把同一 identity与typed `Dequeued`/`Flushed` reason回告timer owner。普通delivery和同步wait
  dequeue必须在siginfo对userspace可见前完成timer-owner handoff：live periodic timer把等待期间新增的
  expirations合入delivery overrun，dequeued occurrence的`si_overrun`与timer最近交付snapshot在同一次
  handoff中提交，然后才rearm；没有replace/republication的单episode中两者必须相等。flush只撤销未交付
  episode，不能伪装成userspace delivery。
- **Failure / cancellation / cleanup：** 稳定的无效/异组 TID，或 Anemone target 已关闭 admission
  时，create 返回 `EINVAL`且不发布 timer ID；并发 target exit 可以在 registration commit 前失败，
  也可以先成功绑定原 identity，但不能绑定后来复用相同 TID 的 task。目标 task 退出时必须先关闭新
  registration admission，再从 membership detach，并在 Signal guard 外 retire private pending/
  reserved delivery。该 retirement 以`Flushed`结束pending episode，不更新最近交付overrun，也不执行
  delivery-driven rearm：未到期的物理 arm 继续到 expiry 后因 exact send 失败而停止；已经 pending 的
  occurrence 被 flush 后没有新的物理 arm；已经 dequeue/rearm 的 occurrence 则保留新 arm 到下次 failed
  expiry。timer delete 仍先移除 ID，再使 generation
  失效、取消 queued request、撤销 registration；已经由 Signal 拥有的 occurrence 可以继续交付旧
  `SI_TIMER`，但只能完成旧 identity，不能恢复 timer。

## ABI 与可见语义

- native RV64/LA64 `SigEvent` 保持 64 bytes、8-byte alignment；`sigev_value`、
  `sigev_signo`、`sigev_notify` 与 union 的 offset 分别保持 0、8、12、16。target TID 是 union
  offset 16 的首个 `i32`，必须通过 ABI accessor 解码，不能让 raw union 布局进入 core owner。
- 成功 notification 的 `siginfo` 保持 `si_code == SI_TIMER`，`si_value` 等于 create 时的
  `sigev_value`，`si_tid` 是 timer ID而不是 target TID。periodic occurrence 的`si_overrun`必须包含从
  首次enqueue到实际dequeue之间错过且可归属于该delivery的周期，并按`INT_MAX`钳位；当timer在dequeue时
  仍有效，`timer_getoverrun()`必须读取同一次dequeue提交的最近交付snapshot。在没有timer replace或
  already-pending republication的受控单episode中，两者数值必须相等；不得据此抹掉Linux允许siginfo叠加
  occurrence-local base count的边界。
- `sigev_notify == SIGEV_THREAD_ID (4)` 时仍按现行 signal-number 规则校验
  `sigev_signo`。稳定的 target TID 不存在、不属于调用者 `ThreadGroup`，或 admission 已关闭时返回
  `EINVAL`。并发 exit 的 success/`EINVAL` race 结果不承诺固定时点，但成功结果必须绑定原 identity。
- `SIGEV_THREAD_ID` 只接受精确值 4（`SIGEV_SIGNAL | SIGEV_THREAD_ID` 因
  `SIGEV_SIGNAL == 0` 仍是 4）；其它未知或混合 notification bits 返回 `EINVAL`，不能按 unsupported
  capability 归类，也不能部分解释。这一 strict decode 对应 Linux `good_sigevent()` 的 exact switch。
- 成功创建后，signal 只进入注册时 exact task 的 private pending。目标退出的 Linux 可见顺序按
  occurrence 所处阶段区分：尚未 expiry 时保留当前物理 arm，到期 exact send 失败后不再物理 rearm；
  已 pending 时 exit flush occurrence，且因为没有 dequeue，周期 timer 不产生下一次物理 arm；已经
  dequeue/rearm 时保留新的物理 arm，到期 exact send 失败后停止。任何阶段都不得重新解析数值 TID 或
  改投其它 member。
- `timer_gettime()` 保持 Linux 的表示：尚未 expiry 或已经 dequeue/rearm 时反映物理 arm；periodic
  occurrence 被 flush 或 exact send 失败后，`it_interval` 保留，`it_value` 仍按 requeue-pending 的原
  expiry/interval 投影到未来，即使没有物理 request；oneshot 在 occurrence 被 flush 或 failed expiry 后
  返回零。`timer_getoverrun()` 的最近交付 snapshot 不因 flush 或 failed send 更新。
- expiry outcome 的周期转换固定如下；Anemone 的 `Consumed` 只服务现行 `SIGSTOP` scoped exception，
  不能成为其它 notification 的隐式成功路径：

  | Outcome | Pending / delivery commit | Periodic physical rearm |
  | --- | --- | --- |
  | `Queued` / `AlreadyPending` | Signal 持有或更新 occurrence；等待 dequeue | 不立即 rearm |
  | `Dequeued` | 在userspace可见前提交finalized `si_overrun`与最近交付snapshot | Signal guard 外 rearm |
  | `Ignored` | 不产生 occurrence、不更新最近交付snapshot，但保留本轮overrun accrual | expiry 路径立即 rearm；恢复 disposition 后首次delivery提交累计值 |
  | `Consumed` | 按 `JOBCTL-SIGNAL-001` 提交 `SIGSTOP` control consumption 与对应 overrun | 立即继续 periodic arm |
  | `Flushed` / `TargetExited` | 不提交最近交付 overrun | 不 rearm |

- `timer_delete()` 不 recall 已经由 Signal 拥有的 private `SI_TIMER`；它仍可被 exact task 的普通
  delivery或同步 wait消费，但 stale dequeue completion不能 rearm 已删除/复用 timer。
- `SIGEV_THREAD` 继续返回 `EOPNOTSUPP` 并打印 notice。Linux raw syscall 对值 2 的 process-directed
  接受不等于 kernel-side callback；这个显式差异防止把 signal delivery 错写成 callback execution。

## Contract Impact

| Contract ID | 变化 | 当前规则 | Target 摘要 | Cutover |
| --- | --- | --- | --- | --- |
| `POSIX-TIMER-001` | Refine | Timer 由 `ThreadGroup` 拥有，通知只支持 none 或 shared process-directed `SI_TIMER` | Timer owner 不变；notification target扩展为shared process-directed或exact task-directed；queued等待dequeue rearm，dequeue在userspace可见前finalize overrun，ignored保留accrual并立即继续periodic arm，flush/target-exited不rearm；target exit按未到期、pending/flush、已dequeue/rearm三阶段闭合，failed send后保持Linux `timer_gettime()`投影 | `PT-THREAD-ID-CUTOVER` |
| `SIGNAL-PENDING-001` | Refine | POSIX timer registration slot 只在 shared pending owner 中 | exact task的private pending owner可持有per-registration `SI_TIMER` slot，由普通delivery或`rt_sigtimedwait`消费；锁外typed dequeue handoff必须在copyout/frame前取得finalized overrun，flush不提交snapshot，并保持ordinary standard signal合并与realtime arrival ordering不变 | `PT-THREAD-ID-CUTOVER` |

### Dependencies

- [`TASK-LIFE-002`](../../contracts/task/thread-group-lifecycle.md#task-life-002--最后-member-detach-后才能发布-exited)：task owner-local cleanup 先于 membership detach。
- [`SIGNAL-ACTION-001/002`](../../contracts/signal/pending-routing.md#signal-action-001--ignored-disposition-在-pending-publication-前生效)：ignored admission 与普通 trap-return action selection 保持由 Signal owner 决定。
- [`SIGNAL-TEMP-MASK-001/002/003`](../../contracts/signal/temporary-mask-delivery.md)：claim 后的 task-private reserved delivery 与退出 cleanup 不建立第二份 occurrence truth。
- [`JOBCTL-SIGNAL-001`](../../contracts/task/job-control.md#jobctl-signal-001--control-signal-generation与jobctl提交同序)：thread-directed timer 使用 control signal number 时仍服从现行 group-wide generation ordering。
- [`SOFT-TIMER-REQUEST-001`](../../contracts/time/soft-timer-request.md#soft-timer-request-001--排队句柄物理删除一次请求)：排队 request 的取消与 callback 边界不变。

## Implementation Boundary

- **允许改变：** native `SigEvent` 的 `_tid` ABI accessor、`timer_create()` notification 解码、
  POSIX timer notification capability、Signal owner 内的 task-private timer slots、task exit
  cleanup、`rt_sigtimedwait` 对同一 private slot 的同步消费、owner-local KUnit 与双架构用户态 oracle。
- **必须保持：** `ThreadGroup` timer ownership/ID namespace、timekeeper 与 soft-timer contract、
  `SIGEV_SIGNAL` shared routing、普通 signal 合并与 action selection、job-control owner、
  temporary-mask handoff、现有 delete/exec/last-member ordering，以及 raw `SIGEV_THREAD` 的
  unsupported 边界。
- **实现提示：** 预计触及 `anemone-abi` 的 time UAPI、kernel POSIX timer ABI/core、task signal
  pending/timer handoff、task exit 与定向测试；这些位置非穷举 write set，也不授权扩大 public API。
- **停止条件：** 若实现需要强引用维持 `Task` 可执行/成员生命周期、在 expiry 时按数值 TID 重新查找、把 private
  occurrence 同时复制到 shared pending、从 timer owner 读取 Signal 私有容器、改变 job-control/
  temporary-mask contract、接受异组 target、降低双架构 runtime oracle，必须回到 RFC review 或
  Target Renegotiation，不能执行 cutover。

## Acceptance 与 Validation

- RFC 接受只批准本页 target、owner、ABI、contract delta 与 gate 顺序，不自动授权任何 Gate。
- source/KUnit 证明 ABI offset、strict notification decode、同组校验、private slot identity、mask/ignore/
  job-control、ignored后恢复disposition、完整outcome/rearm矩阵、普通delivery与`rt_sigtimedwait`同步消费、
  dequeue-finalized `si_overrun`/`timer_getoverrun()`同源提交、ignored期间accrual延迟到下一次真实delivery、
  dequeue-vs-flush overrun、目标退出、
  delete/flush/in-flight callback、
  TID reuse 与普通 signal regression。
- RV64 release build/KUnit 与 raw syscall oracle 必须覆盖成功 exact delivery、异组/
  不存在 TID 的 `EINVAL`、unknown/mixed notify的`EINVAL`、concurrent-exit两种合法closure、private
  `rt_sigtimedwait`、blocked/unblocked target、target-exit三个阶段、periodic物理停止与
  `timer_gettime()`投影、删除后已排队 signal仍可消费，以及同号 ordinary task-directed realtime signal
  与 timer occurrence 的 arrival ordering。exact-delivery用例必须让target与decoy都保持signal blocked，
  decoy在expiry窗口独占同步等待、target仍存活但延后开始等待；decoy timeout且target随后取得occurrence才能
  排除shared fallback。target-exit用例必须以
  新type-4 registration稳定返回`EINVAL`确认admission已经关闭，不能用`SYS_exit`前的用户态flag加固定sleep
  代替。periodic用例必须延迟dequeue跨过多个interval，并同时断言收到的`si_overrun`和紧随其后的
  `timer_getoverrun()`；该case不得插入replace/republication，必须得到相同的非零值，不能只覆盖零overrun。
  ignored-recovery用例还必须让timer在`SIG_IGN`期间跨过多个interval，确认delivery前
  `timer_getoverrun()`保持旧值、恢复disposition后的首次`SI_TIMER.si_overrun`和最近交付snapshot包含该累计。
- RV64 必须通过 raw oracle；Vim 只作为集成 smoke 证明原始 `E1286` 症状消失，不能替代 raw
  ABI、errno、exact-delivery 与 lifecycle 证据。LA64 runtime 本轮按维护者授权为 `Not Run / waived`。
- 文档检查按仓库流程执行；本任务显式跳过全部 mdBook 检查。

## 风险与反馈

最大的实现风险是 task exit 与 registration publication 竞态，以及 Signal pending retirement 回调
重入 timer owner 导致锁序回环。对应 proof obligations 见[目标与不变量](./invariants.md)，分阶段路线、
验证与 hard stop 见[实施路线](./implementation.md)。任何证据若要求改变 target identity、owner、
failure errno、cleanup 或 cutover 强度，必须先回写 RFC review，不能把较弱能力写成完成。

## 文档与证据

- [目标与不变量](./invariants.md)
- [实施路线](./implementation.md)
- commit / PR / optional transaction：None
- 外部源码证据：`xref:linux-6.6.32:include/uapi/asm-generic/siginfo.h#sigevent_t`、
  `xref:linux-6.6.32:kernel/time/posix-timers.c#good_sigevent`、
  `xref:linux-6.6.32:kernel/time/posix-timers.c#posix_timer_event`、
  `xref:linux-6.6.32:kernel/time/posix-timers.c#posix_timer_fn`、
  `xref:linux-6.6.32:kernel/time/posix-timers.c#timer_overrun_to_int`、
  `xref:linux-6.6.32:kernel/time/posix-timers.c#posixtimer_rearm`、
  `xref:linux-6.6.32:kernel/time/posix-timers.c#common_timer_get`、
  `xref:linux-6.6.32:kernel/signal.c#send_sigqueue`、
  `xref:linux-6.6.32:kernel/signal.c#dequeue_signal`、
  `xref:linux-6.6.32:kernel/signal.c#flush_sigqueue` 与
  `xref:linux-6.6.32:kernel/exit.c#__exit_signal`

## 修订记录

- **R0（2026-08-05）：** 接受 native `SIGEV_THREAD_ID` exact-task delivery target；以固定 Linux
  6.6.32 的 create/expiry/private-pending/dequeue/flush/gettime 语义为依据，Vim仅作为集成smoke。
- **R0 clarification（2026-08-06）：** 固化 dequeue-finalized `si_overrun` 与最近交付snapshot的同源
  可见语义，并收紧exact-route和target-exit raw oracle；target、owner、ABI范围与contract delta未改变。

## Closure

Closed — 2026-08-06。Gate 2 已完成 `PT-THREAD-ID-CUTOVER`；Gate 3 已完成基于 `etc/linux-6.6.32` 的
THREAD_ID 语义与架构静态审计。RV64 板上证据沿用 Gate 2；LA64 runtime 按维护者授权为 `Not Run / waived`，
不影响本次静态 closure。未运行 mdBook。
