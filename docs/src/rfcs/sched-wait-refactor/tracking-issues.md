# Sched Wait Refactor Tracking Issues

**状态：** Closed（R0 post-close issues neutralized）
**最后更新：** 2026-07-25
**父 RFC：** [RFC-20260601-sched-wait-refactor](./index.md)
**原事务日志：** [2026-06-01 - Sched Wait Refactor](../../devlog/transactions/2026-06-01-sched-wait-refactor.md)

本文记录 R0 收口后发现、且需要由 wait-core owner 处理的共享协议问题。新增 issue 不改写 R0 已完成的 wait identity、logical completion、park latch 与 stale-safe placement 事实；若后续接受的修复改变 canonical contract，应按 RFC workflow 形成语义修订和新的 transaction。

## Keter

- 暂无。

## Euclid

- 暂无。

## Safe

- 暂无。

## Neutralized

### KETER-WAIT-001：synchronous remote placement 不能组合进 cross-CPU IPI completion

**状态：** Neutralized by `SCHED-WAKE` / 2026-07-25

**原问题：** `wake_wait()` 在 producer CPU 完成 logical wake 后立即执行 stale-safe physical placement；receiver 属于其它 CPU 时，旧 `remote_wake_enqueue()` 使用 synchronous IPI 等待 owner CPU 返回 placement result。两个 CPU 的 IPI handler 同时完成对方 wait 时可能各自等待反向 wake IPI。

**Resolution：** [SCHED-WAKE 当前契约](../../contracts/scheduler/wake-delivery.md)把 logical completion 后的 placement 定义为 scheduler obligation handoff。remote payload 持有 strong `Arc<Task>`，先进入 single-target owner-CPU queue 再 ring IPI；producer 在 transport 接管后返回，不等待 placement result。owner handler 直接执行 stale-safe local revalidation，physical classification 不再返回 consumer；dynamic scheduler request 的临时串行 gate 同时删除。

**Evidence / boundary：** source audit 确认 wake tail 不再调用 synchronous result transport、wake payload 不能 broadcast、wait identity / park latch / stale-safe owner revalidation 沿用 R0 closure；初赛 RV64 端到端普通启动覆盖 build、255 项 KUnit 与既有用户态回归。后继小迭代的 acceptance 有意不要求双 CPU race matrix，故旧关闭条件中的 bidirectional SMP stress 是 Not Run，不作为本次 closure evidence。allocation-free IRQ transport仍由 register issue 跟踪。
