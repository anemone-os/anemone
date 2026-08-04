# Realtime Step 当前契约

**Contract ID：** `TIMEKEEPER-STEP-001`
**状态：** Active
**Owner：** common timekeeper（step protocol）与 per-CPU soft timer request service（request recheck）
**参与领域：** timekeeper、clock syscall、soft timer、scheduler wait-core、timerfd
**覆盖范围：** realtime offset/change-seq 原子提交、锁外全 CPU request 重检、absolute realtime sleep、timerfd absolute realtime 与 cancel-on-set
**不覆盖：** POSIX timer ID/overrun/signal pending、CPU-time timer、RTC seed/writeback、suspend accounting、频率修正和渐调
**实现位置：** `anemone-kernel/src/{time/timekeeper.rs,time/clock,time/timer,fs/timerfd/mod.rs,sched/mod.rs}`
**依赖：** [`TIMEKEEPER-CLOCK-001`](./clock-derivation.md#timekeeper-clock-001--所有-clock-读取来自一条整数推导链)、[`SOFT-TIMER-REQUEST-001`](./soft-timer-request.md#soft-timer-request-001--排队句柄物理删除一次请求)
**最后核验：** 2026-08-04

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| realtime offset 与 nonwrapping change sequence | timekeeper | 原子的 realtime/seq snapshot、锁外发布 token | 读取 calendar time 并标识实际 step |
| 一次 monotonic 或 realtime 排队请求 | request 所在 CPU soft timer queue | opaque `TimerHandle` | 到期、物理删除和 execution-lane 交接 |
| absolute realtime sleep wait outcome | scheduler wait-core | realtime request 持窄 timeout trigger | step 到期或 signal 完成原 wait round |
| timerfd deadline、cancel snapshot、cancelled 与 expiry count | `TimerFdCore` | realtime request 持 deadline/seq/generation snapshot | direct read/poll refresh、`ECANCELED` 和 stale completion 拒绝 |

request 中的 change sequence 是 owner-to-queue 的不可变协议 snapshot，不是第二份 current time。每 CPU queue
保存的 observed sequence 只说明该 queue 已检查到哪个 step；它必须单调前进，不能反向决定 offset 或 timerfd
对象状态。

## TIMEKEEPER-STEP-001 — realtime step 不能漏掉或误用旧 timeline

**规则：** `clock_settime(CLOCK_REALTIME)` 和支持的 `clock_adjtime(ADJ_SETOFFSET)` 只修改 timekeeper 的
nonnegative realtime offset。完整输入、输出范围和权限验证必须在 mutation 前完成。timekeeper 在同一锁内
采样 monotonic、验证结果、提交 offset，并且仅在 offset 实际变化时把 change sequence 加一；sequence
耗尽时 fail closed，不回绕复用。

mutation 返回后 timekeeper 锁已经释放。随后 step publisher 扫描所有 online CPU 的 realtime queue；任一时刻
只持有一个 queue lock，锁外才把到期或 clock-change completion 交给请求原 CPU 的 threaded lane。timekeeper
锁内不得获取 queue、wait、timerfd 或 signal owner，也不得执行 callback 或唤醒 task。

absolute realtime request 在登记前取得一致的 realtime/change-seq snapshot，并把 deadline 与可选
cancel-on-change sequence 交给一次 queue-owned request。请求插入后必须用最新 timekeeper snapshot 重检，关闭
“snapshot 后 step、scanner 先完成、request 后登记”的窗口。queue 的 observed sequence 只能前进；已经落后于
observed sequence 的 scanner 既不能取消请求，也不能用旧 realtime 值使请求到期。向前 step 越过 deadline 时
请求到期；向后 step 保留请求并按新 timeline 等待。

相对 sleep、相对 timerfd 和 `ITIMER_REAL` 在提交时固定为 monotonic deadline，不受 realtime step 影响。
absolute realtime timerfd 进入 realtime queue；只有 realtime + absolute 请求允许
`TFD_TIMER_CANCEL_ON_SET`。step 选择 cancellation 时，queue 物理移除该次请求，再由 `TimerFdCore` 的 generation
确认当前 arm、清除 schedule/expirations、发布 cancelled readiness。下一次 read 返回一次 `ECANCELED`；成功的
`timerfd_settime()` 清除 cancelled 状态。已经进入 threaded lane 的旧 completion 只能由 generation 拒绝，不能
改变 replacement。

**违反表现：** offset 与 sequence 分两次发布；offset 未变化仍推进 sequence；timekeeper 锁内扫描 queue 或
唤醒 waiter；只检查 syscall 当前 CPU；snapshot/登记之间的 step 被漏掉；旧 scanner 在 backward step 后提前
到期请求；cancel-on-set 只推进 generation 而把请求留在 queue；相对 sleep/timerfd 随日期修改提前或延后。

**验证 / Enforcement：** owner-local KUnit 覆盖 mutation no-side-effect/overflow、native timex layout、realtime
heap 双向判断、cancel sequence、旧 scanner、insert-side recheck、remote CPU recheck、timerfd 非法 flags、
relative realtime 固化、物理 cancel、一次 `ECANCELED` 和 stale replacement。2026-08-04 最终源码在 RV64/LA64
release SMP=2 分别通过 425/425 与 426/426 KUnit；同源用户 oracle 覆盖 set/adj 权限与错误、相对/绝对三种
sleep clock、signal/remaining、realtime 双向 step、relative/absolute realtime timerfd 和 cancel-on-set。
source audit确认 timekeeper 锁内没有 queue/wait/timerfd callback，两个架构的 pretest rootfs 均从对应 manifest
重新生成。完整证据见 cutover transaction；mdBook 按开发者指示 Not Run。

**最初来源：** [Clock Timekeeping 与 POSIX Timers RFC R0](../../rfcs/clock-timekeeping-posix-timers/index.md)
的 `TC-STEP-CUTOVER`。

**当前来源：** [2026-08-04 Gate 3 transaction](../../devlog/transactions/2026-08-04-clock-timekeeping-posix-timers.md#gate-3-closure-与-tc-step-cutover--2026-08-04)。
