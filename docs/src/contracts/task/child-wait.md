# Child Wait 当前契约

**Contract ID：** `CHILD-WAIT`
**状态：** Active
**Owner：** task/wait child-selection and claim protocol
**参与领域：** parent-child topology / ThreadGroup lifecycle / wait4 / waitid / Event / procfs
**覆盖范围：** exited-child wait truth、target selection、predicate rescan、`WNOWAIT` peek 与 reap claim
**不覆盖：** stopped / continued report、ptrace status、pidfd wait、core-dump reporting、完整 rusage
**实现位置：** `anemone-kernel/src/task/api/wait/`、`anemone-kernel/src/task/topology/parent_child.rs`、`anemone-kernel/src/task/api/exit/mod.rs`
**依赖：** `TASK-LIFE-001`、`TASK-LIFE-002`、`TASK-LIFE-003`、`PGRP-SIGNAL-001`
**Pending Successor：** [RFC-20260720-unix-jobctl R0](../../rfcs/unix-jobctl/index.md)；`UJ-CUTOVER` 前不生效
**最后核验：** 2026-07-20

## CHILD-WAIT-001 — 当前 wait truth 只有 Exited child

**规则：** 当前 wait-family 只把仍属于调用者 child topology、且 lifecycle 为 `Exited(code)` 的 user ThreadGroup 视为 waitable。`wait4` / `waitid` 不从 SIGCHLD、Event、procfs state 或 scheduler state推导 child status。

**违反表现：** signal notification 被当成可消费 status、非 child ThreadGroup 被返回，或 `Alive / Exiting` child 被错误选择为 exited result。

**验证 / Enforcement：** `WaitScanner::scan_one()`、`wait_outcome_from_child()`、`ThreadGroup::find_child()` 源码审计；wait-family LTP。

**最初来源：** 现有 wait4 exited-child core；[waitid exited-child bridge](../../devlog/changes/2026-06-14-waitid.md)。

**当前来源：** live task/wait owner，2026-07-20 源码核验。

## CHILD-WAIT-002 — Target selection 每轮重读 child relation

**规则：** `P_ALL` / any-child、`P_PID` / exact TGID 和 `P_PGID` / child PGID selection 每轮从当前 parent-child relation 扫描。scan 可以先取得 object snapshot，但 consuming claim 必须在 topology transaction 中重新确认 child relation 与 `Exited` 状态。

**违反表现：** stale child object 在 reparent 或并发 reap 后仍授权 claim，或 selector 自行保存第二份 child membership truth。当前 consuming claim 尚不承诺原子重验 child 的 PGID selector；后续若需要该保证，必须作为真实 contract delta 引入。

**验证 / Enforcement：** `WaitScanner`、`ThreadGroup::find_child()` 与 `ThreadGroup::try_reap_child()` 源码审计；P_ALL / P_PID / P_PGID waitid 回归。

**最初来源：** [2026-05-27 进程组与会话 stage-1 主干](../../devlog/2026-05-25_to_2026-06-07.md#2026-05-27---进程组与会话-stage-1-主干)；[waitid exited-child bridge](../../devlog/changes/2026-06-14-waitid.md)。

**当前来源：** live parent-child topology / task-wait owner，2026-07-20 源码核验。

## CHILD-WAIT-003 — Event 只触发 predicate rescan

**规则：** parent `child_exited` Event 只表示 child predicate 可能改变。waiter 在 listen publication 前后都重扫 child list；wake、interrupt 或 Event payload 不携带 selected child、exit code或 claim ownership。

**违反表现：** Event token 被解释为 child status、一次 wake 对应固定 child，或 lost / coalesced wake 使 durable `Exited` predicate 永久不可见。

**验证 / Enforcement：** `wait_for_exited_child()` predicate-loop 与 `kernel_exit()` publication 顺序源码审计；wait interruption / concurrent exit 回归。

**最初来源：** 现有 wait4 exited-child loop；[waitid exited-child bridge](../../devlog/changes/2026-06-14-waitid.md)。

**当前来源：** live task/wait owner，2026-07-20 源码核验。

## CHILD-WAIT-004 — Peek 与 reap 使用同一 truth、不同 claim

**规则：** `waitid(..., WNOWAIT)` 从 selected `Exited` child 读取 snapshot 而不移除 topology relation；普通 wait4 / waitid 通过 `try_reap_child()` 原子移除 child relation和 topology identity。多个 waiter 竞争时至多一个 reap 成功，失败者丢弃旧 scan 并重新判断 `ECHILD`、`WNOHANG` 或继续等待。

**违反表现：** `WNOWAIT` 消费 child、多个 waiter 同时 reap、reap 失败后沿用 stale status，或 claim 与 parent relation removal 分离。

**验证 / Enforcement：** `WaitDisposition::{Peek,Reap}`、`wait_for_exited_child()` 与 `try_reap_child()` 源码审计；concurrent waiter 回归。

**最初来源：** [waitid exited-child bridge](../../devlog/changes/2026-06-14-waitid.md)。

**当前来源：** live task/wait owner，2026-07-20 源码核验。

## CHILD-WAIT-005 — Non-exit wait ABI 当前 fail closed 或保持 exit-only

**规则：** `waitid` 对 `WSTOPPED` / `WCONTINUED` 返回 `EOPNOTSUPP` 并记录日志；`wait4` 接受已识别的 `WUNTRACED` / `WCONTINUED` bits，但当前 scan 仍只返回 exited child，不伪造 stopped / continued status。`waitid(..., WNOWAIT)` 只对当前 exited-child truth 生效。

**违反表现：** 在没有 durable child report 的情况下合成 stopped / continued status，或把 accepted flag 误写为对应语义已实现。

**验证 / Enforcement：** `validate_waitid_options()`、`WaitOptions` parser 和 shared exited-child scan 源码审计；[当前限制](../../register/current-limitations.md#ane-20260527-process-group-session-stage1)。

**最初来源：** [waitid exited-child bridge](../../devlog/changes/2026-06-14-waitid.md)。

**当前来源：** live wait ABI owner，2026-07-20 源码核验。
