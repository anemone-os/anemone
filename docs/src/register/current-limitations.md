# 当前限制

本页记录当前已接受的限制。这些条目不是未知异常，而是当前阶段明确存在、后续需要系统性收敛的能力缺口。

## ANE-20260522-OTMPFILE-STAGE1

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** VFS / openat

**Summary:** 当前 O_TMPFILE 采用 create-open-unlink 的 stage-1 仿真实现，不具备真正匿名 inode、强原子性或后续 link 回目录的完整语义。

**Exit Condition:** 实现文件系统支撑的无名临时 inode，并补齐 linkat、AT_EMPTY_PATH 与 O_EXCL 相关语义。

**Owner:** doruche
**Last Verified:** 2026-05-22
**Related:** [开发日志：2026-05-11 至 2026-05-24](../devlog/2026-05-11_to_2026-05-24.md)

## ANE-20260523-TRUNCATE-MMAP-COHERENCY

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** VFS / truncate / mmap

**Summary:** 当前 truncate 会更新 inode 大小并裁剪驻留文件页缓存，但不会主动失效已经安装到用户地址空间的文件映射，因此 live mmap 下不承诺 Linux 级的强一致性或完整 SIGBUS 语义。

**Exit Condition:** 为文件映射补齐 shrink 场景下的映射失效或回收路径，并明确验证 truncate 与 mmap 在 grow、shrink 和并发访问下的可见性语义。

**Owner:** doruche
**Last Verified:** 2026-05-23
**Related:** [开发日志：2026-05-11 至 2026-05-24](../devlog/2026-05-11_to_2026-05-24.md)

## ANE-20260523-EXT4-TRUNCATE-CACHE-INVALIDATION

**Type:** Limitation
**Status:** Active
**Severity:** Low
**Area:** ext4 / truncate / page cache

**Summary:** 当前 ext4 truncate 在更新磁盘镜像后，会按页粒度失效“可见字节范围发生变化”的 resident page cache，而不是对边界页做原位修补并继续信任其内存内容。

**Exit Condition:** 把之前 shrink-then-extend 暴露旧字节的问题继续收敛到明确根因，并以可靠的边界页原位修补或更强的一致性不变量替换当前的页粒度失效策略，同时重新验证 resident page cache 与 truncate grow/shrink 的可见性语义。

**Owner:** doruche
**Last Verified:** 2026-05-23
**Related:** [开发日志：2026-05-11 至 2026-05-24](../devlog/2026-05-11_to_2026-05-24.md), [当前限制](./current-limitations.md)

## ANE-20260524-DEVFS-STATIC-PUBLISH

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** devfs / device model

**Summary:** 当前 devfs 第一版主要只支持启动期静态 publish 到扁平 `/dev` 根目录；为了 `user-test` 的 `ramfs` 挂载，另有一个静态 `/dev/shm` 目录挂载点，但这不代表通用目录层级能力。不支持运行期 unpublish/hot-unplug、别名或 symlink。

**Exit Condition:** 只有在真实设备热插拔或多级命名空间需求出现后，再为 devfs 增加显式的发布失效协议、目录发布能力与相应的 dentry/inode 回收路径。

**Owner:** doruche
**Last Verified:** 2026-05-24
**Related:** [开发日志：2026-05-11 至 2026-05-24](../devlog/2026-05-11_to_2026-05-24.md)

## ANE-20260524-DEVFS-BLOCK-DEFAULT-SEMANTICS

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** block / devfs

**Summary:** 当前 block 子系统默认 `/dev` helper 仍采用块对齐 seek/read/write 语义，不提供 Linux 风格更接近字节流的块设备文件兼容层，也不提供 waitable poll。

**Exit Condition:** 明确目标 userspace 对块设备节点的兼容性需求后，为默认 block helper 增加所需的兼容层或正式收敛成受文档约束的语义，并补齐对应验证。

**Owner:** doruche
**Last Verified:** 2026-05-24
**Related:** [开发日志：2026-05-11 至 2026-05-24](../devlog/2026-05-11_to_2026-05-24.md)

## ANE-20260525-SYSV-SHM-MUNMAP-DETACH

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** mm / SysV shm

**Summary:** 当前 SysV shm 只保证通过 `shmdt`、`fork`、`exec` 和进程退出维护 attach 生命周期；如果用户直接对 shm 映射调用 `munmap`，内核不会把它视为一次 SysV shm detach，也不会同步修正 `shm_nattch` 或 attachment 记录。

**Exit Condition:** 为 VMA 增加可追踪的 SysV shm 来源标记或 unmap hook，使任意 `munmap` 路径都能一致驱动 detach / 计数回收，并用专门回归验证 partial unmap、whole unmap 与 `shmdt` 的一致性。

**Owner:** doruche
**Last Verified:** 2026-05-25

## ANE-20260525-SYSV-SHM-LOCK-NOOP

**Type:** Limitation
**Status:** Active
**Severity:** Low
**Area:** mm / SysV shm

**Summary:** 当前 `SHM_LOCK` / `SHM_UNLOCK` 只保留兼容入口和日志，不实际执行页锁定、解锁或 `SHM_LOCKED` 状态维护，因此不会产生 Linux 风格的驻留锁页语义。

**Exit Condition:** 为 SysV shm 接入真实的页锁定路径、权限校验与 `SHM_LOCKED` 状态同步，并补齐相关回归测试。

**Owner:** doruche
**Last Verified:** 2026-05-25

## ANE-20260525-SYSV-SHM-PERMISSIONS-PENDING-CREDENTIALS

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** mm / SysV shm / credentials

**Summary:** 当前一期不纳入权限敏感的 SysV shm 测例，像 `shmat02`、`shmctl02` 这类依赖 `setuid`/`setgid` 和有效 uid/gid 切换的路径，仍会受限于现有 credentials 线的未完成状态。

**Exit Condition:** 单独实现 credentials 的真实有效/真实 uid/gid 语义、`setuid`/`setgid` 行为和 IPC 权限检查之后，再把这些权限敏感测例纳入 SysV shm 白名单并回归验证。

**Owner:** doruche
**Last Verified:** 2026-05-25

## ANE-20260526-SIGNAL-RESTORER-LEGACY-COMPAT

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** signal / ABI

**Summary:** 当前 signal ABI 仍按 legacy `sa_restorer` 语义兼容老测例，主要目标是 `musl 1.2.2` 的 `pthread_cancel`；`glibc 2.3.5` 只是对照参考，不引入面向新内核头的条件编译分支来切换 UAPI 结构。

**Exit Condition:** 当需要同时支持新旧用户态头文件时，再单独设计一层可配置的 signal UAPI 适配，并补齐 musl / glibc / libc-test 的回归验证。

**Owner:** doruche
**Last Verified:** 2026-05-26
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

## ANE-20260527-PROCESS-GROUP-SESSION-STAGE1

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** task topology / process group / session / job control

**Summary:** 当前进程组与会话实现是从 `69bff4b` 之后引入的 stage-1 主干，已经覆盖 PGID/SID 拓扑、`setpgid` / `getpgid` / `setsid` / `getsid`、process-group `kill` 和 `wait4` 的基础选择语义，但还不是完整 job-control 实现；尚未接入 controlling tty、foreground/background process group、terminal job-control 信号、orphaned process group 的 `SIGHUP` / `SIGCONT` 规则，也尚未提供 `waitid`。

**Exit Condition:** 补齐 `waitid` 的 P_PID / P_PGID / P_ALL 基础语义，接入 controlling tty 和 foreground process-group 管理，并为 background terminal access、session leader 退出、newly orphaned stopped process group 等路径补齐 Linux/POSIX 对齐的回归测试。

**Owner:** doruche
**Last Verified:** 2026-05-27
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

## ANE-20260527-MMAP-LOCK-SYNC-STAGE1

**Type:** Limitation
**Status:** Active
**Severity:** Low
**Area:** mm / mlock / msync

**Summary:** 当前 `mlock` / `munlock` 只做页面覆盖校验，不记录 lock 状态，也不做 swap 驻留控制；`msync` 只对连续覆盖到的已映射区间做同步，遇到 hole 时直接返回错误，不承诺 Linux 那种部分覆盖继续推进的语义。

**Exit Condition:** 接入真实页锁定/解锁账本，并把 `msync` 改成按 VMA / file-backed range 逐段处理，重新验证 `VmLck`、holes 和 `MS_INVALIDATE` 行为。

**Owner:** doruche
**Last Verified:** 2026-05-27
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

## ANE-20260527-MREMAP-ANON-ONLY

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** mm / mremap

**Summary:** 当前 `mremap` 只适合单个、可按匿名风格编辑的 VMA；如果旧区间跨越多个 VMA，或者目标需要保留 file-backed / shared backing 与 `pgoff`，现有实现会把尾部按匿名模板重建，语义会偏离 Linux。

**Exit Condition:** 为 backing-aware grow / move 单独建路径，或者在入口显式拒绝不支持的 VMA 类型，并补齐对应回归。

**Owner:** doruche
**Last Verified:** 2026-05-27
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

## ANE-20260527-FALLOCATE-BASIC-REGULAR-ONLY

**Type:** Limitation
**Status:** Active
**Severity:** Low
**Area:** fs / fallocate

**Summary:** 当前 `fallocate` 只开放普通文件上的基本延展语义，并且只接受 `FALLOC_FL_KEEP_SIZE` 这一类兼容入口；洞填充、零范围、collapse / insert / unshare 等模式，以及特殊文件类型，都还处在兼容收口阶段。

**Exit Condition:** 为不同文件系统和文件类型补齐真正的 `fallocate` 后端，再逐步放开更多 mode 组合和对应的 errno 约束。

**Owner:** doruche
**Last Verified:** 2026-05-27
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

## ANE-20260527-PWRITEV2-FLAGS-STAGE1

**Type:** Limitation
**Status:** Active
**Severity:** Low
**Area:** fs / pwritev2 / vectored IO

**Summary:** 当前 `pwritev2` 是为了收口 LTP 基础 errno 和 offset 语义的 stage-1 入口，只支持 `flags == 0` 与 `offset == -1` 的 current-position 行为；非零 `RWF_*` flags 统一返回 `EOPNOTSUPP`，尚不提供 per-IO append、sync、nowait 或 hipri 语义。

**Exit Condition:** 明确逐项实现或文档化拒绝 `RWF_HIPRI`、`RWF_DSYNC`、`RWF_SYNC`、`RWF_NOWAIT`、`RWF_APPEND` 等 flags，并补齐 `pwritev2` flags 组合的回归验证。

**Owner:** doruche
**Last Verified:** 2026-05-27
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

## ANE-20260528-PROC-TGID-FD-FRAMEWORK-PENDING

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** procfs / fd / path visibility

**Summary:** 当前 procfs 还没有 `/proc/<tgid>/fd` 目录框架。glibc 的 `realpath("/tmp")` 可通过普通 `readlink("/tmp") -> EINVAL` 路径完成，但 musl 的 `realpath` 在某些路径上会依赖 `readlink("/proc/self/fd/<n>")`，因此 `getcwd02` 的 musl 变体仍会因 `/proc/self/fd/3` 不存在而 `ENOENT`。

**Exit Condition:** 引入系统性的 `/proc/<tgid>/fd` 目录实现，基于目标 thread group 的 fd 表提供 `readdir`、`readlink`、`stat/open` 所需的稳定语义，并明确 fd 生命周期、权限和路径可见性规则；完成后重新验证 musl `getcwd02` 及依赖 fd symlink 的相关 libc 路径。

**Owner:** doruche
**Last Verified:** 2026-05-28
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

## ANE-20260528-RAMFS-RENAME-STAGE1

**Type:** Limitation
**Status:** Active
**Severity:** Low
**Area:** ramfs / rename

**Summary:** 当前 `ramfs` 的 `rename` 是为了收口 LTP `getcwd04` regular-file 链式改名而加入的 stage-1 实现，只支持同一 superblock 内非目录项改名、普通覆盖和 `RENAME_NOREPLACE`。目录 rename、循环检测、跨目录目录树移动以及更完整的 Linux rename flag 组合仍未实现。

**Exit Condition:** 为 `ramfs` 补齐 directory rename 的父子关系维护、空目录/非空目录覆盖规则、循环防护和需要支持的额外 rename flags，并增加覆盖 cross-directory、directory 与 overwrite 场景的回归测试。

**Owner:** doruche
**Last Verified:** 2026-05-28
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)
