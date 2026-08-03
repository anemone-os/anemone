# Timekeeper 当前契约

**Owner：** architecture clock source 与 common timekeeper
**覆盖范围：** 统一硬件计数、Hertz、monotonic 零点、realtime 读取偏移、coarse snapshot 与八个 native clock 的读值投影
**不覆盖：** realtime mutation/通知、soft timer 请求、sleep、POSIX timer、RTC 接入和 CPU usage 累计生命周期
**最后核验：** 2026-08-04

本目录只登记已经 cut over 的共享时间规则，不声称 RFC-20260803 的 timer、realtime step 或 POSIX timer
目标已经实现。

## Owner Boundary

- architecture clock source 独占 raw counter 的平台校正和稳定 Hertz；common timekeeper 只消费一个跨 CPU
  不后退的计数域。
- timekeeper 独占 boot counter、realtime read offset、dormant change sequence 和 coarse monotonic snapshot。
- task / thread group 继续独占 CPU usage 累计；clock route 只读取 snapshot。
- calendar consumer 只读取 timekeeper 的 realtime projection，不保存另一份可修改 realtime。

## Contract Surfaces

- [Clock derivation](./clock-derivation.md)：`TIMEKEEPER-CLOCK-001` 定义统一计数、整数换算、clock projection
  和分辨率。

## 邻接边界

- [Clock Timekeeping 与 POSIX Timers RFC](../../rfcs/clock-timekeeping-posix-timers/index.md)：尚未 cut over 的
  realtime step、soft timer 与 POSIX timer target。
- [ThreadGroup lifecycle](../task/thread-group-lifecycle.md)：CPU usage 和后续 POSIX timer cleanup 的 task owner
  边界。
