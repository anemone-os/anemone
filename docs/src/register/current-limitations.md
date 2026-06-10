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

## ANE-20260529-FILE-BACKED-MMAP-FAULT-STAGE1

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** mm / mmap / file-backed mapping

**Summary:** 当前 file-backed mmap 已能覆盖基础映射与权限 errno，但 page fault 路径仍是 stage-1。稀疏扩展文件或未分配洞页读取可能从 ext4/lwext4 返回 `InvalidArgument` 并触发 `SIGSEGV`，EOF 后映射页也仍通过 `NotMapped -> SIGSEGV` 暴露，尚未实现 Linux 风格的洞页零填充与 EOF 后 `SIGBUS` 分流。

**Exit Condition:** 明确 file-backed backing fault 的错误域，支持文件洞页零填充或对应页缓存语义，并让 fault 顶层能区分“无 VMA / guard hole”与“VMA 存在但 backing 不可提供页面”，重新验证 LTP `mmap001`、`mmap13` 以及 truncate / mmap 交互。

**Owner:** doruche
**Last Verified:** 2026-05-29
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md), [当前限制](./current-limitations.md)

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

## ANE-20260605-DEVFS-CHAR-SEEK-IOCTL-STAGE1

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** devfs / char device / lseek / ioctl

**Summary:** 当前字符设备 devfs 入口只提供统一的 read/write 分发，`validate_seek` 固定返回 unsupported，`ioctl` 也固定返回 unsupported。这样会让 Linux 上可 seek 的内存类字符设备偏离预期：`/dev/null`、`/dev/zero`、`/dev/full` 这类设备在 Linux 上有专门的 `null_lseek` 行为，可支持 `O_APPEND` 打开并把 seek 结果归零；`/dev/urandom` 等设备也通过 `noop_llseek` 接受无副作用 seek。当前 LTP `tst_cmd_()` 用 `O_WRONLY | O_APPEND | O_CREAT` 打开 `/dev/null` 重定向命令输出时，会因为该缺口得到 `EOPNOTSUPP` warning。字符设备侧还没有类似 block devfs `BlockDev::ioctl` 的私有 ioctl hook，因此 random、tty、misc 等后续字符设备 ioctl 只能停在统一 unsupported，不能按设备分发。

**Decision:** 当前不为了单个 `/dev/null` warning 贸然扩展 `FileOps` 或在 VFS 层硬编码特殊设备。后续需要先明确字符设备拥有的最小 seek/ioctl contract：默认 ioctl 应保持 Linux 风格稳定 unsupported，具体字符设备再通过自身 hook 接收私有命令；seek 语义应能表达内存类字符设备的 `null_lseek` / `noop_llseek`，同时不破坏 pipe、目录、普通文件和 block devfs 的现有边界。

**Exit Condition:** 为字符设备子系统补齐可审查的 seek 与私有 ioctl 分发模型，并让 `/dev/null`、`/dev/zero`、`/dev/full`、`/dev/urandom` 至少覆盖 `O_APPEND` 打开、显式 `lseek` 返回值和未知 ioctl errno 的 Linux 兼容边界；随后重新验证触发该 warning 的 LTP mkfs/setup 路径，并把 random/tty 类后续 ioctl 缺口重新归类到对应子域。

**Owner:** doruche
**Last Verified:** 2026-06-05
**Related:** [IOCTL Loop 事务日志](../devlog/transactions/2026-06-04-ioctl-loop.md), [当前限制：IOCTL LTP stage-1 gaps](./current-limitations.md#ane-20260604-ioctl-ltp-stage1-gaps)

## ANE-20260525-SYSV-SHM-MUNMAP-DETACH

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** mm / SysV shm

**Summary:** 当前 SysV shm 只保证通过 `shmdt`、`fork`、`exec` 和进程退出维护 attach 生命周期；如果用户直接对 shm 映射调用 `munmap`，内核不会把它视为一次 SysV shm detach，也不会同步修正 `shm_nattch` 或 attachment 记录。

**Exit Condition:** 为 VMA 增加可追踪的 SysV shm 来源标记或 unmap hook，使任意 `munmap` 路径都能一致驱动 detach / 计数回收，并用专门回归验证 partial unmap、whole unmap 与 `shmdt` 的一致性。

**Owner:** doruche
**Last Verified:** 2026-05-25

## ANE-20260525-SYSV-SHM-LOCK-RESIDENCY-STAGE1

**Type:** Limitation
**Status:** Active
**Severity:** Low
**Area:** mm / SysV shm

**Summary:** 当前 `SHM_LOCK` / `SHM_UNLOCK` 已维护 Linux 可见的 `SHM_LOCKED` mode bit，`IPC_STAT` / `SHM_STAT` 能观察状态切换，满足 LTP `shmctl07` 这类元数据检查。但这仍是 stage-1 兼容状态，不实际 pin / unpin 页面，不接入驻留页账本、`RLIMIT_MEMLOCK` 或 `CAP_IPC_LOCK` / credentials 权限。

**Exit Condition:** 为 SysV shm 接入真实的页锁定路径、驻留页统计、memlock 限额和权限校验，并补齐覆盖 `SHM_LOCKED` 状态、页驻留和失败 errno 的回归测试。

**Owner:** doruche
**Last Verified:** 2026-05-29

**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

## ANE-20260525-SYSV-SHM-PERMISSIONS-PENDING-CREDENTIALS

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** mm / SysV shm / credentials

**Summary:** 当前一期不纳入权限敏感的 SysV shm 测例，像 `shmctl02`、`shmctl04`、`shmget04`、`shmat02` 这类依赖 `setuid` / `setgid`、有效 uid/gid 切换或 IPC 权限检查的路径，仍会受限于现有 credentials 线的未完成状态。

**Exit Condition:** 单独实现 credentials 的真实有效/真实 uid/gid 语义、`setuid`/`setgid` 行为和 IPC 权限检查之后，再把这些权限敏感测例纳入 SysV shm 白名单并回归验证。

**Owner:** doruche
**Last Verified:** 2026-05-29

**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

## ANE-20260529-SYSV-SHM-LTP-INFRA-STAGE1

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** procfs / sysctl / kconfig / SysV shm / user-test

**Summary:** SysV shm 组仍依赖若干当前未提供或未纳入当前架构目标的 Linux 可观察设施：`shmctl03` / `shmget02` 读取 `/proc/sys/kernel/shmmax` 等 sysctl，`shmget03` 读取 `/proc/sysvipc/shm`，`shmget05` / `shmget06` 需要可解析的 kernel `.config`，`shmctl05` 在当前 rv64 目标上因 `__NR_remap_file_pages` 不存在而 TCONF，`shmctl06` 因当前 64-bit ABI 不具备 `time_high` 字段而 TCONF，`shmat01` 的只读写 fault 检查还会经过缺失的 `getrlimit(RLIMIT_CORE)` coredump 辅助路径。这些不表示 SysV shm registry 或 asm-generic ABI 布局本身仍有同类小修缺口。

**Exit Condition:** 为 procfs/sysctl 补齐 SysV shm 需要的只读 knobs 和 `/proc/sysvipc/shm` 视图，提供测试环境可消费的内核配置视图，明确 profile 对架构 TCONF 项的处理策略，并补齐 LTP 所需的基础 rlimit 读路径后，重新验证 `shmctl03`、`shmget02`、`shmget03`、`shmget05`、`shmget06` 和 `shmat01`。

**Owner:** doruche
**Last Verified:** 2026-05-29

**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

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

## ANE-20260602-CLONE3-STAGE1-ADAPTER

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** task / clone3 / pidfd / cgroup / pid namespace

**Summary:** 当前 `clone3` 是现有 `kernel_clone()` 的 ABI 适配层：支持读取并校验 Linux `struct clone_args`，复用已有 fork-like clone、`CLONE_VFORK`、`CLONE_SETTLS`、`PARENT_SETTID`、`CHILD_SETTID`、`CHILD_CLEARTID` 和 `CLONE_CLEAR_SIGHAND` 路径。需要新基础设施的 `CLONE_PIDFD`、`CLONE_INTO_CGROUP`、`set_tid` / `set_tid_size` 和 pid namespace / cgroup / pidfd 文件对象语义仍明确返回未实现，不伪造 pidfd 或指定 tid 分配。

**Exit Condition:** 引入 pidfd 文件对象与 fd 分配、`pidfd_send_signal` 目标解析、cgroup task 归属模型、指定 TID 分配和 pid namespace 层级/权限语义后，重新验证 LTP `clone301` 的 pidfd 分支、`clone303` 以及 clone3 set_tid/selftests。

**Owner:** doruche
**Last Verified:** 2026-06-02
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md), [当前限制](./current-limitations.md)

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

**Summary:** 当前 `mremap` 只适合单个、可按匿名风格编辑的 VMA；如果旧区间跨越多个 VMA，或者目标需要保留 file-backed / shared backing 与 `pgoff`，现有实现会把尾部按匿名模板重建，语义会偏离 Linux。2026-05-29 已把 `mremap03` 这类 old range 无效的用户可见 errno 收口到 `EFAULT`，但 `mremap01` 的 file-backed grow tail 仍会因为 backing 丢失而 fault。

**Exit Condition:** 为 backing-aware grow / move 单独建路径，或者在入口显式拒绝不支持的 VMA 类型，并补齐对应回归。

**Owner:** doruche
**Last Verified:** 2026-05-29
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

## ANE-20260529-MEMORY-LTP-PROCFS-DEVZERO-RLIMIT-STAGE1

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** procfs / devfs / resource limits / mmap

**Summary:** LTP memory 组仍依赖若干尚未系统化的 Linux 可观察接口：`mmap04` 需要 `/proc/self/maps`，`mmap12` 需要 `/proc/self/pagemap`，`mmap14` 现在可以打开 `/proc/<pid>/status`，但仍需要其中 `VmLck` 反映真实 locked-memory accounting；`mmap10` 需要 `/dev/zero` mmap backing，`mmap18` 需要 `MAP_GROWSDOWN` 和 `getrlimit(RLIMIT_CORE)`，`munmap03` 需要 `getrlimit(RLIMIT_DATA)`。这些不是本轮 mmap errno 收口能局部修掉的核心 VMA 编辑问题。

**Exit Condition:** 为 procfs 补齐 memory 组所需的 maps / pagemap 只读语义，并让 `/proc/<pid>/status` 的 `VmLck`、RSS/segment 类字段接入真实 mm 账本；为 `/dev/zero` 提供匿名零页 mmap backing，明确支持或拒绝 `MAP_GROWSDOWN` 的栈增长模型，并实现 LTP 所需的基础 rlimit 读写语义后，重新验证 `mmap04`、`mmap10`、`mmap12`、`mmap14`、`mmap18` 和 `munmap03`。

**Owner:** doruche
**Last Verified:** 2026-06-03
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

## ANE-20260529-PROC-TGID-STAT-STAGE1

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** procfs / task / mm / scheduler

**Summary:** 当前 `/proc/<tgid>/stat` 已提供 Linux 兼容的 52 字段格式，并填入 pid、ppid、pgrp、session、leader status 粗映射、thread 数、CPU usage ticks、starttime、vsize、cmdline/env range、exit signal/code 等已有数据源；但 rss、fault 统计、tty/job-control、ELF segment 边界、signal bitmap、realtime/delay/guest time 等字段仍是 stage-1 占位值。

**Exit Condition:** 为 resident page accounting、minor/major fault 统计、ELF load/data/brk 边界、signal mask/disposition bitmap、controlling tty / foreground process group 和更完整调度策略字段补齐真实数据源，并用依赖 `/proc/<pid>/stat` 的 LTP / libc 脚本重新验证字段语义。

**Owner:** doruche
**Last Verified:** 2026-05-29
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

## ANE-20260531-IOMUX-INFINITE-WAIT-STAGE1

**Type:** Limitation
**Status:** Resolved
**Severity:** Medium
**Area:** iomux / scheduler / procfs / user-test

**Summary:** 该 stage-1 限制已由 `sched-latch` 迁移关闭。`ppoll` / `pselect6` 的有 fd 阻塞路径现在通过共享 `fs/api/iomux/wait.rs` helper 创建 wait-core `Latch`，source 通过 typed `PollRequest` / `PollRegisterResult` 注册 trigger，wake 后统一执行 final readiness scan 和 outcome mapping；不再依赖 `yield_now()` busy polling 表达 iomux 睡眠。

**Exit Condition:** 已完成。`ppoll` / `pselect6` 接入 wait-core latch OR wait；pipe source 在同一 source lock 下检查 readiness 并保存 trigger，状态变更时先 detach 再在释放 source lock 后触发；未迁移 snapshot-only source 在 register + not-ready 时 fail closed 为 `Unsupported`，不会让 syscall 睡在未 armed source 上。阶段 6 旁路审计未发现 `PollWaiter` / `poll_waiters` 残留、iomux wait path 中的 `yield_now()`、fd source 直接写 task sched state、或 fd source 持 source lock 进入 wait core wake。用户侧阶段 6 测试已通过。

**Residual Boundary:** `pselect6` exception / POLLPRI readiness 仍是显式 stub；该功能缺口不重新打开本条已关闭的 iomux 睡眠可观测性限制。更完整的 `/proc/<tgid>/stat` 字段精度仍按 `ANE-20260529-PROC-TGID-STAT-STAGE1` 跟踪。

**Owner:** doruche
**Last Verified:** 2026-06-04
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md), [Sched Latch 事务日志](../devlog/transactions/2026-06-03-sched-latch.md), [RFC-20260603-sched-latch](../rfcs/sched-latch/index.md)

## ANE-20260608-PSELECT6-EXCEPTFDS-STAGE1

**Type:** Limitation
**Status:** Active
**Severity:** Low
**Area:** fs / iomux / pselect6

**Summary:** `pselect6` 当前接受 `exceptfds` 作为 stage-1 兼容入口：置位 fd 仍会被校验，返回前对应用户 fdset 会被清空，并在非空 `exceptfds` 请求时打 notice；但内核尚无内部 `POLLPRI` / exception readiness `PollEvent`，也没有 source-side register / wake 能力，因此不会把任何 fd 报告为 exception-ready。这个兼容 no-op 是为了让 lmbench 等 defensive `exceptfds` 探测不再被 `ENOTSUP` 拦截，不代表完整 Linux `exceptfds` 语义已经实现。

**Exit Condition:** 为 iomux 引入明确的 exception / priority readiness 表达，补齐相关 source 的 snapshot 与 latch register 语义，并让 `pselect6 exceptfds` 按真实 readiness 更新输出 fdset；随后用 lmbench 与覆盖 `POLLPRI` / exception readiness 的回归重新验证。

**Owner:** doruche
**Last Verified:** 2026-06-08
**Related:** [pselect6 exceptfds 小迭代记录](../devlog/changes/2026-06-08-pselect6-exceptfds-compat.md), [开发日志：2026-06-08 至 2026-06-21](../devlog/2026-06-08_to_2026-06-21.md)

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

**Summary:** `/proc/<tgid>/fd` 目录缺失本身已在 `proc-tgid-fd` 阶段 1 中收口：当前实现提供 same-tgid 范围内的 fd number snapshot、`/proc/<tgid>/fd` 目录枚举、数字 fd lookup、`fd/<n>` symlink `readlink()` 和基本 `getattr()`，普通路径按目标 leader root 视角显示，pipe 显示为 `pipe:[ino]`。本条目重分类为阶段 1 residual limitations：尚未实现完整 Linux magic-link open / open file description 重新打开语义，跨进程权限仍固定为 `same-tgid only` 而非 ptrace / dumpable / namespace 策略，`/proc/<tgid>/fdinfo` 仍缺失，pipe 以外匿名对象只提供稳定 fallback 而非完整 Linux-like 精确显示名，`O_PATH` 在 proc fd link、权限和后续能力上的完整边界仍需跟 `O_PATH` 条目一并收敛。本轮未运行 musl `getcwd02`、LTP `pipe07`、QEMU 或 smoke，因此不把这些用例记录为已通过。

**Exit Condition:** 按后续阶段补齐 fd entry open / magic-link follow、跨进程权限、`fdinfo`、匿名对象精确显示名和 `O_PATH` 相关 proc fd 能力，并重新验证 musl `getcwd02`、LTP `pipe07` 及依赖 fd symlink 的相关 libc 路径。

**Owner:** doruche
**Last Verified:** 2026-06-04
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md), [PROC TGID FD 事务日志](../devlog/transactions/2026-06-04-proc-tgid-fd.md)

## ANE-20260528-OPATH-STAGE1-CAPABILITIES

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** VFS / openat / fd

**Summary:** 当前 `O_PATH` 已作为独立 access mode 建模，支持 `readlinkat(fd, "", ...)`、`fstat` / `newfstatat(fd, "", AT_EMPTY_PATH)` 和作为 `openat` dirfd 的基础子集；普通 `read`、`write`、`lseek`、`mmap`、`getdents64` 会按 path-only fd 边界拒绝。尚未提供 `O_PATH` directory fd 上的 `fchdir`、完整 chmod/chown/ioctl 边界，或 `/proc/<pid>/fd` 对 path fd 的完整可见性。

**Exit Condition:** 明确并补齐 `O_PATH` fd 在 fchdir、metadata mutation、ioctl、procfs fd link 和权限检查上的完整 Linux 兼容边界，并用覆盖 symlink、directory、regular file 与 empty-path syscall 的回归矩阵验证。

**Owner:** doruche
**Last Verified:** 2026-05-28
**Related:** [开放问题](./open-issues.md), [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

## ANE-20260528-OPEN-STATUS-FLAGS-STAGE1

**Type:** Limitation
**Status:** Active
**Severity:** Low
**Area:** VFS / openat / fcntl

**Summary:** `openat` 已把 access mode、fd-local flags、file status flags 和 Linux-visible compat bits 分开保存，`F_GETFL` 能还原 open 时保存的持久 flag，`F_SETFL` 只动态修改 `O_APPEND`、`O_NONBLOCK` 和 `O_DIRECT`。opened file description 仍是 file status flags 的唯一真相源；`FileOps` data I/O 通过短生命周期 ctx 观察 normalized status snapshot，pipe 不再保存私有 `nonblock` 行为状态，block special file 的 `O_DIRECT` 拒绝由后端 status check 表达。`O_SYNC`、`O_DSYNC` 和 `O_NOATIME` 当前会保存并通过 `F_GETFL` 可见，但只记录兼容状态，不承诺真实同步写入或 atime 抑制语义；通过 `F_SETFL` 传入这些不可动态修改位会被忽略并打日志。

**Exit Condition:** 为同步写、direct I/O 和 atime 更新引入真实文件系统语义，或者逐项收敛为明确拒绝/兼容策略，并补齐 `openat`、`fcntl(F_GETFL/F_SETFL)` 与 IO 可见性的回归验证。

**Owner:** doruche
**Last Verified:** 2026-06-10
**Related:** [FileOps status ctx 边界清理小迭代记录](../devlog/changes/2026-06-10-fileops-status-ctx.md), [开发日志：2026-06-08 至 2026-06-21](../devlog/2026-06-08_to_2026-06-21.md), [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

## ANE-20260528-PIPE-PROCFS-KNOBS-STAGE1

**Type:** Limitation
**Status:** Active
**Severity:** Low
**Area:** pipe / fcntl / procfs / user-test

**Summary:** 当前匿名 pipe 的基础语义已覆盖 `SIGPIPE`、`O_NONBLOCK`、`F_GETPIPE_SZ`、`F_SETPIPE_SZ(0)` 与 `FIONREAD`，但容量仍是单页固定 stage-1；`F_SETPIPE_SZ` 不支持真实扩容，`O_DIRECT` 只保留可观察 flag 而未实现 packet-mode pipe。LTP `pipe15` 还依赖 `/proc/sys/fs/pipe-user-pages-soft`，`pipe2_04` 的阻塞状态检查依赖 `/proc/<pid>/stat`，这些 procfs knobs/process stat 入口尚未提供。

**Exit Condition:** 为 pipe 容量引入可增长/可收缩的真实 backing 和资源限制账本，补齐 `/proc/sys/fs/pipe-*` 与 `/proc/<pid>/stat` 中测试所需的最小可观察语义，并重新验证 `pipe15`、`pipe2_04` 及 `fcntl(F_SETPIPE_SZ)` 边界测例。

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

## ANE-20260528-ROFS-DIRECT-WRITE-STAGE1

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** fs / mount / VFS / mmap

**Summary:** 当前 `MS_RDONLY` 已能通过 mount flags 传播到 VFS，并覆盖直接写路径：目录项创建/删除/改名、普通文件 open-for-write / write / truncate、`chmod` / `chown` / `utimensat` / `fallocate` 会在只读挂载上返回 `EROFS`。这不是完整 Linux ROFS：shared writable mmap、dirty/writeback 与 `msync` 关系、remount/bind/move mount 语义，以及除 `MS_RDONLY` 外的 mount flags 仍未系统化。

**Exit Condition:** 为 file-backed shared writable mmap 和 writeback 引入明确的只读挂载约束；补齐或显式拒绝 remount、bind、move、propagation 等 mount flag 组合；用覆盖 open/write/truncate/metadata/mmap/remount 的回归矩阵验证 ROFS 语义。

**Owner:** doruche
**Last Verified:** 2026-05-28
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)

## ANE-20260604-IOCTL-LTP-STAGE1-GAPS

**Type:** Limitation
**Status:** Active
**Severity:** Medium
**Area:** ioctl / block / loop / random / procfs / user-test

**Summary:** LTP ioctl 组的直接小缺口已经部分收口：block EOF read 在 EOF 处返回 `0`，通用 block devfs 支持 `BLKRASET` / `BLKRAGET` 读回，`/dev/urandom` 已发布，user-test 为 LTP 的 loop built-in driver 检测安装 `/lib/modules/6.6.32/modules.dep` 和 `modules.builtin` fixture。但这不是完整 ioctl 组闭环：`ioctl01` 仍依赖 pty/devpts/ptmx，`ioctl02` 的本地 group entry 尚未改为 upstream wrapper，`ioctl03` 仍缺 TUN/TAP，`ioctl04` 后续还需要可靠 mkfs/setup 与 `BLKROGET` / `BLKROSET` 加只读 mount 语义，`ioctl07` 进入下一层后仍需要 `RNDGETENTCNT` 和 `/proc/sys/kernel/random/entropy_avail`，`ioctl08` / `ioctl09` 分别依赖 btrfs、parted、partscan、loop partition nodes 和 `/sys/block` 可观察面。`ioctl_loop01..07` 现在可以越过 driver availability false-negative；最新 `ioctl_loop01` 已找到 `/dev/loop0`，但继续暴露 `parted` 环境缺失与 `/sys/block/loop0/loop/partscan` 缺失。后续仍会暴露 `LOOP_CHANGE_FD`、`LOOP_SET_CAPACITY`、`LOOP_SET_BLOCK_SIZE`、`LOOP_CONFIGURE`、partscan、direct I/O、autoclear 和 loop sysfs 等真实语义缺口。

**Decision:** 当前不为 `ioctl_loop01` 单独伪造 `/sys/block/loopN/loop/{partscan,autoclear,backing_file}`，也不把 `LO_FLAGS_PARTSCAN` / `LO_FLAGS_DIRECT_IO` / 未具备释放 hook 的 `LO_FLAGS_AUTOCLEAR` 伪装成成功。补一个只读 `partscan` 文件只能解除首个 `ENOENT`，随后会马上遇到 flag 回显、sysfs 状态、partition node 和真实 partscan 语义要求；这属于 loop sysfs / partition 子域，不纳入本轮实现。

**Exit Condition:** 按子域分阶段补齐并重新验证 ioctl 组：tty/pty wrapper 与基础 tty ioctl、TUN/TAP feature gating、generic block readonly ioctl 与 readonly mount、random ioctl/procfs entropy 面、loop 扩展 ioctl/sysfs/partscan/partition nodes、以及 rootfs 工具与 btrfs/parted 环境边界。每个子域完成后用对应 LTP case 重新归类，避免把 prerequisite false-negative 与真实 ioctl 语义混在一起。

**Owner:** doruche
**Last Verified:** 2026-06-04
**Related:** [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md), [IOCTL Loop 事务日志](../devlog/transactions/2026-06-04-ioctl-loop.md), [rv64 LTP ioctl 运行证据](../rfcs/ioctl-loop/backgrounds/ltp-ioctl-rv64-20260604/index.md), [RFC-20260603-IOCTL-LOOP](../rfcs/ioctl-loop/index.md)

## ANE-20260607-SIGNAL-LTP-INFRA-STAGE1

**Type:** Limitation
**Status:** Active
**Severity:** Low
**Area:** signal / procfs / resource limits / kconfig / user-test

**Summary:** signal profile 中仍有若干 LTP 设施或 Linux 可观察面缺口，不应和本轮 signal syscall 语义修复混为一类：`kill03` 与 `tkill02` 需要读取 `/proc/sys/kernel/pid_max`，当前返回 `ENOENT` 并 `TBROK`；`kill11` 的 setup 依赖 `getrlimit(RLIMIT_CORE)`，当前 `getrlimit(4, ...)` 返回 `ENOSYS`；`kill13` 通过 `/etc/ltp/anemone-kconfig` 检查 `CONFIG_UBSAN_SIGNED_OVERFLOW`，当前 fixture 未声明而 `TCONF`。日志里的 `unknown syscall number 123` 是缺少 `sched_getaffinity` 的 LTP 启动噪声，`rt_sigqueueinfo01` 附近的 `unknown syscall number 283` 是缺少 `membarrier` 的线程库噪声；它们不是本次 `tgkill03` / `rt_sigqueueinfo01` 的直接根因。

**Exit Condition:** 为 LTP signal profile 所需的 procfs/sysctl、基础 `getrlimit` 和 kconfig fixture 补齐最小可观察语义，并决定是否实现或静默兼容 `sched_getaffinity` / `membarrier` 这类启动探测 syscall；随后复跑 signal profile，确认这些 case 不再以设施缺口遮蔽 syscall 语义判断。

**Owner:** doruche
**Last Verified:** 2026-06-07
**Related:** [Signal LTP tgkill/sigqueueinfo 小迭代记录](../devlog/changes/2026-06-07-signal-ltp-tgkill-sigqueueinfo.md), [开放问题：Signal LTP remaining semantics](./open-issues.md#ane-20260607-signal-ltp-remaining-semantics), [开发日志：2026-05-25 至 2026-06-07](../devlog/2026-05-25_to_2026-06-07.md)
