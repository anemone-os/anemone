# ThreadGroup Lifecycle 当前契约

**Contract ID：** `TASK-LIFE`
**状态：** Active
**Owner：** `ThreadGroup` terminal lifecycle
**参与领域：** task / topology / signal terminal action / child wait / procfs
**覆盖范围：** user ThreadGroup 的 `Alive / Exiting / Exited` truth、exit-code 选择、member detach 与 waitable publication
**不覆盖：** job-control stop phase、ptrace stop、kernel-thread lifecycle、subreaper policy
**实现位置：** `anemone-kernel/src/task/mod.rs`、`anemone-kernel/src/task/api/exit/`、`anemone-kernel/src/task/topology/`
**依赖：** None
**Pending Successor：** None
**最后核验：** 2026-07-20

## 状态与所有权

| 状态 | 唯一 Owner | 说明 |
| --- | --- | --- |
| `Alive` | `ThreadGroupInner::status.life_cycle` | 允许普通成员与 topology 操作 |
| `Exiting(first_code)` | 同上 | terminal group exit 已开始；后续发起者不能替换 first code |
| `Exited(first_code)` | 同上 | 最后 member 已 detach，child 可以被 parent wait 选择 |

task-local exit code、scheduler Zombie 和 procfs binding 不得反向驱动 ThreadGroup lifecycle。

## TASK-LIFE-001 — ThreadGroup lifecycle 是 terminal truth

**规则：** user ThreadGroup 的 terminal phase 与 group exit code 只由 `ThreadGroupInner::status.life_cycle` 持有。`exit_group` 的第一个 `Alive -> Exiting(code)` 决定 group terminal code；后续 exit-group 请求沿用该 code，不能建立第二份 terminal decision。

**违反表现：** task-local exit code覆盖已发布的 group code、Signal 或 scheduler 自行发布 `Exited`，或多个 terminal owner 并行推进 phase。

**验证 / Enforcement：** `ThreadGroupLifeCycle`、`kernel_exit_group()` 与 `kernel_exit()` 源码审计；multi-thread exit 回归。

**最初来源：** 现有 ThreadGroup / exit 实现。

**当前来源：** live ThreadGroup lifecycle owner，2026-07-20 源码核验。

## TASK-LIFE-002 — 最后 member detach 后才能发布 Exited

**规则：** 每个 exiting task 先完成 owner-local cleanup 并从 topology / ThreadGroup membership detach；只有最后一个 member detach 后，ThreadGroup lifecycle owner 才完成 orphan reparent 并发布 `Exited(first_code)`。`Exited` 因而意味着 topology-visible member set 已空，并允许 parent wait 尝试 reap。

**违反表现：** live member 尚未 detach 时 child 已可 reap、membership removal 晚于 `Exited`，或 reparent / waitability 由 task `Drop` 偶然完成。

**验证 / Enforcement：** `Task::detach_from_topology()`、`kernel_exit()`、`ThreadGroup::try_reap_child()` assertion 与源码审计。

**最初来源：** 现有 task topology / exit 实现。

**当前来源：** live ThreadGroup lifecycle owner，2026-07-20 源码核验。

## TASK-LIFE-003 — Terminal publication 先于 parent notification

**规则：** last-member exit 先发布 `ThreadGroupLifeCycle::Exited`，再按 child 的 configured terminate signal决定是否发送 signal，并发布 `child_exited` Event。通知只是要求 parent 重扫；它不能先于 lifecycle truth 创建 waitable child。本条不承诺并发 reparent 下 signal 与 Event 的 parent-selection 原子性。

**违反表现：** parent notification 先于已经承诺的 terminal predicate，或 notification 成为 exit truth。

**验证 / Enforcement：** `kernel_exit()` publication 顺序源码审计；wait / exit race 回归。

**最初来源：** 现有 task exit 实现。

**当前来源：** live ThreadGroup lifecycle owner，2026-07-20 源码核验。
