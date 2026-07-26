# Epoll 背景材料

本目录保存 [RFC-20260726-epoll](../index.md) 的历史定位输入。背景材料不覆盖当前公共 Draft target，不构成 current contract、implementation plan、write set 或执行授权。

RFC target：

- [RFC 入口](../index.md)
- [目标与不变量](../invariants.md)
- [迁移实施计划](../implementation.md)
- [Tracking Issues](../tracking-issues.md)

受影响的当前契约：

- [Scheduler Latch wait round](../../../contracts/scheduler/latch-wait-round.md)
- [Poll wait 与 source registration](../../../contracts/iomux/poll-wait.md)
- [Opened-description lifecycle](../../../contracts/task/opened-description-lifecycle.md)

历史材料：

- [RFC 前设计定位共识](./positioning.md)：保留 source subscription、epoll owner、opened-description lifecycle 和备选方案的早期推导；当前结论以父 RFC 为准。
