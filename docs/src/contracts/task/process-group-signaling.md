# Process-group Signal Targeting 当前契约

**Contract ID：** `PGRP-SIGNAL`
**状态：** Active
**Owner：** task topology / `ProcessGroup` membership
**参与领域：** task topology / process group / signal syscall / Signal
**覆盖范围：** `kill(0, sig)`、`kill(-pgid, sig)` 的 ThreadGroup target selection 与独立 signal publication
**不覆盖：** controlling TTY、foreground process group、orphaned-process-group policy、process-group-wide stop transaction
**实现位置：** `anemone-kernel/src/task/topology/process_group.rs`、`anemone-kernel/src/task/sig/api/kill.rs`
**依赖：** `SIGNAL-PENDING-001`、`SIGNAL-PENDING-002`
**Pending Successor：** None
**最后核验：** 2026-07-21

## PGRP-SIGNAL-001 — ProcessGroup 只拥有成员选择

**规则：** task topology 是 ProcessGroup membership 的唯一 owner。signal broadcast 从 ProcessGroup 获取当次 ThreadGroup snapshot 后释放 topology / ProcessGroup lock；ProcessGroup 不拥有 signal pending、delivery phase 或任何 member ThreadGroup 的行为状态。

**违反表现：** ProcessGroup 保存 job-control phase、在 topology lock 内执行 signal delivery / wake，或用递送结果反向修改 membership truth。

**验证 / Enforcement：** `ProcessGroup::get_members()`、`ProcessGroup::recv_signal()` 与 topology mutation 源码审计；process-group signal 用户态测试。

**最初来源：** [2026-05-27 进程组与会话 stage-1 主干](../../devlog/2026-05-25_to_2026-06-07.md#2026-05-27---进程组与会话-stage-1-主干)。

**当前来源：** live task-topology owner，2026-07-20 源码核验。

## PGRP-SIGNAL-002 — 每个 ThreadGroup 独立接受 occurrence

**规则：** `kill(0, sig)` 与 `kill(-pgid, sig)` 对 snapshot 中每个可授权的 user ThreadGroup 独立执行 permission check 和 `ThreadGroup::recv_signal()`。调用成功不建立 process-group-wide atomic delivery、stop phase 或统一完成点；成员变化不会把一个 ThreadGroup 的 signal / job-control 状态转移给 ProcessGroup。

**违反表现：** 部分 ThreadGroup 的 publication 被解释为全组原子 commit，某个 member 失败回滚其它已接受 occurrence，或 ProcessGroup 等待全部 ThreadGroup 完成 signal action。

**验证 / Enforcement：** `sys_kill()`的process-group branches、`ProcessGroup::recv_signal()`与每个ThreadGroup的generation handoff源码审计；两个ThreadGroup、四种stop signal的process-group broadcast runtime。

**最初来源：** [2026-05-27 进程组与会话 stage-1 主干](../../devlog/2026-05-25_to_2026-06-07.md#2026-05-27---进程组与会话-stage-1-主干)。

**当前来源：** live signal syscall / task-topology owner；[RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)保持每个ThreadGroup独立接受与stop/report；[Stage 5 cutover事务](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。
