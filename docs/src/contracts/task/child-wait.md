# Child Wait 当前契约

**Contract ID：** `CHILD-WAIT`
**状态：** Active
**Owner：** task/wait child-selection and claim protocol
**参与领域：** parent-child topology / ThreadGroup lifecycle / job control / wait4 / waitid / Event / procfs
**覆盖范围：** terminal与job-control child status truth、target selection、predicate rescan、`WNOWAIT` peek、report consume与terminal reap
**不覆盖：** ptrace status、pidfd wait、core-dump reporting、完整rusage
**实现位置：** `anemone-kernel/src/task/api/wait/`、`anemone-kernel/src/task/topology/parent_child.rs`、`anemone-kernel/src/task/api/exit/mod.rs`
**依赖：** `TASK-LIFE-001..003`、`PGRP-SIGNAL-001`、`JOBCTL-STATE-001`、`JOBCTL-REPORT-001`
**Pending Successor：** None
**最后核验：** 2026-07-21

## CHILD-WAIT-001 — Wait truth来自terminal或job-control owner

**规则：** wait-family只从仍属于调用者child topology的user ThreadGroup读取typed child status：`Exited(code)`来自terminal lifecycle；`Stopped(reason) / Continued`来自child-owned job-control report。terminal status具有最高优先级。`wait4 / waitid`不从SIGCHLD、Event、procfs或scheduler state推导status。

**违反表现：** signal notification 被当成可消费 status、非 child ThreadGroup 被返回，或 `Alive / Exiting` child 被错误选择为 exited result。

**验证 / Enforcement：** `WaitScanner::select_one()`、`wait_outcome_from_exited_child()`、report selection与`ThreadGroup::find_child()`源码审计；wait-family LTP。

**最初来源：** 现有 wait4 exited-child core；[waitid exited-child bridge](../../devlog/changes/2026-06-14-waitid.md)。

**当前来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)；[Stage 5 cutover事务](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。

## CHILD-WAIT-002 — Target selection 每轮重读 child relation

**规则：** `P_ALL` / any-child、`P_PID` / exact TGID和`P_PGID` / child PGID selection每轮从current parent-child relation扫描。scan可以先取得object snapshot，但terminal reap或job-control report consume必须在`parent relation -> child owner` transaction中重新确认relation、selector和selected state；失败者丢弃旧candidate并重扫。

**违反表现：** stale child object在reparent、selector变化、并发reap或report consume后仍授权claim，或selector自行保存第二份child membership/status truth。

**验证 / Enforcement：** `WaitScanner`、`ThreadGroup::find_child()` 与 `ThreadGroup::try_reap_child()` 源码审计；P_ALL / P_PID / P_PGID waitid 回归。

**最初来源：** [2026-05-27 进程组与会话 stage-1 主干](../../devlog/2026-05-25_to_2026-06-07.md#2026-05-27---进程组与会话-stage-1-主干)；[waitid exited-child bridge](../../devlog/changes/2026-06-14-waitid.md)。

**当前来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)；[Stage 5 cutover事务](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。

## CHILD-WAIT-003 — Event 只触发 predicate rescan

**规则：** parent `child_status_changed` Event只表示terminal或job-control child predicate可能改变。waiter在listen publication前后都重扫child list；wake、interrupt或Event payload不携带selected child、status或claim ownership。producer只有在durable owner state已经提交后才guards-out publish。

**违反表现：** Event token 被解释为 child status、一次 wake 对应固定 child，或 lost / coalesced wake 使 durable `Exited` predicate 永久不可见。

**验证 / Enforcement：** `wait_for_child_status()` predicate-loop、job-control report与`kernel_exit()` publication顺序源码审计；wait interruption / concurrent exit回归。

**最初来源：** 现有 wait4 exited-child loop；[waitid exited-child bridge](../../devlog/changes/2026-06-14-waitid.md)。

**当前来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)；[Stage 5 cutover事务](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。

## CHILD-WAIT-004 — Peek、report consume与terminal reap使用同一truth

**规则：** `waitid(..., WNOWAIT)`从selected terminal或job-control status读取snapshot，不移除topology relation也不消费report；普通wait4 / waitid对Stopped/Continued exact-once consume当前report，对Exited则通过`try_reap_child()`原子移除child relation和topology identity。多个waiter竞争时至多一个consume/reap成功，失败者丢弃旧scan并重新判断`ECHILD`、`WNOHANG`或继续等待。

**违反表现：** `WNOWAIT` 消费 child、多个 waiter 同时 reap、reap 失败后沿用 stale status，或 claim 与 parent relation removal 分离。

**验证 / Enforcement：** `WaitDisposition::{Peek,Reap}`、`wait_for_child_status()`、report consume与`try_reap_child()`源码审计；concurrent waiter回归。

**最初来源：** [waitid exited-child bridge](../../devlog/changes/2026-06-14-waitid.md)。

**当前来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)；[Stage 5 cutover事务](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。

## CHILD-WAIT-005 — Wait4 / waitid提供stopped与continued ABI

**规则：** `wait4`以`WUNTRACED / WCONTINUED`选择Stopped/Continued并序列化Linux wait status；`waitid`以`WSTOPPED / WCONTINUED`选择对应report并返回`CLD_STOPPED / CLD_CONTINUED`。`WNOWAIT`对terminal和job-control status都只peek。stopped/continued `si_uid`保持当前scoped limitation `0`，不得从任意live member猜测credential。

**违反表现：** 没有durable child report时合成status；selector未请求对应class仍返回report；peek消费report；wait4/waitid从不同truth序列化；缓存leader credential填充`si_uid`。

**验证 / Enforcement：** `WaitOptions` parser、shared typed scanner、wait4/waitid serializer与report claim源码审计；WNOWAIT双peek+consume、`si_uid=0`、waitid07/08和waitpid08/13 runtime。

**最初来源：** [waitid exited-child bridge](../../devlog/changes/2026-06-14-waitid.md)。

**当前来源：** [RFC-20260720-unix-jobctl R1](../../rfcs/unix-jobctl/index.md)；[Stage 5 cutover事务](../../devlog/transactions/2026-07-20-unix-jobctl.md#stage-5-uj-cutover与事务收口---2026-07-21)。
