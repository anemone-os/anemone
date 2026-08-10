# Soft Timer Request 当前契约

**Contract ID：** `SOFT-TIMER-REQUEST-001`
**状态：** Active
**Owner：** per-CPU soft timer request service
**参与领域：** time/timer、threaded timer completion、scheduler wait-core、timerfd、`ThreadGroup::ITIMER_REAL`
**覆盖范围：** 一次排队请求、opaque 排队句柄、物理删除、IRQ 出队与已出队 stale completion 交接
**不覆盖：** realtime mutation/step notification、absolute realtime timer、`TFD_TIMER_CANCEL_ON_SET`、POSIX timer ID/overrun/signal pending、tickless 或 high-resolution timer
**实现位置：** `anemone-kernel/src/{time/timer,fs/timerfd,task/itimer.rs,sched/mod.rs}`
**依赖：** [`TIMEKEEPER-CLOCK-001`](./clock-derivation.md#timekeeper-clock-001--所有-clock-读取来自一条整数推导链)
**最后核验：** 2026-08-04

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| 到期时刻、event identity、IRQ/threaded lane 与一次 callback | event 所在 CPU 的 soft timer queue | opaque `TimerHandle` | 排序、出队或物理删除一次请求 |
| timerfd schedule、generation 与 expiry count | `TimerFdCore` | 当前排队句柄、callback generation snapshot | replace/disarm/close cleanup 与 stale completion 拒绝 |
| `ITIMER_REAL` 目标、周期、validness 与 signal commit | `ThreadGroup::ITimers` | 当前排队句柄、callback validness snapshot | replace/disarm/teardown cleanup 与 `SIGALRM` |
| wait outcome 与 round identity | scheduler wait-core | 当前排队句柄、`WakeToken` | wait 返回时删除请求，已出队 callback 只能尝试完成原 round |

soft timer 不保存或推进长期对象的第二份 schedule、周期、expiry count、signal 或 wait outcome。

## SOFT-TIMER-REQUEST-001 — 排队句柄物理删除一次请求

**规则：** 每次 enqueue 创建一个位于提交 CPU 队列中的 one-shot request，并返回字段私有、不可复制且
`must_use` 的排队句柄。event identity 在整个 boot 中不回绕复用；identity 空间耗尽时 fail closed，旧句柄
不得命中新请求。句柄只定位 queue-owned request，不表示长期 timer 对象或 callback 已执行状态。

cancel 只锁定句柄记录的一个 owner CPU queue。若请求仍在队列中，cancel 原子移除它并返回 `true`；若请求已被
删除或已经出队到 IRQ/threaded execution lane，则返回 `false`。禁止同时持有两个 CPU queue 锁。删除得到的
callback 必须在 queue 锁释放后析构。

timer IRQ 在本 CPU queue 锁内只批量移出已经到期的请求；锁外才执行 IRQ callback，或把 threaded callback
交给本 CPU completion lane。queue 锁内不得调用长期对象、wait、signal 或外部 callback。

长期 owner 在 replace、disarm、wait return 和 teardown 时必须先让旧 generation、validness 或 wait identity
失效，并物理删除仍在 queue 中的请求。cancel 返回 `false` 时，这些 owner-local identity 只负责拒绝已经出队的
旧 completion；generation 不能代替 queue cleanup。周期目标和 missed-expiry accounting 仍由长期 owner 从原
目标推进，soft timer 只承载每次下一到期请求。

**违反表现：** replace/delete 只推进 generation 而让远期 callback 留在 queue；event ID 回绕导致旧句柄删除
新请求；remote cancel 同时持有两个 CPU queue 锁；queue 锁内执行或析构 callback；timer core 保存 timerfd/
itimer/wait 的第二份长期状态；周期从 callback 实际运行时间重新起算。

**验证 / Enforcement：** owner-local KUnit 覆盖同 deadline identity 顺序、任意 heap 删除、ID exhaustion、
重复删除、IRQ 出队后 cancel、锁外 callback drop、remote CPU cancel、远期反复 schedule/cancel、wait 提前唤醒、
timerfd replace/disarm/refresh/last-close、`ITIMER_REAL` replace/disarm/teardown、POSIX timer反复
replace/disarm/delete/bulk cleanup和周期原目标推进。2026-08-04 R0最终源码在RV64/LA64 release SMP=2分别通过
469/469与470/470 KUnit；真实wait、timerfd、`ITIMER_REAL`与POSIX timer用户态oracle通过。双架构完整证据记录在
cutover transaction。

**最初来源：** [Clock Timekeeping 与 POSIX Timers RFC R0](../../rfcs/clock-timekeeping-posix-timers/index.md)
的 `ST-REQUEST-CUTOVER`。

**当前来源：** [2026-08-04 Gate 2 transaction](../../devlog/transactions/2026-08-04-clock-timekeeping-posix-timers.md#gate-2-closure-与-st-request-cutover--2026-08-04)。
