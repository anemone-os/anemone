# ANE-CHG-20260614-procfs-sysctl-pde-tree

**Type:** Small Feature / Procfs / Sysctl / LTP Compatibility
**Status:** Completed
**Date:** 2026-06-14
**Authors:** doruche, Codex
**Area:** procfs / sysctl / pseudo inodes / SysV shm / signal / LTP

## Problem

多个 LTP profile 依赖 `/proc/sys` 下的 Linux 可观察配置项。当前最直接的缺口是
`/proc/sys/kernel/pid_max`、SysV shm 的 `shmmax` / `shmall` / `shmmni`，以及
capability profile 读取的 `cap_last_cap`。

原有 procfs 已有 `ProcDirEntry` 静态注册入口，但它只表达 `/proc` 根目录的扁平项：
root lookup 会查静态表，root readdir 却手写 `.`、`..`、`self` 和动态 `<tgid>`。
继续为 `/proc/sys/*` 手写目录和文件会让 lookup、readdir 和 inode identity 的一致性
继续依赖约定。

## Scope

本轮只扩展 procfs-owned PDE 静态树，并发布第一批只读 `/proc/sys/kernel` 节点：

- `/proc/sys/kernel/pid_max`
- `/proc/sys/kernel/shmmax`
- `/proc/sys/kernel/shmall`
- `/proc/sys/kernel/shmmni`
- `/proc/sys/kernel/cap_last_cap`

本轮不实现 `/proc/sys/fs/*`、random entropy、pipe / fanotify / vm knobs、
`/proc/sysvipc/shm`、通用 sysctl parser、可写 sysctl transaction、tgid 动态树迁移，
也不把 private draft 路径写入公开文档。

## Solution

`ProcDirEntry` 从扁平 root 表扩展为 procfs 静态树描述符，支持目录、通用短内容文件、
通用 symlink 和 `Custom` escape hatch。PDE 静态树只表达 procfs 名字、拓扑、inode
identity、元数据和节点行为分发；具体 sysctl 内容仍由 owning subsystem 的常量或 helper
临时生成。

`/proc` root 仍是组合层：先枚举 PDE root children，再枚举动态 `<tgid>`；lookup 先查
PDE root children，再按数字 tgid 进入动态树。`/proc/<tgid>/...` 继续由 tgid 动态树拥有，
不纳入 PDE lifecycle。

具体目录项按 procfs 目录结构拆分：`fs/proc/sys/` 承载 `/proc/sys`，`fs/proc/sys/kernel/`
承载 `/proc/sys/kernel`，叶节点各自放在独立 rs 文件中。`pde.rs` 只保留 PDE 基础设施和
root 静态表引用，不承载具体 sysctl 取值逻辑。

## Change

- `ProcDirEntry` 新增 `Dir` / `File` / `Symlink` / `Custom` kind，probe 阶段递归分配
  stable inode 并 seed 到 procfs singleton superblock icache。
- 通用 PDE inode private data 保存 `&'static ProcDirEntry` 和 parent inode number；
  generic ops 只从该 owner 指针分发行为空间，`ino` 只作为 icache identity 见证。
- 通用 PDE 目录支持 lookup / readdir，目录 readdir 输出 `.`、`..` 后枚举 children。
- 通用 PDE 文件每次 read / read_at / seek 重新生成短内容；poll 报 readable；
  ioctl 返回 `UnsupportedIoctl`；没有 write callback 的节点 fail closed。
- `/proc/self` 和 `/proc/mounts` 迁到 generic symlink callback；`/proc/uptime` 和
  `/proc/meminfo` 保留原有 custom ops。
- `/proc/sys/kernel/{pid_max,shmmax,shmall,shmmni,cap_last_cap}` 以 `0444` 发布，内容为
  十进制值加换行，分别来自 `MAX_PROCESSES`、SysV shm 常量和 `CAP_LAST_CAP`。

## Validation

- `git check-ignore -v` 确认 private draft 仍在 gitignored 的本地工作材料范围。
- implementation worker 首轮运行 `just build` 和 `git diff --check` 通过。
- review agent 首轮未发现 Apollyon / Keter；发现的 `ProcDirEntry.kind` 与
  `mode.ty()` 分裂风险已通过 seed-time `assert!` 修复，并经 review agent 窄 recheck
  确认为 Neutralized。
- 主控重构具体 sysctl 节点到 `fs/proc/sys/kernel/` 后，`just build` 通过；build 仅保留
  既有 `sync/mono.rs` unused-import warning。
- `git diff --check` 通过。
- `just fmt kernel --check` 仍只在 generated `kconfig_defs.rs` / `platform_defs.rs` 上报告
  既有格式差异；本轮新增和修改的 procfs 文件无 fmt diff。
- 本轮 agent 侧未运行 QEMU / LTP runtime profile。

## Tracking Issues

### CHG-001 - PDE kind and mode split

**Status:** Neutralized
**Severity:** Euclid

**Issue:** `ProcDirEntry.kind` 驱动行为分发，而 `mode.ty()` 驱动 inode type、`d_type`
和 nlink。若后续目录项写错，可能出现 symlink-looking file、regular-looking dir 等分裂。

**Resolution:** `seed_pde_tree()` 在分配 inode、初始化 `MonoOnce` 和 seed icache 前内联断言
非 `Custom` PDE 的 kind / mode 一致性：`Dir -> InodeType::Dir`、
`File -> InodeType::Regular`、`Symlink -> InodeType::Symlink`。

### CHG-002 - runtime procfs coverage

**Status:** Deferred
**Severity:** Safe

**Issue:** 本轮已完成源码级和 build 验证，但没有运行 QEMU / LTP runtime profile 来证明
root getdents、`/proc/sys/kernel` getdents、read_at / seek 和写入 fail-closed 行为。

**Resolution:** 作为后续 runtime 验证项保留。当前公开记录只声明源码和 build 层关闭，
不声称相关 LTP case 已通过。

## Risk / Follow-up

- 第一批 sysctl 文件是只读观察面；需要 save / restore 写 sysctl 的 LTP case 仍不应标为
  已关闭。
- `/proc/sysvipc/shm`、kernel `.config` fixture、`getrlimit(RLIMIT_CORE)`、random
  entropy、pipe sysctl、fanotify limit 和 VM knobs 继续留作后续工作。

## Links

- Biweekly devlog: [2026-06-08 至 2026-06-21](../2026-06-08_to_2026-06-21.md)
- Register / limitations: [SysV shm LTP infra](../../register/current-limitations.md#ane-20260529-sysv-shm-ltp-infra-stage1), [Signal LTP infra](../../register/current-limitations.md#ane-20260607-signal-ltp-infra-stage1)
