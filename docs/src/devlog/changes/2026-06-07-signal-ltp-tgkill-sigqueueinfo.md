# ANE-CHG-20260607-signal-ltp-tgkill-sigqueueinfo

**Type:** Bugfix / Investigation
**Status:** Completed
**Date:** 2026-06-07
**Authors:** doruche, Codex
**Area:** signal / syscall ABI / LTP

## Problem

`build/user-test-rv64.log` 的 signal profile 中有两类明确 syscall 语义错误：

- `tgkill03` 对负数 `tgid` / `tid` 期望 `EINVAL`，当前 `Tid(u32)` 参数解析会把负数转换成大号 TID，最终查找失败并返回 `ESRCH`。
- `rt_sigqueueinfo01` 在线程中用 `gettid()` 得到非 leader TID 后调用 `rt_sigqueueinfo(tid, SIGUSR1, uinfo)`，当前实现只按 TGID 查找线程组，非 leader TID 会返回 `ESRCH`。

同一日志里还有 `tgkill02`、`rt_sigaction01` / `rt_sigaction02`、`kill02` 和若干 LTP 设施缺口；这些不在本次窄修范围内，转入 register 跟踪。

## Scope

本次只修 `tgkill03` 和 `rt_sigqueueinfo01` 的直接语义问题：

- `tgkill(2)` 的 `pid_t` 参数入口校验。
- `rt_sigqueueinfo(2)` 对非 leader TID 的目标解析。

本次不实现 realtime signal queue resource accounting、`RLIMIT_SIGPENDING`、`rt_sigaction` 信号编号上界策略、`kill02` timeout 根因修复、`/proc/sys/kernel/pid_max`、`getrlimit(RLIMIT_CORE)` 或 LTP kconfig fixture 扩展。

## Solution

`tgkill` 改为接收 signed `pid_t` 形态的 `i32`，在转换成 `Tid` 前显式拒绝 `tgid <= 0 || tid <= 0`，保持 Linux `tgkill` 对非正 task identity 返回 `EINVAL` 的边界。

`rt_sigqueueinfo` 改为先按 `PIDTYPE_PID` 风格解析传入 ID：用 `get_task(&Tid)` 找到目标 task，再把 signal 作为 process-directed signal 递送到该 task 所属 thread group。这样非 leader `gettid()` 不会被误当成 TGID 查找失败，同时仍保留 Linux 中 `kill_proc_info()` 的 process-directed pending queue 语义。

## Change

- `anemone-kernel/src/task/sig/api/tgkill.rs`：入口参数从 `Tid` 改为 `i32`，加入非正 pid/tid 的 `EINVAL` 校验，再转换为 `Tid` 继续既有 thread-group membership 检查。
- `anemone-kernel/src/task/sig/api/rt_sigqueueinfo.rs`：入口参数从 `Tid` 改为 `i32`，目标查找从 `get_thread_group(&pid)` 改为 `get_task(&pid)`，权限检查用解析到的 task，最终仍投递到目标 task 的 thread group。
- `docs/src/register/open-issues.md`：记录 signal profile 剩余语义 / runtime 缺口。
- `docs/src/register/current-limitations.md`：记录 signal LTP 设施 / 可观察面缺口。

## Validation

`just build` 通过；`git diff --check` 通过。未运行 QEMU / LTP，`tgkill03` 和 `rt_sigqueueinfo01` 的运行态确认等待用户复跑 signal profile。

## Tracking Issues

### CHG-001 - Signal profile 剩余语义缺口

**Status:** Deferred
**Severity:** Euclid

**Issue:** `tgkill02`、`rt_sigaction01` / `rt_sigaction02` 和 `kill02` 仍需要后续单独处理。

**Resolution:** 升级为 register 条目 `ANE-20260607-SIGNAL-LTP-REMAINING-SEMANTICS`。

### CHG-002 - Signal profile LTP 设施缺口

**Status:** Deferred
**Severity:** Safe

**Issue:** `kill03` / `tkill02` 依赖 `/proc/sys/kernel/pid_max`，`kill11` 依赖 `getrlimit(RLIMIT_CORE)`，`kill13` 依赖 kconfig fixture 声明。

**Resolution:** 升级为 current limitations 条目 `ANE-20260607-SIGNAL-LTP-INFRA-STAGE1`。

## Risk / Follow-up

`rt_sigqueueinfo01` 预计会越过 `ESRCH`，但最终是否能收到 handler 还依赖 shared pending signal 被目标线程或同组其他线程消费的调度时序。该行为与 Linux process-directed signal 模型一致，运行态仍需用 LTP 复跑确认。

## Links

- Biweekly devlog: [2026-05-25 至 2026-06-07](../2026-05-25_to_2026-06-07.md)
- Register / limitations: [开放问题](../../register/open-issues.md), [当前限制](../../register/current-limitations.md)
