# Event Wake Race 问题简述

日期：2026-05-31

## 问题

在 `Event::listen*()` 的等待循环与 `publish()` 的唤醒交错时，旧一轮 wake 的尾巴可能晚于 waiter 进入下一轮等待，最终让 `task_enqueue()` 作用到一个状态已变化的 task 上。

这类交错会触发调度路径上的断言失败，表现为随机 panic 或用户测试不稳定。

## 影响

1. 影响 `event` / `scheduler` 的并发正确性。
2. 干扰 `user-test` 和 LTP 回归判断。
3. 问题核心是唤醒归属和入队时序，而不是单纯的条件检查缺失。

## 当前方案

当前采用的完整方案是 `WaitState/WakeToken` 协议。它把“一轮等待”的身份、完成原因和退役状态集中到 `WaitState`，由 `WakeToken` 作为只读交付句柄，避免把轮次状态分散在 `Event`、`Listener` 和 `TaskStatus` 之间。

该方案同时要求 wake 后的物理入队走 stale-safe placement，并把 event wake、timeout、signal 和主动 cancel 收敛到同一个 wait core。这样才能同时证明旧 listener 不能逻辑完成新一轮等待，旧 wake 尾巴也不能把新一轮 `Waiting` task 投入 runqueue。

此前评估过的 wake-tail 专用条件入队只能作为窄化止血方案：它不引入等待轮次身份，因此不能单独证明协议闭合。

## 相关材料

- [Sched Wait Refactor 不变量需求](../invariants.md)
- [Sched Wait Refactor 迁移实施计划](../implementation.md)
- [Event WaitState/WakeToken 问题清单](./event-waitstate-waketoken-issues.md)
- [Event WaitState/WakeToken 2026-06-01 单文件归档](./event-waitstate-waketoken-plan-monolith.md)
- [Event wake-tail 条件入队窄化修复草案（历史备选）](./event-try-task-enqueue-narrow-fix.md)
