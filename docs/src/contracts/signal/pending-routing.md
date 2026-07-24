# Signal Pending Routing 与 Ordinary Action Selection 当前契约

**Contract ID：** `SIGNAL-PENDING` / `SIGNAL-ACTION`
**状态：** Active
**Owner：** Signal pending / disposition protocol
**参与领域：** signal / task / thread group / architecture trap return
**覆盖范围：** task-directed 与 ThreadGroup-directed occurrence 的 pending 归属、ignored admission、普通 return-to-user 的 fetch / action selection，以及control-signal generation handoff
**不覆盖：** temporary-mask reserved-delivery handoff、`rt_sigtimedwait` 的完整同步消费语义、ThreadGroup-owned job-control phase / report、fresh / clone / exec user entry
**实现位置：** `anemone-kernel/src/task/sig/`、`anemone-kernel/src/arch/{riscv64,loongarch64}/exception/trap/utrap.rs`
**依赖：** `JOBCTL-SIGNAL-001`、`JOBCTL-STATE-001`
**Pending Successor：** None
**最后核验：** 2026-07-21

## 状态与能力所有权

| 状态 / 能力 | 唯一 Owner | 其它参与方持有什么 | 行为用途 |
| --- | --- | --- | --- |
| task-private pending occurrence | 目标 `Task::sig_pending` | sender 只提交 occurrence | task-directed delivery |
| ThreadGroup-shared pending occurrence | `ThreadGroupInner::sig_pending` | member 只通过 fetch / notification 参与 | process-directed delivery |
| live disposition | 共享 `SignalDisposition` | receiver 和 trap-return 读取 snapshot | ignored admission 与 action selection |
| current signal mask | 目标 task 的 signal-mask owner | pending scan 读取 snapshot | fetch eligibility |

`Event`、scheduler notification、选中的 member 或 trapframe 都不是 pending occurrence 的第二真相源。

## SIGNAL-PENDING-001 — Directed occurrence 只进入对应 pending owner

**规则：** task-directed ordinary occurrence进入目标task的private pending；ThreadGroup-directed ordinary occurrence只进入该ThreadGroup的shared pending。standard signal使用单slot合并，realtime signal使用FIFO queue。ordinary pending occurrence在被fetch、显式flush或pending owner teardown前由对应pending owner持有；temporary-mask classifier完成claim后的task-private handoff由[`SIGNAL-TEMP-MASK-002`](./temporary-mask-delivery.md#signal-temp-mask-002--defer-必须先建立-task-private-delivery-handoff)负责。`SIGSTOP`是唯一scoped exception：合法generation完成opposite-class ordinary pending cleanup和global-init admission后，直接作为[`JOBCTL-SIGNAL-001`](../task/job-control.md#jobctl-signal-001--control-signal-generation与jobctl提交同序)的control input消费，不进入private / shared pending。

**违反表现：** 同一个 group-directed occurrence 被复制到多个 member、private / shared 同时持有同一 occurrence，或 notification 结果反向决定 pending truth。

**验证 / Enforcement：** `Task::recv_signal()`、`ThreadGroup::recv_signal()`、owner-private `ThreadGroup::recv_job_control_signal()`、`PendingSignals::{push_signal,fetch_any,flush_specific}`源码审计；task/group-directed四种stop signal、opposite cleanup与signal LTP回归。

**最初来源：** 现有 Signal 实现；[Signal temporary-mask restore 事务](../../devlog/transactions/2026-06-06-signal-temp-mask-restore.md)记录了 pending 与 deferred delivery 的后续演进。

**当前来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)；[Stage 5 cutover事务](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。

## SIGNAL-PENDING-002 — Group-directed publication 与 member notification 分离

**规则：** ThreadGroup-directed ordinary occurrence先发布到shared pending，再对当次member snapshot中未屏蔽该signal的member发出notification；ordinary pending `SIGKILL`使用强制notification。notification只促使member重扫shared pending，不授予occurrence ownership，也不把process-directed signal变成thread-local signal。`SIGSTOP`不发布pending、不force-complete active wait；它在`ThreadGroup` generation owner内直接请求stop。可选user-execution kick若未来加入，只能guards-out且在目标已离开user execution时安全no-op。

**违反表现：** member wake 被当作 delivery commit、member snapshot 变成长期 signal target truth，或在持有 topology / ThreadGroup membership lock 时执行递送副作用。

**验证 / Enforcement：** `ThreadGroup::recv_signal()` 与 `ThreadGroup::get_members()` 源码审计；private/shared pending 定向测试。

**最初来源：** 现有 Signal 与 ThreadGroup 实现。

**当前来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)；[Stage 5 cutover事务](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。

## SIGNAL-ACTION-001 — Ignored disposition 在 pending publication 前生效

**规则：** receive path在写入private或shared pending前读取live disposition；显式ignore或default-ignore的ordinary occurrence不进入pending。已经通过target / permission validation的control-signal generation side effect独立线性化：`SIGCONT`无条件执行cleanup / resume后，ordinary occurrence仍按live disposition决定是否进入pending；stop-class generation同样先完成opposite cleanup，`SIGSTOP`再按global-init admission直接请求stop，另外三种conditional stop仍须由后续live action selection决定是否取得DefaultStop authority。

**违反表现：** ignored occurrence 留在 pending 并在 disposition 未再次改变时被普通 delivery 消费，或 receiver 在 publication 后才把 occurrence 当作从未发生。

**验证 / Enforcement：** `Task::recv_signal()`、`ThreadGroup::recv_signal()` 与 `SignalAction::is_ignored()` 源码审计。

**最初来源：** 现有 Signal 实现。

**当前来源：** live Signal admission rule；[RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)保持ignored admission并增加control generation handoff；[Stage 5 cutover事务](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。

## SIGNAL-ACTION-002 — Ordinary trap-return 才提交异步 action

**规则：** ordinary user-entry arbitration通过`handle_signals()`消费pending；当前task先取得reserved target，再扫描private pending与ThreadGroup shared pending，并在claim后读取live disposition决定default、ignore或custom action。custom action通过当前trapframe建立handler frame；default terminal action进入ThreadGroup lifecycle。普通user ThreadGroup的`SIGSTOP`已经在generation transaction直接消费，不进入本路径；`SIGTSTP / SIGTTIN / SIGTTOU`只有在本路径最终选择`DefaultStop`后，才携带captured epoch向ThreadGroup owner请求条件性stop。`Stopping / Stopped`期间只允许RFC明确列出的terminal / control closure；已reserved的`SIGCONT`可以完成live action与temporary-mask cleanup，但handler frame提交不授予user-entry permit。

**违反表现：** notification 提前提交 handler / default action、shared pending 被多个 member 同时消费，或 ordinary trap-return 绕过当前 disposition 直接进入 custom handler。

**验证 / Enforcement：** `Task::fetch_signal()`、`handle_signals()`、`perform_signal_action()` 及 RV64 / LA64 ordinary trap-return 源码审计；signal handler 回归。

**最初来源：** 现有 Signal trap-return 实现。

**当前来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)；[Stage 5 cutover事务](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。

## 当前边界

本页只拥有Signal pending、reservation与action selection。ThreadGroup phase、continue epoch、exposure、report、user-entry gate与wait/procfs投影由[Unix job control当前契约](../task/job-control.md)拥有。R1保留reserved target相对later pending signal的既有优先级；pre-existing reserved `SIGCONT`可以把later pending `SIGKILL`延迟到下一次mandatory kernel entry，但不能绕过Stopped gate、删除`SIGKILL` pending或覆盖已经提交的terminal lifecycle。
