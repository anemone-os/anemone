# User Entry 当前契约

**Contract ID：** `USER-ENTRY`
**状态：** Active
**Owner：** user-task transition protocol
**参与领域：** architecture trap return / Signal / ThreadGroup lifecycle / job control / task privilege accounting
**覆盖范围：** RV64与LA64 ordinary、fresh、clone和exec user transition的统一Signal/lifecycle/jobctl arbitration、privilege publication与architecture transition顺序
**不覆盖：** scheduler stop state、kernel-thread entry、ptrace resume
**实现位置：** `anemone-kernel/src/arch/{riscv64,loongarch64}/`、`anemone-kernel/src/task/{jobctl,sig}/`
**依赖：** `SIGNAL-ACTION-002`、`TASK-LIFE-001`、`JOBCTL-STOP-001`、`JOBCTL-LIFE-001`
**Pending Successor：** None
**最后核验：** 2026-07-21

## USER-ENTRY-001 — Ordinary trap-return完成统一arbitration

**规则：** RV64与LA64的ordinary user trap handler在architecture return前、local interrupt仍允许普通内核处理时调用统一`arbitrate_user_entry()`。该arbitration完成Signal pending/action与temporary-mask收口，重验ThreadGroup terminal lifecycle和job-control gate；只有`Alive + Running`可以登记current member exposure并继续。随后入口关闭local interrupt、完成architecture-local FPU state和`Task::on_prv_change(Privilege::User)`，最后执行不可返回的hardware user transition。

**违反表现：** ordinary trapped task 在 pending terminal/custom action 尚未仲裁时执行用户指令、`on_prv_change(User)` 早于仍可发生 kernel-mode trap 的窗口，或 RV64 / LA64 复制出不同 Signal policy。

**验证 / Enforcement：** 两个 architecture `utrap_handler()` 尾部、`handle_signals()` 与 `__utrap_return_to_task` 调用顺序源码审计；ordinary syscall / exception / signal delivery 回归。

**最初来源：** 现有 RV64 / LA64 trap-return 与 Signal 实现。

**当前来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)；[Stage 5 cutover事务](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。

## USER-ENTRY-002 — 所有user transition共享mandatory gate

**规则：** fresh task、clone child和exec new image在第一次执行用户指令前都汇入与ordinary return相同的逻辑arbitration。新membership在gate前保持unexposed；exec/dethread survivor不能继承victim的exposure、park责任或reserved action。任何入口发现terminal lifecycle都进入既有no-return terminal path；`Stopping / Stopped`保持unexposed并只在jobctl parker上等待，wake后从Signal/lifecycle/jobctl起点重新仲裁，Event不携带user-entry permit。

R1保留Signal-owned reservation-first顺序：pre-existing reserved `SIGCONT`可以在Stopped期间完成live action selection、handler-frame / no-frame cleanup，并将later pending `SIGKILL`延迟到下一次mandatory kernel entry；在真实新`SIGCONT`恢复`Running`前不得进入handler用户态。恢复后可以先执行这次已经reserved的handler所对应的有限用户态，`SIGKILL`仍保持pending并在下一次mandatory kernel entry重新仲裁；该顺序不能绕过Stopped gate、删除`SIGKILL` pending或覆盖已经提交的terminal lifecycle。

**违反表现：** fresh/clone/exec直接执行用户指令；wake token授予entry permit；member在final gate前标记exposed后仍可返回可恢复错误；reserved handler frame提交被误认为已经进入handler；RV64/LA64维护不同策略。

**验证 / Enforcement：** RV64/LA64 ordinary/raw/fresh entry、clone与exec call graph source closure；multi-member stop、ordinary wait、exec new image、temporary-mask SIGCONT action与SIGKILL dominance runtime/KUnit。

**最初来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)。

**当前来源：** [Unix job control事务Stage 5 cutover](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。
