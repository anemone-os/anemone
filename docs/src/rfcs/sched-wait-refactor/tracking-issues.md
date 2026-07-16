# Sched Wait Refactor Tracking Issues

**状态：** Active（R0 post-close review）
**最后更新：** 2026-07-15
**父 RFC：** [RFC-20260601-sched-wait-refactor](./index.md)
**原事务日志：** [2026-06-01 - Sched Wait Refactor](../../devlog/transactions/2026-06-01-sched-wait-refactor.md)

本文记录 R0 收口后发现、且需要由 wait-core owner 处理的共享协议问题。新增 issue 不改写 R0 已完成的 wait identity、logical completion、park latch 与 stale-safe placement 事实；若后续接受的修复改变 canonical contract，应按 RFC workflow 形成语义修订和新的 transaction。

## Keter

### KETER-WAIT-001：synchronous remote placement 不能组合进 cross-CPU IPI completion

**状态：** Keter / Open / Post-close follow-up

**问题：** `wake_wait()` 在 producer CPU 完成 logical wake 后立即执行 stale-safe physical placement；receiver 属于其它 CPU 时，当前 `remote_wake_enqueue()` 使用 synchronous IPI 等待 owner CPU 返回 placement result。普通 task-context producer 可以在等待期间继续接收中断，但若 producer 本身运行在 IPI hardirq handler，两个 CPU 同时完成对方的 wait 时可能各自在 handler 内等待反向 wake IPI，形成不可恢复的双向等待。

**Owner boundary：** wait core 继续拥有 wait identity、logical completion、park state 与 stale-safe placement；IPI subsystem 继续拥有 message transport。消费方可以用局部串行 gate 限制自己的 hardirq producer graph，但不能把该约束描述成 wait-core 已支持任意 cross-CPU hardirq completion，也不能复制 wait state 或自行补偿入队。

**Allocation boundary：** 当前 IPI message 和 queue node 的 IRQ-off allocation 继续服从内核已有的 fatal OOM 接受边界。本 issue 不要求把 OOM 改造成可恢复错误，也不要求消费方预留 message、实现 rollback 或引入 allocation-free transport。

**关闭条件：** 由 wait-core owner 明确 hardirq producer 的 remote placement delivery contract，使 cross-CPU completion 不再依赖 handler 内同步互等；若修复改变 logical/physical completion 边界、placement return contract 或 IPI transport ownership，先更新 `index.md` / `invariants.md` 并建立新 transaction。验证至少覆盖两个 CPU 同时完成对方 wait、pre-park/post-park、stale tail 与 consumer-local serialization 移除后的回归。

## Euclid

- 暂无。

## Safe

- 暂无。

## Neutralized

- 暂无。
