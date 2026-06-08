# ANE-CHG-20260608-pselect6-exceptfds-compat

**Type:** Bugfix / Compatibility
**Status:** Completed
**Date:** 2026-06-08
**Authors:** doruche, Codex
**Area:** fs / iomux / pselect6 / lmbench

## Problem

`build/lmbench.log` 显示 lmbench 进入 latency measurements 后反复调用 `pselect6`，其中部分调用同时传入 `inp` 和 `exp`。当前 `pselect6` 在 latch register scan 阶段把 `exceptfds` interest 视为无法 arm 的 source，`wait_for_iomux_ready()` 随后返回 `ENOTSUP`，导致 lmbench 第一组 testcase 无法按原先方式推进。

这不是 `pselect6` 整体等待迁移回退，也不是 read/write fd readiness 的 latch 协议问题；直接触发点是 exception readiness 仍没有内部 `PollEvent` / source registration，而 syscall 入口从“兼容放行、输出为空”收紧成了“register 阶段 fail closed”。

## Scope

本次只回退 `pselect6 exceptfds` 的兼容边界：

- 保留 `exceptfds` 里置位 fd 的 `EBADF` 校验。
- 保留用户态 `exceptfds` 输出清空。
- 保留 readfds / writefds 的 typed poll register fail-closed 行为。
- 不新增 `POLLPRI`、socket OOB、pty packet mode、driver priority events 或对应 latch source。
- 不修改 `ppoll`、shared iomux wait helper 或 wait-core latch 语义。

## Solution

`exceptfds` 重新作为 stage-1 compatibility no-op 处理：如果用户传入非空 exception fdset，`sys_pselect6` 打一条 notice；scan 阶段只校验 fd 存在，然后把 exception readiness 视为 not-ready。这样 defensive `exceptfds` 探测不会被 `ENOTSUP` 拦截，同时仍不会伪造任何 exception readiness。

拒绝把 `exceptfds` 映射成空 `PollEvent` register source。空 interest register 会让 wait helper 误以为 source 已经正确参与 latch 协议，掩盖真实 `POLLPRI` 缺口。本轮仅在 `pselect6` adapter 内声明兼容 stub，真实未迁移 fd source 的 register+not-ready 仍继续 fail closed。

## Change

- `anemone-kernel/src/fs/api/iomux/pselect6.rs`：`scan_pselect_fds()` 不再因为 register mode 下存在 exception interest 返回 `IomuxScanOutcome::Unsupported`；`validate_exception_fdset()` 仍负责遍历并校验 fd。
- `sys_pselect6()`：当修剪后的 `exceptfds` 非空时打印 notice，说明 exception readiness 仍是 compatibility stub。
- `docs/src/register/current-limitations.md`：新增 `ANE-20260608-PSELECT6-EXCEPTFDS-STAGE1`，把该兼容 no-op 记录为 active limitation。

## Validation

`just build` 通过；`git diff --check` 通过。未在本轮 agent 侧重跑 lmbench / QEMU；运行态确认等待复跑 `build/lmbench.log` 对应 lmbench profile。

## Tracking Issues

### CHG-001 - 真正的 exception readiness 尚未实现

**Status:** Deferred
**Severity:** Safe

**Issue:** 当前实现接受 `exceptfds` 但不会产生 exception readiness，依赖 `POLLPRI` / OOB / packet-mode 等语义的程序仍无法观察到对应事件。

**Resolution:** 升级为 current limitations 条目 `ANE-20260608-PSELECT6-EXCEPTFDS-STAGE1`。

## Risk / Follow-up

如果后续测试真正依赖 `exceptfds` 变为 ready，需要先为内部 iomux 增加明确的 exception / priority readiness 表达，再逐个 source 实现 snapshot 与 register 语义。当前 no-op 只适合“传了 `exceptfds` 但不依赖其触发”的兼容探测。

## Links

- Biweekly devlog: [2026-06-08 至 2026-06-21](../2026-06-08_to_2026-06-21.md)
- Register / limitations: [当前限制](../../register/current-limitations.md#ane-20260608-pselect6-exceptfds-stage1)
