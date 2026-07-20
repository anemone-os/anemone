# Signal Pending Routing 与 Ordinary Action Selection 当前契约

**Contract ID：** `SIGNAL-PENDING` / `SIGNAL-ACTION`
**状态：** Active
**Owner：** Signal pending / disposition protocol
**参与领域：** signal / task / thread group / architecture trap return
**覆盖范围：** task-directed 与 ThreadGroup-directed occurrence 的 pending 归属、ignored admission、普通 return-to-user 的 fetch 和 action selection
**不覆盖：** temporary-mask reserved-delivery handoff、`rt_sigtimedwait` 的完整同步消费语义、job-control side effect、fresh / clone / exec user entry
**实现位置：** `anemone-kernel/src/task/sig/`、`anemone-kernel/src/arch/{riscv64,loongarch64}/exception/trap/utrap.rs`
**依赖：** None
**Pending Successor：** None
**最后核验：** 2026-07-20

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| task-private pending occurrence | 目标 `Task::sig_pending` | sender 只提交 occurrence | task-directed delivery |
| ThreadGroup-shared pending occurrence | `ThreadGroupInner::sig_pending` | member 只通过 fetch / notification 参与 | process-directed delivery |
| live disposition | 共享 `SignalDisposition` | receiver 和 trap-return 读取 snapshot | ignored admission 与 action selection |
| current signal mask | 目标 task 的 signal-mask owner | pending scan 读取 snapshot | fetch eligibility |

`Event`、scheduler notification、选中的 member 或 trapframe 都不是 pending occurrence 的第二真相源。

## SIGNAL-PENDING-001 — Directed occurrence 只进入对应 pending owner

**规则：** task-directed occurrence 进入目标 task 的 private pending；ThreadGroup-directed occurrence 只进入该 ThreadGroup 的 shared pending。standard signal 使用单 slot 合并，realtime signal 使用 FIFO queue。ordinary pending occurrence 在被 fetch、显式 flush 或 pending owner teardown 前由对应 pending owner 持有；temporary-mask classifier 完成 claim 后的 task-private handoff 由 [`SIGNAL-TEMP-MASK-002`](./temporary-mask-delivery.md#signal-temp-mask-002--defer-必须先建立-task-private-delivery-handoff)负责。

**违反表现：** 同一个 group-directed occurrence 被复制到多个 member、private / shared 同时持有同一 occurrence，或 notification 结果反向决定 pending truth。

**验证 / Enforcement：** `Task::recv_signal()`、`ThreadGroup::recv_signal()`、`PendingSignals::{push_signal,fetch_any,flush_specific}` 源码审计；signal syscall / LTP 回归。

**最初来源：** 现有 Signal 实现；[Signal temporary-mask restore 事务](../../devlog/transactions/2026-06-06-signal-temp-mask-restore.md)记录了 pending 与 deferred delivery 的后续演进。

**当前来源：** live Signal owner，2026-07-20 源码核验。

## SIGNAL-PENDING-002 — Group-directed publication 与 member notification 分离

**规则：** ThreadGroup-directed occurrence 先发布到 shared pending，再对当次 member snapshot 中未屏蔽该 signal 的 member 发出 notification；`SIGKILL` / `SIGSTOP` 使用强制 notification。notification 只促使 member 重扫 shared pending，不授予 occurrence ownership，也不把 process-directed signal 变成 thread-local signal。

**违反表现：** member wake 被当作 delivery commit、member snapshot 变成长期 signal target truth，或在持有 topology / ThreadGroup membership lock 时执行递送副作用。

**验证 / Enforcement：** `ThreadGroup::recv_signal()` 与 `ThreadGroup::get_members()` 源码审计；private/shared pending 定向测试。

**最初来源：** 现有 Signal 与 ThreadGroup 实现。

**当前来源：** live Signal owner，2026-07-20 源码核验。

## SIGNAL-ACTION-001 — Ignored disposition 在 pending publication 前生效

**规则：** 当前 receive path 在写入 private 或 shared pending 前读取 live disposition；显式 ignore 或 default-ignore 的 occurrence 不进入 pending。该规则只描述当前 ordinary signal admission，不承诺尚未实现的 job-control generation-time side effect。

**违反表现：** ignored occurrence 留在 pending 并在 disposition 未再次改变时被普通 delivery 消费，或 receiver 在 publication 后才把 occurrence 当作从未发生。

**验证 / Enforcement：** `Task::recv_signal()`、`ThreadGroup::recv_signal()` 与 `SignalAction::is_ignored()` 源码审计。

**最初来源：** 现有 Signal 实现。

**当前来源：** live Signal owner，2026-07-20 源码核验。

## SIGNAL-ACTION-002 — Ordinary trap-return 才提交异步 action

**规则：** ordinary user trap-return 通过 `handle_signals()` 消费 pending；当前 task 先扫描 private pending，再扫描 ThreadGroup shared pending，并在 fetch 后读取 live disposition决定 default、ignore 或 custom action。custom action 通过当前 trapframe 建立 handler frame；default terminal action进入 ThreadGroup lifecycle。

**违反表现：** notification 提前提交 handler / default action、shared pending 被多个 member 同时消费，或 ordinary trap-return 绕过当前 disposition 直接进入 custom handler。

**验证 / Enforcement：** `Task::fetch_signal()`、`handle_signals()`、`perform_signal_action()` 及 RV64 / LA64 ordinary trap-return 源码审计；signal handler 回归。

**最初来源：** 现有 Signal trap-return 实现。

**当前来源：** live Signal owner，2026-07-20 源码核验。

## 当前边界

`SIGSTOP`、`SIGTSTP`、`SIGTTIN`、`SIGTTOU` 和 `SIGCONT` 已有 default-action 分类，但 stop / continue default action 当前仍未实现；当前也没有 opposite-class pending cleanup 或 unconditional `SIGCONT` resume side effect。该能力缺口由[进程组与会话 stage-1 当前限制](../../register/current-limitations.md#ane-20260527-process-group-session-stage1)记录，不是本页的 effective job-control contract。
