# RFC-20260601-sched-wait-refactor

**状态：** 已完成
**负责人：** doruche, Codex
**最后更新：** 2026-06-03
**领域：** scheduler / event / timer / signal / wait core
**事务日志：** [2026-06-01 - Sched Wait Refactor](../../devlog/transactions/2026-06-01-sched-wait-refactor.md)
**开放问题：** None
**下一步：** None

## 摘要

本 RFC 定义 `Event` wake race 的共享修复计划：通过稳定等待轮次身份、单一 wait completion 事务、stale-safe wake placement、park latch handoff 和受约束的 Event listener requeue，把旧 wake tail 撞上新 wait round 的竞态收敛到一套可审查协议中。

公开 canonical 文档是：

- [不变量需求](./invariants.md)：必须保持的协议边界和证明义务。
- [迁移实施计划](./implementation.md)：落地顺序、审计范围、可观测性、验证和停止边界。

历史 review 材料保存在 [背景材料](./backgrounds/index.md) 下。它们解释原始 race、被否决的窄化方案和 issue 收口过程，但不覆盖 canonical 不变量和实施计划。

## 背景

当前旧 wait/wake 路径允许旧一轮 wake tail 晚于 waiter 进入下一轮等待。此时旧 tail 可能尝试把状态已经重新变成 `Waiting` 的 task 入队，从而触发 scheduler 的 `Runnable` 断言，或造成 user-test 不稳定。

根因不是缺少某个局部条件检查，而是旧协议缺少：

- 稳定等待轮次身份；
- 唯一逻辑 wake/cancel 线性化点；
- wake completion 后的 stale-safe physical placement；
- `schedule()` 与 wake tail 之间的 park latch handoff；
- mode-blocked Event listener 的受约束回挂协议。

## 目标

- 定义证明旧 listener 和旧 wake tail 不能完成或入队新 wait round 所需的不变量。
- 给出避免半套协议中间态的迁移计划。
- 明确 Event、timeout、signal、cancel 和 wake placement 的子系统责任边界。
- 保留足够 debug/trace 可观测性，支持后续 race 复审。

## 非目标

- 不重写调度策略、调度类、时间片行为或负载均衡。
- 不在本 RFC 中完成 futex PI、poll/epoll 或完整 Linux waitqueue 兼容。
- 不通过弱化 `task_enqueue()` 断言掩盖竞态。
- 不让个人 `etc/` 笔记成为公共 canonical 引用。

## 文档地图

Canonical：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)

背景材料：

- [背景材料索引](./backgrounds/index.md)
- [Wait/Wake Race 问题简述](./backgrounds/wake-race-problem-brief.md)
- [WaitState/WakeToken 问题清单](./backgrounds/waitstate-waketoken-issues.md)
- [Wake-tail 条件入队窄化修复草案](./backgrounds/wake-tail-narrow-fix.md)
- [WaitState/WakeToken 单文件归档](./backgrounds/waitstate-waketoken-plan-monolith.md)

## 接受边界

本 RFC 已作为 scheduler wait refactor 的实现计划来源被接受，并已完成对应事务日志跟踪。

如果后续实现发现会改变 wait identity、线性化点、状态所有权、锁序、listener requeue、exclusive quota、stale-safe placement 或 wait-core capability 边界的内容，必须先更新本 RFC 或新增 follow-up RFC，再声明协议闭合。
