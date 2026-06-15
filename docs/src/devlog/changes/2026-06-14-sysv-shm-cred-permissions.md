# ANE-CHG-20260614-sysv-shm-cred-permissions

**Type:** Small Feature / Compatibility
**Status:** Completed
**Date:** 2026-06-14
**Authors:** doruche, Codex
**Area:** mm / SysV shm / credentials / LTP

## Problem

SysV shm 的 registry、ABI layout、attach 生命周期和 `SHM_LOCKED` 可见 metadata 已经有
stage-1 实现，但早期落地时 credentials 体系尚未合入，所以权限 helper 仍是
always-allow。这样会让 `shmget04`、`shmat02` 和 `shmctl02` 这类 LTP 权限测例无法区分
普通 DAC 失败、owner/admin 失败和 `SHM_STAT_ANY` 的 Linux 特例。

这一问题适合作为小迭代处理：SysV shm 已经有内部 `ShmPerm` metadata，`shmget`、
`shmat` 和 `shmctl` 的权限调用点也集中在 API 层。本轮只把 shm object 接回现有
credentials snapshot，不重做 registry、VMA attach 或 SysV msg/sem 权限模型。

## Scope

本轮覆盖：

- `shmget` 创建段时记录当前 effective uid/gid 到 `uid/gid/cuid/cgid`。
- keyed `shmget` 命中已有段后按请求 mode bits 做 SysV IPC DAC check。
- `shmat` 按 `SHM_RDONLY`、默认 read-write 和 `SHM_EXEC` 计算 read/write/execute
  access mask。
- `IPC_STAT` / `SHM_STAT` 做 read permission check。
- `SHM_STAT_ANY` 保持 Linux `/proc/sysvipc/shm` 风格的传统 DAC bypass。
- `IPC_SET` / `IPC_RMID` 需要 owner/creator 或 `CAP_SYS_ADMIN`。
- `SHM_LOCK` / `SHM_UNLOCK` 需要 owner/creator 或 `CAP_IPC_LOCK`，但仍只更新
  `SHM_LOCKED` metadata bit。
- `IPC_SET` 只更新 `uid/gid/mode`，保留 `cuid/cgid`。

本轮不覆盖真实 `SHM_LOCK` page pin / unpin、`RLIMIT_MEMLOCK` 账本、hugetlb 权限语义、
`/proc/sysvipc/shm`、可写 sysctl、kernel config fixture、rlimit/coredump 辅助设施，
也不为 SysV msg/sem 提前抽象统一 IPC permission helper。

## Solution

新增 shm-local permission helper，而不是复用 VFS `FsPermChecker`。SysV IPC 使用当前
effective uid/gid 和 supplementary groups；VFS 普通访问使用 fsuid/fsgid，把两者合并会让
后续 `setfsuid()` 边界变得不清楚。

`ShmCredView` 在 syscall/API 层由 `Task::cred()` snapshot 构造，只携带 euid、egid、
owned supplementary groups，以及 `CAP_IPC_OWNER`、`CAP_IPC_LOCK`、`CAP_SYS_ADMIN` 三个
布尔能力。registry 和 segment 只保存 IPC object metadata，不获取 current task，也不保存
完整 `CredentialSet`。

`ShmPerm` / `ShmPermUpdate` 内部改用 typed `Uid` / `Gid`。Linux `ipc_perm` 的 raw
`u32` uid/gid 只在 `shmctl` copy-in / copy-out 边界转换，继续保持 ABI struct 不进入 shm
core。

## Change

- 新增 `mm::uspace::shm::permission`，承载 `ShmCredView`、`ShmPermAccess`、
  `ShmControlAccess`、DAC helper 和 control helper。
- `shmget` 创建路径从当前 credential snapshot 写入 object owner / creator metadata；
  existing-key 路径保持 `EEXIST`、size `EINVAL`、DAC `EACCES` 的 errno 顺序。
- `shmat` 在 attach reservation 后、VMA 安装前执行 credential-aware DAC check；失败时
  取消 reservation 并按既有 reclaim 规则释放 segment。
- `shmctl` 为 `IPC_STAT` / `SHM_STAT` 接 read DAC，为 `IPC_SET` / `IPC_RMID` 接
  owner/admin gate，为 `SHM_LOCK` / `SHM_UNLOCK` 接 lock/admin gate；`SHM_STAT_ANY` 在
  调用点显式绕过 DAC。
- `IPC_SET` 调整为先 copy-in 用户 `shmid_ds`，再 lookup 和 owner/capability check。
- `anemone-apps/shm-test` 增加 fork 后 `setuid(65534)` 的本地 smoke，覆盖
  unprivileged `shmget`、`shmat`、`IPC_STAT`、`IPC_SET`、`IPC_RMID`、`SHM_LOCK`、
  `SHM_UNLOCK` 的拒绝路径，以及 `SHM_STAT_ANY` 的 DAC bypass。

## Validation

- implementation worker 运行 `just fmt kernel`、`just fmt shm-test`、
  `just xtask app build shm-test --arch riscv64`、
  `just xtask app build shm-test --arch loongarch64`、`git diff --check` 和
  `just build` 通过；`just build` 仅报告既有 `sync/mono.rs` unused-import warning。
- review agent 未发现 Apollyon / Keter 阻塞项。
- 主控补充 `SHM_UNLOCK` denial smoke 后，`just fmt shm-test`、
  `just xtask app build shm-test --arch riscv64` 和
  `just xtask app build shm-test --arch loongarch64` 通过。
- `git diff --check` 通过；新增 `permission.rs` 用 `git diff --no-index --check`
  检查无 whitespace warning。
- `mdbook build docs` 通过。
- 主控最终 `just build` 通过；build 仅报告既有 `sync/mono.rs` unused-import warning。
- 本轮 agent 侧未运行 QEMU / LTP runtime profile；`shmget04`、`shmat02`、`shmctl02`
  仍需要后续 user-test profile 验证。

## Tracking Issues

### CHG-001 - LTP runtime verification

**Status:** Deferred
**Severity:** Safe

**Issue:** 本轮只做源码、build 和本地 `shm-test` app build 验证，没有运行
`shmget04`、`shmat02`、`shmctl02` 的 QEMU / LTP runtime profile。

**Resolution:** 保留为 register residual limitation。后续运行 SysV shm 权限小 profile 后，
如果 `shmget04`、`shmat02`、`shmctl02` 的 glibc / musl 结果都闭合，再关闭对应
permissions limitation。

### CHG-002 - group and execute allow-path smoke

**Status:** Deferred
**Severity:** Safe

**Issue:** 本地 smoke 覆盖 unprivileged denial 和 `SHM_STAT_ANY` bypass，但没有覆盖
supplementary group allow path 或 `SHM_EXEC` execute-bit allow/deny path。

**Resolution:** 这些属于更细的 runtime coverage，不阻塞本轮核心 DAC/control hook。若后续
LTP 或本地 case 暴露 group/execute 分支偏差，再补定向 smoke。

## Risk / Follow-up

- `SHM_LOCK` / `SHM_UNLOCK` 现在有 credential gate，但仍不提供真实 page residency、
  memlock accounting 或 `RLIMIT_MEMLOCK` 失败路径；该限制继续由
  `ANE-20260525-SYSV-SHM-LOCK-RESIDENCY-STAGE1` 跟踪。
- `SHM_HUGETLB` 仍沿用既有 compatibility no-op 和日志路径，本轮不新增
  `CAP_IPC_LOCK` gate。
- `shmctl04` 的 `SHM_STAT_ANY` 权限语义已在代码和 smoke 中覆盖，但该 LTP case 还会比较
  `/proc/sysvipc/shm` 视图，完整通过仍依赖 SysV shm LTP infra 后续项。

## Links

- Biweekly devlog: [2026-06-08 至 2026-06-21](../2026-06-08_to_2026-06-21.md)
- Register / limitations: [SysV shm permissions residual](../../register/current-limitations.md#ane-20260525-sysv-shm-permissions-pending-credentials), [SysV shm lock residency stage-1](../../register/current-limitations.md#ane-20260525-sysv-shm-lock-residency-stage1), [SysV shm LTP infra](../../register/current-limitations.md#ane-20260529-sysv-shm-ltp-infra-stage1)
