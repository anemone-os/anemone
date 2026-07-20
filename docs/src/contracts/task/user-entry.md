# Ordinary User Entry 当前契约

**Contract ID：** `USER-ENTRY`
**状态：** Active
**Owner：** user-task transition protocol
**参与领域：** architecture trap return / Signal / task privilege accounting
**覆盖范围：** RV64 与 LA64 ordinary user trap-return 的 Signal arbitration、privilege publication 与 architecture transition 顺序
**不覆盖：** fresh task、clone child、exec new image 的直接 entry，job-control gate，scheduler stop state
**实现位置：** `anemone-kernel/src/arch/{riscv64,loongarch64}/exception/trap/utrap.rs`、`anemone-kernel/src/task/sig/mod.rs`
**依赖：** `SIGNAL-ACTION-002`、`TASK-LIFE-001`
**Pending Successor：** [RFC-20260720-unix-jobctl R0](../../rfcs/unix-jobctl/index.md)；`UJ-CUTOVER` 前不生效
**最后核验：** 2026-07-20

## USER-ENTRY-001 — Ordinary trap-return 先完成 Signal arbitration

**规则：** RV64 与 LA64 的 ordinary user trap handler 在 architecture return 前、local interrupt 仍允许普通内核处理时调用 `handle_signals()`；Signal 完成 pending scan、default / custom action与 temporary-mask owner-local收口后，入口关闭 local interrupt、完成 architecture-local FPU state 和 `Task::on_prv_change(Privilege::User)`，最后执行不可返回的 hardware user transition。

**违反表现：** ordinary trapped task 在 pending terminal/custom action 尚未仲裁时执行用户指令、`on_prv_change(User)` 早于仍可发生 kernel-mode trap 的窗口，或 RV64 / LA64 复制出不同 Signal policy。

**验证 / Enforcement：** 两个 architecture `utrap_handler()` 尾部、`handle_signals()` 与 `__utrap_return_to_task` 调用顺序源码审计；ordinary syscall / exception / signal delivery 回归。

**最初来源：** 现有 RV64 / LA64 trap-return 与 Signal 实现。

**当前来源：** live architecture trap-return / Signal owner，2026-07-20 源码核验。

## 当前覆盖边界

fresh task 的 `user_task_entry_secondary()`、clone child 的 `enter_cloned_user_task()` 和 exec new image 的 context load 走独立直接 entry，目前不继承 `USER-ENTRY-001` 的完整 Signal arbitration。它们也没有 job-control gate。该差异是本页明确的 current scope，不得把 ordinary trap-return contract 推断成“所有用户态入口已经闭合”。
