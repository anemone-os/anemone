# ANE-CHG-20260608-iomux-empty-timeout-sleep

**Type:** Bugfix
**Status:** Completed
**Date:** 2026-06-08
**Authors:** doruche, Codex
**Area:** fs / iomux / ppoll / pselect6 / scheduler

## Problem

`build/iozone2.log` 显示 `iozone-glibc` 进入第二个 throughput 子测例后，开始大量打印 `pselect6(n=0, ...)` 和 `sys_pselect6: timeout expired before latch begin`。旧参考日志跑的是 `iozone-musl`，所以两份日志不是严格的 libc 对照；但卡住的 glibc 路径可以对应到 iozone 的 `Poll(1)` helper：父进程等待 child 启动 flag 时调用 `select(0, NULL, NULL, NULL, 1us)`。

开启内核抢占后，timer 驱动的 reschedule 会让 child task 获得 CPU 时间，因此 workload 不再卡住。但这只是遮蔽问题：empty iomux set 上的正 timeout 应该表现为 interruptible timeout sleep，而不是在发布任何 wait state 前就作为 busy probe 返回。

## Scope

本次只修复 `ppoll` 和 `pselect6` 共享的空集合 / 无 source iomux 超时路径。

- `timeout == 0` 仍保持立即返回的非阻塞 timeout。
- 真实 fd readiness 仍走 latch register 路径。
- `ppoll` 的负 fd entry 继续按 Linux 语义忽略，不计入 source。
- `pselect6 exceptfds` 继续保持当前兼容 stub，不伪装成真实可 arm source。
- 不修改 scheduler preemption policy，也不修改 iozone test harness 配置。

## Solution

`IomuxScanOutcome` 新增 `NoSources`，与 `NotReady` 区分开。syscall adapter 只有在没有真实 read/write poll source 可 snapshot 或 register 时才返回 `NoSources`；shared wait helper 收到 `NoSources` 后进入显式的 wait-core timeout sleep helper。

这样真实 fd source 仍保留 latch 协议，同时空集合行为更接近 Linux：零 timeout probe 立即返回；正 timeout 或 NULL timeout 进入 interruptible wait，让 scheduler 能观察到睡眠状态，而不是依赖内核抢占提供公平性。

## Change

- `anemone-kernel/src/fs/api/iomux/wait.rs`：新增 `IomuxScanOutcome::NoSources` 和 `wait_without_iomux_sources()`，用于 empty iomux wait。
- `anemone-kernel/src/fs/api/iomux/ppoll.rs`：当所有用户 `pollfd` 都被忽略时返回 `NoSources`，例如 `nfds == 0` 或所有 entry 都是负 fd。
- `anemone-kernel/src/fs/api/iomux/pselect6.rs`：当 read/write fdset 在按 `n` 修剪后没有任何置位 fd 时返回 `NoSources`。

## Validation

- `just fmt kernel` 通过。
- sandboxed `just build` 到达 vendored `lwext4_rust` C 编译阶段后命中已知 sandbox `Bad system call`。
- unsandboxed `just build` 通过，只保留既有的 `anemone-kernel/src/sync/mono.rs` unused-import warning。

本轮 agent 未重跑完整 iozone QEMU profile。

## Tracking Issues

### CHG-001 - iozone 运行态复核

**Status:** Deferred
**Severity:** Safe

**Issue:** 代码层修复已经 build 通过，但本记录没有包含 agent 侧新跑的 `iozone-glibc` profile，尚未用运行日志确认第二个 throughput 子测例在不依赖内核抢占的情况下通过。

**Resolution:** 保留为后续验证项。现有日志和 iozone 源码已经能确认症状与来源映射，本轮窄修落在 shared iomux wait 边界。

## Risk / Follow-up

`pselect6 exceptfds` 仍是独立的 stage-1 兼容限制：当前只校验 fd，不提供真实 exception readiness source。如果后续测试依赖 exception readiness 变为 ready，该问题仍不属于本轮 empty-timeout 修复范围。

## Links

- Biweekly devlog: [2026-06-08 至 2026-06-21](../2026-06-08_to_2026-06-21.md)
- Related change: [pselect6 exceptfds 兼容回退](./2026-06-08-pselect6-exceptfds-compat.md)
