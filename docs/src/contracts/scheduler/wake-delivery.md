# Asynchronous Wake Delivery 当前契约

**Contract ID：** `SCHED-WAKE`
**状态：** Active
**Owner：** scheduler wake-delivery handoff protocol
**参与领域：** wait core / scheduler processor / IPI transport / Event / Latch / task lifecycle
**覆盖范围：** wait logical completion、runnable qualification、physical-placement obligation 的移交与 owner-CPU revalidation
**不覆盖：** Event listener queue、Latch source cleanup、signal pending routing、scheduler class policy、通用 synchronous IPI、CPU hotplug / migration、allocation-free IRQ transport
**实现位置：** `anemone-kernel/src/sched/{wait,processor,event,latch,request}.rs`、`anemone-kernel/src/exception/ipi.rs`、`anemone-kernel/src/task/{api/exit,topology/deferred}.rs`
**依赖：** task 的固定 `cpuid` owner、deferred task disposal；尚未提取为独立 contract ID
**Pending Successor：** None
**最后核验：** 2026-07-25

## 术语

- **Logical completion：** wait core 对当前 wait identity、mode 和 armed 状态的唯一事务，并在成功时同时提交 completion reason 与 `Waiting -> Runnable`。
- **Placement obligation：** logical completion 成功后，scheduler 必须在本地终结或可靠交给 task owner CPU 的 physical-placement 责任。
- **Physical placement：** owner CPU 对 task 当前 sched state、current identity 和 runqueue membership 重新验证后作出的本地决定。

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| 当前 wait identity、outcome、park state 与 task sched state | wait core 的 task sched-state 事务 | producer 持有受限 wake token / active-wait capability | 决定本次 completion 是否赢得当前 wait round |
| logical completion 后的 placement obligation | scheduler wake-delivery handoff | wait core 只调用 no-result submission 入口 | 本地终结或交给 owner CPU |
| remote message queue 与投递 | IPI transport | scheduler 提交 single-target payload | enqueue-before-ring 的可靠 handoff |
| outstanding remote task lifetime | wake IPI payload 的强 `Arc<Task>` capability；task lifecycle 仍由 deferred disposal 拥有 | handler 只借此定位稳定对象 | 避免 TID lookup / reuse 和 handler 成为最终 payload owner |
| physical placement result | owner CPU scheduler-local diagnostic | Event、Latch、signal、timer 与 exit 不可观察 | trace stale/current/queued/enqueued 分类 |

## SCHED-WAKE-001 — Logical completion 与 runnable qualification 同步提交

**规则：** `wake_wait()` / `wake_active_wait()` 只能在 task sched-state 的唯一 noirq write transaction 内验证当前 wait identity、wake mode 和 `Armed` 状态；成功时必须同时提交 `WaitState::Completed(reason)` 与 `TaskSchedState::Runnable`。事务释放后才允许提交 placement obligation。Event、Latch、signal、timer 与 exit 不得复制 completion state machine 或补调裸 enqueue。

**违反表现：** 旧 token 完成新 wait、completion reason 与 runnable qualification 分裂、consumer 维护第二份完成状态，或在 task-state guard 内进入 IPI / runqueue placement。

**验证 / Enforcement：** `sched/wait.rs` 的 completion transaction 与 `finish_wake_attempt()` source audit；历史 wait-core pre/post-park、late-token 和 stale-tail closure evidence；当前 build / KUnit / boot regression。

**最初来源：** [Sched Wait Refactor RFC](../../rfcs/sched-wait-refactor/invariants.md)；[原实现事务](../../devlog/transactions/2026-06-01-sched-wait-refactor.md)。

**当前来源：** [Asynchronous wake delivery 小迭代](../../devlog/changes/2026-07-25-asynchronous-wake-delivery.md)。

## SCHED-WAKE-002 — Placement handoff 本地终结或远端异步接管

**规则：** logical completion 成功后，wait core 调用 scheduler 的 no-result handoff。task 属于当前 CPU 时，scheduler 立即执行一次 stale-safe local placement；task 属于其它 CPU 时，scheduler 必须先把 single-target obligation 放入 owner CPU 的 IPI queue，再 ring IPI，并在 transport 接管后立即返回。producer 不等待 owner CPU 返回 placement result；remote wake delivery 不得建立反向 synchronous IPI completion 边。

**违反表现：** hardirq producer 在 remote wake tail 自旋等待 owner result、logical completion 后没有 placement owner、consumer 根据 producer context 选择两套 transport，或 remote request 需要全局串行 gate 才能避免双向互等。

**验证 / Enforcement：** `submit_wake_placement()`、`send_ipi_async()`、IPI queue/handler 与 `sched/request` source audit；初赛 RV64 build / KUnit / 普通 boot regression。双 CPU race matrix 不属于本次 cutover evidence。

**最初来源：** synchronous baseline 见 [Sched Wait Refactor 原实现事务](../../devlog/transactions/2026-06-01-sched-wait-refactor.md)；其组合缺口见 [KETER-WAIT-001](../../rfcs/sched-wait-refactor/tracking-issues.md#keter-wait-001synchronous-remote-placement-不能组合进-cross-cpu-ipi-completion)。

**当前来源：** [Asynchronous wake delivery 小迭代](../../devlog/changes/2026-07-25-asynchronous-wake-delivery.md)。

## SCHED-WAKE-003 — Remote obligation 持有强生命周期能力且提交后不可 rollback

**规则：** outstanding remote-placement payload 必须持有目标 `Task` 的强生命周期能力，直到 owner CPU 消费 obligation；handler 不得通过 numeric TID 或 topology lookup 重解析对象。logical completion 已提交后，target-offline 或 allocation/transport submission failure 是明确的 fatal invariant boundary，不得作为 ordinary `WakeResult` 返回、不得 rollback wait outcome，也不得提供 consumer compensation API。wake payload 只允许 single-target delivery，禁止 broadcast copy。

user task 和 kthread 在 unpublish / topology detach 前都先把强引用交给 deferred-disposal queue；disposer 只在该引用是唯一 strong owner 时释放。因此 payload outstanding 时 handler 释放 message 不会成为完整 `Task` 的最终 owner。这个证明只覆盖 wake payload 本身，不宣称全局 deferred-disposal 的 `Weak::upgrade()` 竞态或 IRQ-off allocation 已经关闭。

**违反表现：** async handler 使用 TID lookup、同一 obligation 被广播、transport error 暴露给 Event/Latch、已提交 completion 被改回 Waiting，或 wake payload 成为 task 的最终强 owner。

**验证 / Enforcement：** `IpiPayload::WakeUpTaskStaleSafe`、broadcast preflight、exit/kthread exit、topology unpublish 和 `dispose_deferred_tasks()` source audit；post-commit submission 使用常开 fatal boundary。

**最初来源：** synchronous producer-held lifetime baseline 见 [Sched Wait Refactor 原实现事务](../../devlog/transactions/2026-06-01-sched-wait-refactor.md)。

**当前来源：** [Asynchronous wake delivery 小迭代](../../devlog/changes/2026-07-25-asynchronous-wake-delivery.md)。

## SCHED-WAKE-004 — Owner CPU revalidation 与 consumer-visible logical result 分离

**规则：** owner CPU handler 必须直接进入 owner-local stale-safe placement，并断言 payload task 的固定 `cpuid` 属于当前 CPU。placement 在 IRQ-off processor transaction 中重新检查 task 是否仍 `Runnable`、是否为 current、是否已在 runqueue；stale/current/park-pending/already-queued/enqueued 只用于 scheduler-local 诊断。`WakeResult::Woke` 只表示 logical completion 成功且 scheduler 已接管 placement obligation，不表示 task 已进入 runqueue或已经执行。

Event 只按 `Woke` 消耗 exclusive quota；`ModeBlocked` 继续使用受限 requeue permit；Latch trigger 仍不向 producer返回结果。任何 consumer 都不得观察或补偿 physical placement。

**违反表现：** handler 重新进入可选 remote 分支、consumer 根据 placement 分类改变 quota / readiness、`Woke` 被解释成 task 已运行，或 stale tail 触发严格 new-task enqueue 断言。

**验证 / Enforcement：** owner-local helper assertion、processor stale-safe checks、全树 `WakeResult` consumer audit，以及保留的 Event mode-blocked / Latch no-return source boundary。

**最初来源：** [Sched Wait Refactor RFC](../../rfcs/sched-wait-refactor/invariants.md)；[Sched Latch RFC](../../rfcs/sched-latch/index.md)。

**当前来源：** [Asynchronous wake delivery 小迭代](../../devlog/changes/2026-07-25-asynchronous-wake-delivery.md)。
