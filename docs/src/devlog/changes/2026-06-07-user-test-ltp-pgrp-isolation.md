# ANE-CHG-20260607-user-test-ltp-pgrp-isolation

**Type:** Test Infra Improvement / Investigation
**Status:** Active
**Date:** 2026-06-07
**Authors:** doruche, Codex
**Area:** anemone-apps/user-test / LTP runner / process group

## Problem

当前启动链路里，`init` 通过 `clone(SIGCHLD)` 启动 `/bin/user-test`，没有为
`user-test` 单独创建进程组。内核 fork-like `clone` 会继承父 thread group 的
`pgid` / `sid`，所以 `user-test` 默认仍在 `init` 的进程组里。

`user-test` 的 LTP runner 在 `run_ltp_case()` 中直接 `fork()` 一个 case，再在
child 里 `chdir()`、组装 argv、`execve()`。这里也没有在 case 进程执行前调用
`setpgid(0, 0)` 或 `setsid()`。因此 runner 与被测 case 初始共享同一个进程组。

这会把 LTP / libc / shell 脚本中的进程组操作放大成 runner 级风险：

- `kill(0, sig)` 会作用于调用者当前进程组；
- `kill(-pgid, sig)` 会作用于指定进程组；
- 如果被测 case 和 `user-test` 在同一个进程组，case 触发 fatal group kill 时可能
  同时杀掉 runner；
- 如果 `user-test` 仍和 `init` 同组，风险还可能扩展到整个用户态启动链路。

这不是内核 job-control 语义本身的缺陷。当前 register 已经把完整 job-control 能力
记录为 stage-1 限制；本问题更具体：`user-test` 作为测试设施，没有给每个被测 case
建立足够窄的进程组边界。

## Evidence

- `anemone-apps/init/src/main.rs`：`init` clone 后 child 直接
  `execve("/bin/user-test", ...)`，没有切 PGID。
- `anemone-kernel/src/task/api/clone/mod.rs`：new thread group 继承
  `current_tg.pgid()` / `current_tg.sid()`。
- `anemone-apps/user-test/src/ltp.rs`：`run_ltp_case()` 直接 `fork()` / `execve()`，
  child 执行前没有进程组隔离。
- `anemone-kernel/src/task/sig/api/kill.rs`：`kill(0, sig)` 和 `kill(-pgid, sig)`
  已按进程组广播。
- LTP pan runner 的 child 在 exec 前调用 `setpgrp()`，parent 把 child pid 记录为该
  case 的 pgrp；中断、超时或清理时对 `-pgrp` 发信号，而不是清理 runner 自己的当前
  进程组。

## Scope

本轮只改进用户态测试设施，不做内核 job-control 语义扩展。

包含：

- 为每个 LTP case 创建独立进程组；
- 保持被测 case 仍处于原 session，避免改变 `setsid` / `setpgid` 类测例的初始语义；
- 保留当前 `wait4(pid)` 的按主进程等待方式。

不包含：

- 不实现完整 controlling tty、foreground/background process group、orphaned pgrp
  `SIGHUP` / `SIGCONT`；
- 不把 `user-test` 改造成完整 `runltp` / `ltp-pan`；
- 不默认使用 `setsid()` 隔离每个 case；
- 不在本轮新增 timeout / interrupt / orphan cleanup 机制；后续 cleanup 必须只作用于
  case pgrp。

## Solution

在 `run_ltp_case()` 的 child 分支里，`chdir()` / `execve()` 前调用：

```text
setpgid(0, 0)
```

成功后，该 case 的 PGID 等于它自己的 PID。runner parent 仍用 fork 返回的 pid 等待
该 case 主进程；同一个 pid 也可以作为后续 cleanup 的 case PGID。

如果 `setpgid(0, 0)` 失败，child 应打印明确日志并 `exit(127)`，不要继续执行被测
case。这里 fail closed 更合适：隔离失败时继续跑 case，反而会保留原始 blast
radius。当前 runner 会把 127 归为普通 LTP `FAIL`；如果后续需要更准确统计，可以单独
把 `setpgid` / `execve` 这类基础设施失败归入 `infra_failed`。

本轮不补全 `ltp-pan` 的 timeout / interrupt / orphan cleanup 机制。后续如果给
`user-test` runner 增加超时、中断或失败清理，应对 `-case_pid` 发信号，而不是对
runner 当前进程组发信号。实现 parent-side cleanup 前，还必须先处理 child 尚未完成
`setpgid(0, 0)` 时 parent 就尝试清理 `-case_pid` 的竞态。

`ltp-pan` 会用 `kill(-pgrp, 0)` 判断 case 主进程退出后是否还有残留进程组成员。
Anemone 当前内核 `kill` 路径已经支持 signal 0 probe：它只做存在性 / 权限检查，不把
信号发布到 pending queue。但 `anemone-rs` 的 typed `SigNo` wrapper 没有表达 signal
0 的高层接口；如果后续实现 orphan pgrp probing，应使用底层 syscall wrapper 或补一
个明确的 typed probe API，不要把 probe 伪装成普通 `SigNo`。

`user-test` 自身是否需要从 `init` 进程组里拆出来保持为 follow-up。优先级低于
per-case 隔离，因为 LTP blast radius 的主要来源是 case 与 runner 共享进程组。该
follow-up 也不应默认使用 `setsid()`，除非明确需要新的 session。

## Change

本文档从 private qdev 草案提升为公开小迭代记录，并修复两项实现前 review concern：

- signal 0 probing 的前提从“不确定是否支持”修正为“内核支持，typed wrapper 缺少高层
  probe 表达”；
- 本轮 scope 收紧为只做 per-case pgrp isolation，timeout / interrupt / orphan
  cleanup 明确延期。

代码实现尚未开始。预期最小代码改动只涉及
`anemone-apps/user-test/src/ltp.rs`：引入 `process::setpgid`，并在
`run_ltp_case()` child 分支 `execve()` 前调用 `setpgid(0, 0)`。

## Validation

文档层提升验证：

- `git diff --check` 通过。
- `mdbook build docs` 通过。

实现后建议按从小到大的顺序验证：

1. 构建验证：
   - `just xtask app build user-test --arch riscv64`
   - `git diff --check -- anemone-apps/user-test/src/ltp.rs`
2. 定向行为验证：
   - 临时跑一个打印 `getpgrp()` / `getpid()` 的 case，确认 case PGID 等于自身 PID，
     且不同于 runner；
   - 跑一个在 case 内执行 `kill(0, SIGTERM)` 的定向用例，确认 runner 不被杀；
   - 跑一个会创建子进程的 case，确认子进程继承该 case pgrp。
3. LTP smoke：
   - `kill06` / `kill08`：覆盖负 pid / 当前 pgrp kill；
   - `setpgid01` / `setpgid02` / `setpgid03`；
   - `setsid01`：确认不因 runner 使用 `setsid()` 改变初始条件。

端到端 LTP / QEMU 验证仍按用户当前习惯执行；本记录只给出应验证的最小语义点。

## Tracking Issues

### CHG-001 - Signal 0 probe 前提过时

**Status:** Neutralized
**Severity:** Euclid

**Issue:** 草案原文把 `kill(-pgrp, 0)` 建立在“Anemone 是否支持 signal 0 仍需确认”的
不确定前提上。当前内核已经有 null signal probe 语义；真正缺口是 high-level typed
wrapper 没有 signal 0 表达。

**Resolution:** 已折回 `Solution`：orphan probing 延期，但后续实现时应使用底层
syscall wrapper 或新增 typed probe API。

### CHG-002 - Cleanup 范围与计划不一致

**Status:** Neutralized
**Severity:** Euclid

**Issue:** 草案原 Scope 把 cleanup 写成本轮包含项，但 Plan 又允许 cleanup 等以后再补，
会让实现阶段不清楚是否要顺手引入 timeout / interrupt / orphan cleanup。

**Resolution:** 已折回 `Scope` 和 `Solution`：本轮只做 per-case pgrp isolation；
cleanup 作为 follow-up，且未来 parent-side cleanup 必须处理 case pgrp 尚未建立时的竞态。

## Risk / Follow-up

- 如果 `setpgid(0, 0)` 失败后 child `exit(127)`，当前 runner 统计会显示 LTP `FAIL`，
  不会自动归入 `infra_failed`。
- 后续若新增 cleanup，应先确认 case pgrp 已建立，再对 `-case_pid` 发送 `SIGTERM` /
  `SIGKILL`。
- 如果后续开始讨论 controlling tty、foreground process group、orphaned pgrp
  `SIGHUP` / `SIGCONT`，那已经超出这份小迭代记录，应回到 job-control stage-1 限制或
  另起正式设计。

## Links

- Biweekly devlog: [2026-05-25 至 2026-06-07](../2026-05-25_to_2026-06-07.md)
- Register / limitations: [当前限制：process group / session / job control](../../register/current-limitations.md#ane-20260527-process-group-session-stage1)
