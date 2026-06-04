# RFC-20260603-sched-latch

**状态：** 已接受，Protocol Closed，Stage 6 审计完成
**负责人：** doruche, Codex
**最后更新：** 2026-06-03
**领域：** scheduler / wait core / iomux / poll / select
**事务日志：** [2026-06-03 - Sched Latch](../../devlog/transactions/2026-06-03-sched-latch.md)
**开放问题：** None（文档协议层无 Still open plan gap；[Tracking Issues](./tracking-issues.md) 仅保留实现 gate）。
**下一步：** 核心 `ppoll` / `pselect6` latch OR wait 迁移已完成；后续 source 扩展、POLLPRI / exception readiness、epoll 或异步通知作为独立 follow-up 处理。

## 摘要

本 RFC 定义 `Latch` 原语：它是在 wait core 上提供的一轮 OR 等待组合器，面向 `poll` / `select` 这类 syscall-local consumer 同时等待多个 fd source 的场景。等待方创建一轮 wait core 等待，把同一轮触发能力分发给多个 producer；任意 producer 的第一发有效 trigger 完成本轮等待，旧 trigger 不能完成新一轮等待。

`Latch` 不引入新的调度真相源。wait identity、completion、timeout、signal、cancel、finish 和 stale-safe placement 仍由 wait core 负责；`sched::latch` 只把 waiter lifecycle 和 producer trigger capability 收窄成更适合 iomux OR wait 的接口。

## 背景

`Event` 已经迁移到 wait core 上，适合 futex、mutex、child exit、vfork completion 等“源对象拥有等待队列”的场景。`poll` / `select` 的等待形态不同：一个 syscall-local consumer 同时等待多个 fd 源，任意一个源变为 ready 都应完成本轮等待。

当前 `ppoll` / `pselect6` 的阻塞语义仍处在 stage-1 限制中，已登记的 iomux 睡眠可观测性问题要求后续用真实 wait 协议替代 busy polling。`Latch` 的目标是补上这个缺口：它基于 wait core 的 `ActiveWait` / `WakeToken` / `wake_wait()`，但把 producer 看到的接口收窄成 cloneable trigger，把 consumer 看到的接口收窄成单轮 wait lifecycle。

## 目标

- 定义 `poll` / `select` OR wait 所需的单轮次 wait identity、single-consumer、多 producer trigger 和 first-completion-wins 不变量。
- 固定 `Latch` 与 wait core、iomux、fd/device source、timeout、signal、cancel 之间的责任边界。
- 给出从原语、source 注册协议、pipe source、`ppoll`、`pselect6` 到旁路审计的迁移计划。
- 保留足够 debug / trace 可观测性，使 late trigger、stale placement、source cleanup 和 readiness race 可以被复审。

## 非目标

- 不替换 `Event`，也不把所有同步原语统一成 `Latch`。
- 不在本 RFC 中完整实现 Linux waitqueue、epoll、futex PI 或异步通知框架。
- 不引入跨等待轮次保存的 notification permit。
- 不引入 count-down latch、barrier 或 AND wait 语义。
- 不让 fd/device source 直接操作 task 调度状态或 runqueue。
- 不通过 busy polling 修复 `poll` / `select` 的阻塞语义。

## 文档地图

Canonical：

- [不变量需求](./invariants.md)
- [迁移实施计划](./implementation.md)
- [Tracking Issues](./tracking-issues.md)

背景材料：

- [背景材料索引](./backgrounds/index.md)

## 方案

`Latch` 表示“一轮等待的闭锁”。它不是 `Event`，不是 Tokio `Notify` 的长期 permit，也不是 count-down latch；它只封装一轮 wait core wait round 的 completion capability。

建议角色拆分为 waiter-owned `Latch` 和 producer-held `LatchTrigger`。`Latch` 本体由创建它的 consumer 持有，不可 clone、不可跨 task 转移使用；`LatchTrigger` 可以 clone 给多个 producer，但只暴露 no-return / fail-closed 的 trigger 能力，不暴露 `WakeToken` 本体或 wait-core lifecycle API。

`poll` / `select` 的注册协议采用 snapshot scan -> begin latch -> register scan -> schedule -> finish -> final snapshot scan 的结构。source 注册必须在自己的状态锁下同时检查 readiness 并保存 trigger；source wake 必须在同一状态锁下完成 predicate update 与 trigger detach，释放 source lock 后再触发 detached trigger。被唤醒只表示“可能有源 ready”，syscall 返回前必须重新检查实际 predicate。

## 接受边界

本 RFC 已作为 `sched-latch` 迁移的 canonical source 被接受：实现可以调整类型名和模块路径，但不能改变单轮 wait identity、single-consumer owner boundary、producer trigger capability、source 注册窗口、source wake detach 线性化点、cleanup 非正确性支柱、final readiness scan、以及 wait core stale-safe placement 责任。

当前 [Tracking Issues](./tracking-issues.md) 中没有 Still open plan gap；其中保留的问题均为实现阶段必须验收的 gate 或已由 RFC text 关闭的记录。实现过程中如果发现会改变 wait identity、completion 线性化点、状态所有权、锁序、source cleanup 语义、producer capability 边界或 syscall outcome mapping 的内容，必须先更新本 RFC 或新增 follow-up RFC，再声明对应阶段退出。

## 备选方案

### 复用 `Event`

拒绝。`Event` 是可重复发布的事件源，自己维护 listener 队列；`poll` / `select` 需要的是 syscall-local OR wait round。把所有 fd source 通过临时 `Event` 转发会混淆 source 所有权和 wait round lifecycle。

### 使用跨轮次 notify permit

拒绝。`poll` / `select` wake 只是 readiness hint，返回前必须重扫 predicate；保存跨轮次 permit 会让旧通知自动影响新一轮 wait，破坏 old trigger fail-closed。

### 继续 busy polling

拒绝。busy polling 不能提供 Linux 风格可观察睡眠状态，也不能解决 `ppoll(NULL timeout)` / `pselect6` 在 LTP 中依赖睡眠状态的问题。

### 首版只做 pipe 私有 waiter

延期。可以先迁移 pipe 作为最小 source，但 source 私有 waiter 不能成为协议真相；最终仍需要统一的 `Latch` / `PollRequest` 注册边界。

## 风险

- 半迁移 source 未保存 trigger 而 syscall 仍进入 schedule，会造成永远睡眠。控制方式是 tracking issue 中的“未 armed 不得阻塞” typed register gate。
- source wake 侧 predicate update 与 trigger detach 不在线性化点内，会造成 lost wake。控制方式是在不变量和实施验收中硬化 source lock 规则。
- strong trigger 如果残留在持久 source 队列中，可能长期保活 retired wait state。控制方式是在 source 注册协议落地前确定 weak/strong 策略、资源上界和 pruning gate。
- timeout、signal、ready race 如果由 `ppoll` / `pselect6` 分别解释，会造成 syscall 语义分裂。控制方式是统一 final scan 与 outcome mapping。

## 收口

当前文档协议已经收口；实现阶段 gate 已按事务日志完成阶段 6 审计：

1. [Tracking Issues](./tracking-issues.md) 中的 implementation gate 已有阶段退出证据。
2. 每个阶段的交付、审计、验证和剩余限制已追加到 [事务日志](../../devlog/transactions/2026-06-03-sched-latch.md)。
3. 旧 iomux stage-1 睡眠可观测性限制已在 [current limitations](../../register/current-limitations.md) 标记为 resolved。

如果后续审查新增 Still open plan gap，必须先回到本文档集收口；不得把协议空洞推迟到实现时再决定。
