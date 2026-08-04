# Time 当前契约

**Owner：** architecture clock source、common timekeeper、soft timer request service 与 ThreadGroup POSIX timer
**覆盖范围：** 统一硬件计数、Hertz、clock 读值投影、realtime step、一次 soft timer 请求与 POSIX timer 生命周期
**不覆盖：** RTC 接入、CPU-time timer 和 CPU usage 累计生命周期
**最后核验：** 2026-08-04

本目录只登记已经 cut over 的共享时间规则。R0 最终消费者审计已经关闭；RTC seed 仍是该 revision 的明确
non-goal，不属于当前时间 contract。

## Owner Boundary

- architecture clock source 独占 raw counter 的平台校正和稳定 Hertz；common timekeeper 只消费一个跨 CPU
  不后退的计数域。
- timekeeper 独占 boot counter、realtime offset/change sequence 和 coarse monotonic snapshot；step publisher 只
  消费锁外 token。
- task / thread group 继续独占 CPU usage 累计；clock route 只读取 snapshot。
- calendar consumer 只读取 timekeeper 的 realtime projection，不保存另一份可修改 realtime。
- soft timer request service 只拥有每 CPU queue 中的一次请求和排队 identity；timerfd、itimer 与 wait-core
  继续拥有长期 schedule、周期、到期累计和 wait outcome。
- `ThreadGroup` POSIX timer表/对象独占ID、arm、周期、generation与overrun；soft timer和signal只消费窄能力。

## Contract Surfaces

- [Clock derivation](./clock-derivation.md)：`TIMEKEEPER-CLOCK-001` 定义统一计数、整数换算、clock projection
  和分辨率。
- [Realtime step](./realtime-step.md)：`TIMEKEEPER-STEP-001` 定义 offset/sequence 原子提交、锁外全 CPU 重检
  和 snapshot/登记无丢失协议。
- [Soft timer request](./soft-timer-request.md)：`SOFT-TIMER-REQUEST-001` 定义一次请求、排队句柄、物理删除
  和已出队 stale completion 交接。
- [POSIX timer](./posix-timer.md)：`POSIX-TIMER-001` 定义ID publication、timeline、周期/overrun、signal handoff
  与fork/exec/exit cleanup。

## 邻接边界

- [Clock Timekeeping 与 POSIX Timers RFC](../../rfcs/clock-timekeeping-posix-timers/index.md)：已关闭的 R0 target、
  owner boundary 与 non-goal。
- [ThreadGroup lifecycle](../task/thread-group-lifecycle.md)：CPU usage与POSIX timer cleanup的task owner边界。
