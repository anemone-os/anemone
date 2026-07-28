# Scheduler Latch Wait Round 当前契约

**Contract ID：** `SCHED-LATCH`
**状态：** Active
**Owner：** waiter-owned `Latch` adapter；wait identity 与 completion truth 仍由 scheduler wait core 拥有
**参与领域：** scheduler wait core / iomux / pollable sources
**覆盖范围：** 单轮 OR wait identity、consumer lifecycle、producer trigger capability 与 stale-safe completion
**不覆盖：** source readiness predicate、source registration lifetime、epoll persistent interest、physical wake transport 细节
**实现位置：** `anemone-kernel/src/sched/latch.rs`、`anemone-kernel/src/sched/wait.rs`
**依赖：** `SCHED-WAKE-001..004`
**Pending Successor：** None
**最后核验：** 2026-07-26

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| wait identity、armed/completed/retired outcome 与 task wait state | scheduler wait core | `Latch` 持有受限 active-wait capability | 决定本轮 completion 胜者 |
| consumer begin/cancel/schedule/finish lifecycle | 创建本轮的 current-task `Latch` | source 不可取得 | exactly-once retire 本轮等待 |
| producer completion attempt | cloneable `LatchTrigger` | source 只持有 no-return hint capability | 请求 wait core 完成本轮并触发重扫 |
| wait id / task id 日志字段 | scheduler-local diagnostic | source 可用于日志 | 不参与 correctness decision |

## SCHED-LATCH-001 — 每个 Latch 只拥有一个 wait-core round

**规则：** 一个 `Latch` 必须对应一个新的 wait-core identity，并由创建它的 current task 单独持有 begin/cancel/schedule/finish lifecycle。consumer handle 不可 clone、不可跨 task 使用；每次 begin 后必须 exactly-once finish/retire。trigger、timeout、signal、force 与 cancel 竞争同一 wait-core state，不得另建 `armed` 或 completion truth。

**违反表现：** 一个 consumer round 复用旧 token、多个 owner 推进 lifecycle、drop 遗留 active wait，或 Latch 与 wait core 各自保存 completion state。

**验证 / Enforcement：** `Latch` 私有字段、`!Send` / `!Sync` owner boundary、常开 owner/lifecycle assertions 与 wait-core source audit。

**最初来源：** [Sched Latch RFC](../../rfcs/sched-latch/invariants.md)；[实现事务](../../devlog/transactions/2026-06-03-sched-latch.md)。

**当前来源：** live `sched::latch` implementation，2026-07-26 源码核验。

## SCHED-LATCH-002 — Producer 只持有 no-return completion capability

**规则：** `LatchTrigger` 可以 clone 给多个 producer，但只允许对本轮 identity 发起 no-return、fail-closed trigger。它不得向普通 source 暴露 `WakeToken`、强 `Task`、waiter lifecycle 或可用于行为分支的 `WakeResult`。旧、重复、已 retired trigger 由 wait-core identity 判定为 stale/retired；source cleanup 只服务资源卫生，不是 correctness 支柱。

**违反表现：** source 根据 wake result 补做 enqueue、直接推进 task sched state、旧 trigger 完成新 round，或依赖 entry 及时删除避免误唤醒。

**验证 / Enforcement：** `LatchTrigger` public API、wait token visibility、source trigger consumers 与 stale/retired logging audit。

**最初来源：** [Sched Latch RFC](../../rfcs/sched-latch/invariants.md)；[实现事务](../../devlog/transactions/2026-06-03-sched-latch.md)。

**当前来源：** live `LatchTrigger` capability surface，2026-07-26 源码核验。

## SCHED-LATCH-003 — Logical completion 后仍由 wait core 与 scheduler 收口

**规则：** producer trigger 必须通过 wait core 竞争 logical completion；成功后 scheduler 接管 physical-placement obligation，consumer 不观察 placement result。`Triggered` 只说明本轮需要重验上层 predicate，不证明任何 fd 仍 ready。diagnostic wait id、task id 与 placement classification 不得反向驱动 Latch 或 source 状态机。

**违反表现：** Latch/source 自行补偿 placement、把 `Triggered` 当成 readiness truth，或用 diagnostic identity 匹配未来 round。

**验证 / Enforcement：** `LatchTrigger::trigger()`、`wake_wait()` 与 [`SCHED-WAKE-001..004`](./wake-delivery.md) composition audit。

**最初来源：** [Sched Latch RFC](../../rfcs/sched-latch/index.md)。

**当前来源：** [`SCHED-WAKE`](./wake-delivery.md) 与 live latch implementation，2026-07-26 源码核验。
