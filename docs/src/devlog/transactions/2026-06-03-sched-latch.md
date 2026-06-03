# 2026-06-03 - Sched Latch

**Status:** Active
**Owners:** doruche, Codex
**Area:** scheduler / wait core / iomux / poll / select
**RFC:** [RFC-20260603-sched-latch](../../rfcs/sched-latch/index.md)
**Current Phase:** Agent 0.5 wait-core surface hardening complete; awaiting Agent 1 gate decision

## Scope

本事务跟踪 `sched-latch` 迁移：在 scheduler wait core 上建立 single-consumer / multi-producer 的一轮 OR wait 原语，并把 `poll` / `select` 从 busy polling 迁移到真实 wait 协议。

本事务覆盖：

- `sched::latch` 原语；
- `fs::iomux` 的 typed source register protocol；
- pipe 作为首个 source 的迁移；
- `ppoll` 与 `pselect6` 的共享 latch loop、final scan 和 outcome mapping；
- `PollWaiter` / `poll_waiters` / `yield_now()` 等旧 busy-poll 旁路审计；
- rv64 / LTP 中依赖 poll/select 睡眠可观测性的验证证据。

非目标：

- 不替换 `Event` 或把所有同步原语统一成 `Latch`。
- 不实现完整 Linux waitqueue、epoll、futex PI 或异步通知框架。
- 不引入跨等待轮次保存的 notification permit。
- 不引入 count-down latch、barrier 或 AND wait 语义。

## Invariants

- 一个 `Latch` 只对应一轮 wait core wait identity。
- 同一轮 `Latch` 只有一个 consumer 持有 waiter lifecycle。
- 同一轮 `Latch` 可以派生多个 producer trigger。
- producer trigger、timeout、signal、force 和 consumer cancel 竞争同一个 `WaitState`。
- 任何完成本轮 wait 的路径都由 wait core 负责逻辑完成和 stale-safe physical placement。
- source 注册必须在 source lock 下同时检查 readiness 并保存 trigger。
- source wake 必须在同一 source lock 临界区内完成 predicate update 与 trigger detach，释放 source lock 后再 trigger。
- wake 只是 readiness hint；`ppoll` / `pselect6` 返回前必须重新 final readiness scan。
- 未 armed source 不得让 syscall 进入 latch schedule。

## Handoff

**Last Updated:** 2026-06-03

**Current Branch:** `dev/drc/latch`

**Canonical RFC:** [RFC-20260603-sched-latch](../../rfcs/sched-latch/index.md), [Invariants](../../rfcs/sched-latch/invariants.md), [Implementation Plan](../../rfcs/sched-latch/implementation.md), [Tracking Issues](../../rfcs/sched-latch/tracking-issues.md)

**Completed:** `etc/plans/sched-latch` 草稿已提升为公开 RFC；文档协议审查未发现新的 Apollyon / Keter 级硬障碍；软件工程审查结果已作为实现工程指导写入 implementation gate；事务日志、事务索引、双周 devlog 和 mdBook Summary 已建立链接。Agent 0.5 已完成 wait-core surface hardening：`crate::sched::*` 不再 re-export `ActiveWait`、`WakeToken`、`WaitReason`、`WaitOutcome`、`WakeResult` 或 `WaitStateStatus`；clock / signal syscall adapter 改走受限 current-wait wrapper；`Event` 仍作为 scheduler 内部合法 wait adapter 直接使用 wait core。

**Open Blockers:** 当前没有 Still open plan gap。所有原 Keter 风险均作为 implementation gate 保留。Agent 0.5 只收窄 wait lifecycle / token surface；`TaskSchedState` 和 `Task::update_sched_state_with()` 仍是 crate-internal scheduler-state surface，完全封住普通 source 写 task sched state 需要后续单独阶段扩大 write set 到 `task/sched.rs` 等 owner 文件。

**Next Action:** 总控可重新判定是否进入 Agent 1。Agent 1 仍需实现 `sched::latch`，并按 RFC gate 记录每阶段交付、审计和验证；不要把 Agent 0.5 的 restricted current-wait wrapper 当成 `Latch` 或 `LatchTrigger`。

**Do Not Redo:** 不要重新把 `etc/` 个人草稿作为 canonical source；不要把 `PollWaiter` / `poll_waiters` 草稿扩展成新的 waitable poll 协议；不要让 `ppoll` 与 `pselect6` 分裂 outcome mapping；不要把未迁移 source 当成 armed source。

## Phase Log

### 2026-06-03 - RFC 提升与事务日志启动

**Phase:** planning / RFC promotion

**Change:** 将 `etc/plans/sched-latch` 的已收口内容提升到 [docs/src/rfcs/sched-latch](../../rfcs/sched-latch/index.md)，并建立本事务日志。RFC 目录包含入口、[不变量需求](../../rfcs/sched-latch/invariants.md)、[迁移实施计划](../../rfcs/sched-latch/implementation.md)、[Tracking Issues](../../rfcs/sched-latch/tracking-issues.md) 和背景材料索引。

**Change:** RFC 页首、Tracking Issues、事务日志索引、mdBook Summary 和当前双周 devlog 均已建立公开链接。后续实现记录写入本事务日志，不再引用个人 `etc/` 草稿作为 canonical source。

**Review:** 协议层多线审查结论为：当前没有新的 Apollyon / Keter 硬障碍。单轮 wait identity、owner-bound `Latch`、producer no-return / fail-closed capability、source register gate、source wake detach、cleanup 非正确性支柱、final readiness scan 和 `ppoll` / `pselect6` 统一 outcome mapping 均已在 RFC 中闭合。

**Engineering Guidance:** 软件工程审查发现的维护性风险已落到实施计划：`PollWaiter` / `poll_waiters` 是旧草稿形状，不得继续扩展为新协议；pipe source 迁移时必须清理无效 waiter 队列；`ppoll` / `pselect6` 应共享 latch loop / outcome helper，避免后续漂移。

**Validation:** 本阶段只更新文档结构，未修改代码，未运行构建或 QEMU / LTP。

**Next:** 阶段 1 建立 `sched::latch` 原语，并记录 wait-core placement 前置审计、old trigger late arrival debug hook 或最小单测结果。

### 2026-06-03 - Agent 0.5 wait-core surface hardening

**Phase:** pre-Agent-1 hardening / KETER-005 blocker closure

**Change:** 收窄 `sched::mod` 的 wait-core re-export：`ActiveWait`、`WakeToken`、`WaitReason`、`WaitOutcome`、`WakeResult` 和 `WaitStateStatus` 不再作为 `crate::sched::*` 的普通外部能力暴露。raw begin / clone token / cancel / finish / producer wake result 现在只在 `sched` 子模块内部可见。

**Change:** 新增受限的 current-task wait adapter：`CurrentWaitPrecheck`、`CurrentWaitOutcome` 和 `wait_current_with_timeout()`。`clock_nanosleep` 与 `rt_sigtimedwait` 继续通过 wait core 创建、取消、timeout wake、finish 本轮等待，但调用点不再持有 `ActiveWait`、`WakeToken` 或任意 `WaitReason`。`rt_sigtimedwait` 保留 pending signal 的消费语义：precheck 命中的 signal 保存在 syscall 本地结果中，wrapper 只负责取消并 retire 本轮 wait。

**Change:** `Event` 保留为 scheduler 内部合法 wait adapter，显式使用 `super::wait` 和内部 timeout helper。`Event::publish()` 的 producer completion 仍走 `wake_wait()`，signal / force `notify()` 仍走 `wake_active_wait()`，timeout callback 仍通过 wait identity 调 `wake_wait(..., Timeout, AnyWait)`，因此 logical completion 与 stale-safe placement 合同未改变。

**Review:** 这一步满足 Agent 0 停止条件中的 wait lifecycle surface blocker：普通 fd/device source 不能再通过 `crate::sched::*` 取得 `ActiveWait` / `WakeToken` 来直接创建、clone token、cancel、finish wait round 或读取 `WakeResult` 分支。当前改动没有新增 `Latch` / `LatchTrigger`，没有新增第二套 armed/completion 状态，也没有改变 wait identity、completion 线性化点、Event 语义、timeout/signal/force 路径或 syscall outcome mapping。

**Boundary:** `TaskSchedState` 仍通过 `sched::*` 暴露，因为 `Task` 内部字段、exit 路径、processor placement 和 wait core 仍直接依赖它；`Task::update_sched_state_with()` 也仍是 crate-internal 写入口。Agent 0.5 的 write set 不包含 `task/sched.rs` / `task/mod.rs` / exit / processor owner 文件，因此本阶段只把 wait lifecycle capability 从 fd source 面前拿掉；彻底封住普通 source 写 task sched state 需要后续单独 gate。

**Validation:** `just build` 通过。构建期间仅保留既有 warning：`anemone-kernel/src/sync/mono.rs` 中 `AtomicBool` / `Ordering` 未使用；本阶段未修改该文件。

**Next:** 总控可基于本 hardening 重新判定是否派发 Agent 1。Agent 1 仍需按 RFC 阶段 1 实现 owner-bound `Latch`、no-return / fail-closed `LatchTrigger`、受限 cancel reason 和 trigger 生命周期策略。

## Open Items

- 阶段 1：建立 `sched::latch`，确认 owner-bound `Latch`、no-return `LatchTrigger`、受限 cancel reason 和 exactly-once finish / retire 策略；不要重新向 `crate::sched::*` 暴露 wait-core raw lifecycle。
- 阶段 2：定义 typed `PollRequest` / `PollRegisterResult`，移除、私有化或废弃 `PollWaiter` 草稿入口。
- 阶段 3：迁移 pipe source，清理 `poll_waiters` 残留或替换为明确 latch trigger queue。
- 阶段 4：迁移 `ppoll`，固定可复用的 latch loop / final scan / outcome helper。
- 阶段 5：迁移 `pselect6`，复用 `ppoll` 的等待协议。
- 阶段 6：旁路审计 `PollRequest`、`PollWaiter`、`poll_waiters`、`yield_now()`、wait-core wake 调用和 source trigger queue，并执行 rv64 / LTP 验证。
