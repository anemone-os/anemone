# Time 当前契约

**Owner：** architecture clock source、common timekeeper 与 soft timer request service
**覆盖范围：** 统一硬件计数、Hertz、clock 读值投影，以及一次 soft timer 排队请求和取消协议
**不覆盖：** realtime mutation/通知、完整 sleep 路由、POSIX timer、RTC 接入和 CPU usage 累计生命周期
**最后核验：** 2026-08-04

本目录只登记已经 cut over 的共享时间规则，不声称 RFC-20260803 的 realtime step、完整 clock sleep 或
POSIX timer 目标已经实现。

## Owner Boundary

- architecture clock source 独占 raw counter 的平台校正和稳定 Hertz；common timekeeper 只消费一个跨 CPU
  不后退的计数域。
- timekeeper 独占 boot counter、realtime read offset、dormant change sequence 和 coarse monotonic snapshot。
- task / thread group 继续独占 CPU usage 累计；clock route 只读取 snapshot。
- calendar consumer 只读取 timekeeper 的 realtime projection，不保存另一份可修改 realtime。
- soft timer request service 只拥有每 CPU queue 中的一次请求和排队 identity；timerfd、itimer 与 wait-core
  继续拥有长期 schedule、周期、到期累计和 wait outcome。

## Contract Surfaces

- [Clock derivation](./clock-derivation.md)：`TIMEKEEPER-CLOCK-001` 定义统一计数、整数换算、clock projection
  和分辨率。
- [Soft timer request](./soft-timer-request.md)：`SOFT-TIMER-REQUEST-001` 定义一次请求、排队句柄、物理删除
  和已出队 stale completion 交接。

## 邻接边界

- [Clock Timekeeping 与 POSIX Timers RFC](../../rfcs/clock-timekeeping-posix-timers/index.md)：尚未 cut over 的
  realtime step、完整 clock sleep 与 POSIX timer target。
- [ThreadGroup lifecycle](../task/thread-group-lifecycle.md)：CPU usage 和后续 POSIX timer cleanup 的 task owner
  边界。
