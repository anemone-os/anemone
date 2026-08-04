# POSIX Timer 当前契约

**Contract ID：** `POSIX-TIMER-001`
**状态：** Active
**Owner：** `ThreadGroup` POSIX timer table/object
**参与领域：** time / soft timer / task / signal / syscall ABI
**覆盖范围：** native 64-bit timer 107--111、ID/object生命周期、timeline、周期/overrun与`SI_TIMER`交接
**不覆盖：** compat 403--409、CPU-time timer、`SIGEV_THREAD(_ID)`、alarm clock、time namespace或high-resolution timer
**实现位置：** `anemone-kernel/src/task/posix_timer.rs`、`anemone-kernel/src/time/posix_timer/`、`anemone-kernel/src/task/sig/`
**依赖：** `TIMEKEEPER-CLOCK-001`、`TIMEKEEPER-STEP-001`、`SOFT-TIMER-REQUEST-001`、`SIGNAL-PENDING-001`、`TASK-LIFE-002`
**最后核验：** 2026-08-04

## POSIX-TIMER-001 — ThreadGroup唯一拥有timer对象、ID与通知episode

**规则：** timer表publication/removal是当前ThreadGroup内syscall查找ID的唯一真相。create在ID copyout前只
保留带identity的不可见reservation；失败回滚，旧reservation不能影响清表后复用的ID。delete先移除ID，再推进
generation、物理取消仍排队request、撤销signal registration；已经出队的callback或pending signal只能按旧identity
结束，不能恢复对象。fork建立空表，成功exec和最后member exit执行同一owner-local批量删除协议。

每个arm只保存一个实际timeline和一个原目标。relative realtime/monotonic/boottime都换算到monotonic；只有
absolute realtime保留calendar target并进入realtime queue。periodic target从上一个目标按整数周期推进，不从
callback执行时间重算；同一timer notification pending期间不创建第二份episode，完整错过次数累加到该episode，
delivery时固化为`timer_getoverrun()`结果并在`INT_MAX`钳位。

soft timer只拥有一次queue request；signal owner独占pending、disposition、mask、member selection、wake与frame。
timer expiry只提交timer ID、generation、episode、overrun和sigval，并消费typed enqueue outcome；signal在释放
pending/ThreadGroup guard后回告immutable identity。不同timer选择同一standard signal时保留独立pending slot；普通
standard signal仍按`SIGNAL-PENDING-001`合并。

**违反表现：** 全局ID namespace；reservation无identity覆盖reused ID；relative realtime随calendar step提前或
延后；`callback_now + interval`漂移；generation-only取消；timer读取signal私有pending；fork继承timer；exec/exit
只清表不取消request；不同timer的同号`SI_TIMER`互相覆盖。

**验证 / Enforcement：** POSIX timer owner-local KUnit覆盖ID、周期/overrun及反复replace/disarm/delete/bulk cleanup；
signal pending slot/identity/flush KUnit；2026-08-04 R0最终源码在RV64/LA64 release SMP=2分别通过469/469与
470/470 KUnit。双架构native user oracle覆盖五syscall、rollback/fail-forward、relative/absolute realtime step、
periodic overrun、同号多timer、fork与unmaskable signal；source audit覆盖exec/exit与lock/callback顺序。

**最初来源：** [RFC-20260803 Clock Timekeeping与POSIX Timers](../../rfcs/clock-timekeeping-posix-timers/index.md)；
[Gate 5 PT-SIGNAL-CUTOVER](../../devlog/transactions/2026-08-04-clock-timekeeping-posix-timers.md#gate-5-closure-与-pt-signal-cutover--2026-08-04)。
