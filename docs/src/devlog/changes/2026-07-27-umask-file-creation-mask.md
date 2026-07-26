# ANE-CHG-20260727-umask-file-creation-mask

**Type:** Small Feature / Linux ABI Compatibility
**Status:** Completed
**Date:** 2026-07-27
**Authors:** EDGW, Codex
**Area:** syscall ABI / task filesystem context / VFS creation / kconfig

## Problem

Anemone 已注册 `SYS_UMASK`，但原实现固定返回 `0777`，既不保存新 mask，也不影响
后续 inode 创建。`openat(O_CREAT/O_TMPFILE)` 与 `mkdirat` 则把用户 mode 原样交给
VFS。因此 shell 中的 `umask 022` 没有实际效果，连续 `umask()` 调用也无法取得真实旧值。

用户还观察到 GCC/ld 生成的输出文件没有执行位、需要额外 `chmod`。这个现象与工具链
读取错误 umask 返回值相符，但当前尚未采集 guest syscall trace，所以本记录不把 GCC
症状单独写成已经确认的根因；验收仍要求在同一 toolchain 环境直接复现。

Linux 把 umask 与 root、cwd 放在同一 filesystem context，`umask()` 以一次 exchange
完成低九位规范化和旧值返回；无 POSIX default ACL 时，inode creation mode 再清除
当前 mask 指定的权限。对应参考为
`xref:linux-6.6.32:include/linux/fs_struct.h#fs_struct`、
`xref:linux-6.6.32:kernel/sys.c#SYSCALL_DEFINE1(umask)` 与
`xref:linux-6.6.32:fs/namei.c#mode_strip_umask`。

## Scope

本轮只闭合当前已经存在的 umask 与 named inode creation 路径：

- `SYS_UMASK` 保存 `mask & 0777` 并返回旧值；
- task filesystem context 唯一拥有 umask，保持 fork copy、`CLONE_FS` share 和 exec
  preserve；
- `SYS_OPENAT` 的 `O_CREAT` / `O_TMPFILE` 与 `SYS_MKDIRAT` 应用 creation mask；
- 初始 user filesystem context mask 由 kconfig 提供，默认十进制 18，即八进制 `0022`；
- 定向审计当前全部注册 syscall 和 VFS named-object creation call site。

本轮不实现 POSIX default ACL、`/proc/<pid>/status` 的 `Umask:` projection，也不顺带
新增当前缺失的 `openat2`、`mknodat` / FIFO、pathname UNIX socket `bind` 或 POSIX
`mq_open`。`chmod*`、symlink、anonymous fd/inode 与 System V IPC 按 Linux 语义不应用
umask。

## Solution

`FsState::Ready` 在既有 root/cwd 旁保存唯一 `umask: InodePerm`。现有
`Arc<RwLock<FsState>>` 已经表达正确的共享边界：不带 `CLONE_FS` 的 clone/fork 通过
`FsState::fork()` 复制，带 `CLONE_FS` 的 task 共享同一 handle。实现不在 `Task`、inode、
credential 或各 filesystem backend 中增加第二份状态。

`SYS_UMASK` 在 syscall boundary 截断 raw mask，只把普通 rwx 位交给 `FsState` 的原子
replace 接口。创建入口读取一次 mask snapshot并形成 masked `InodePerm`；通用
`vfs_touch_at()` / `vfs_mkdir_at()` 保持不变，避免 boot、procfs/devfs、mount 初始化和
KUnit 的 kernel-internal creation 被偶然运行的 task mask 污染。

初始 `0022` 作为用户可见策略进入 KernelConfig，由 xtask generator 生成
`INITIAL_UMASK`。kernel 使用普通 compile-time assertion拒绝超出 `0777` 的配置。Linux
同样以 `0022` 初始化 `init_fs`，并在复制 filesystem context 时复制 umask：
`xref:linux-6.6.32:fs/fs_struct.c#init_fs`、
`xref:linux-6.6.32:fs/fs_struct.c#copy_fs_struct`。

## Change

- `conf/.defconfig` 和 xtask KernelConfig generator 增加 `initial_umask: u16`，默认 18。
- `FsState::Ready` 增加 umask owner、原子 replace 和 creation-mode mask 窄接口；
  `FsState::fork()` 显式复制该值。
- `sys_umask` 删除 stub，规范化低九位、保存新值并返回真实旧值。
- `sys_openat` 仅对 `O_CREAT/O_TMPFILE` 的 mode 应用 mask；已有目标不修改 mode。
- `sys_mkdirat` 在进入 VFS primitive 前应用同一 mask helper。
- KUnit 增加 raw high-bit normalization、普通文件/目录权限矩阵、special-bit 保留、fork
  copy 与 shared-handle 可见性测试。
- source audit确认当前已有且需要 umask 的 syscall 只有 `umask`、`openat`、`mkdirat`，
  三者均已接线；未来 named-object creator必须复用同一 filesystem-context owner。

## Validation

- `just build --preset qemu-virt-la64-release --bind smp=1 --bind memory=1G` 通过。
- `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G` 通过。
- 两个 release build 均启用 `kunit` feature，新增 KUnit 完成编译。
- `just xtask-test` 共运行 55 项，53 项通过。两个失败位于本轮写集之外：
  `resolved_selection_owns_all_snapshot_inputs` 的固定 fixture 值不匹配，以及 device-tree
  `/bin/false` error-text 断言不匹配；本记录不把该命令写成通过。
- `just fmt all --check` 中 kernel 与 xtask 通过，随后停在未修改的
  `anemone-apps/busybox/src/main.rs` 既有格式差异；全仓格式检查未整体通过。
- `git diff --check` 通过。
- RV64 无盘 QEMU smoke 在 mount rootfs 时按预期报告
  `rootfs block device not found: vda`；KUnit runner 位于 root mount 之后，因此该运行不
  构成 KUnit runtime 或 umask ABI 证据。
- agent pre-close 阶段未运行 KUnit runtime、LTP `umask01`、guest mode/stat matrix、
  GCC 输出权限与直接 exec。
- 2026-07-27 用户完成上述全部 runtime closure matrix并确认全部通过：focused KUnit、
  LTP `umask01`、guest umask/mode/stat/继承语义和原始 GCC/ld 输出权限与直接 exec 均无
  失败；GCC 产物不再需要额外 `chmod`。本条只记录用户运行结论，不补造未提供的 case
  数量、日志路径或命令输出。

## Tracking Issues

### CHG-001 - guest umask 与 GCC runtime 尚未验证

**Status:** Neutralized
**Severity:** Euclid

**Issue:** 双架构 build只证明生成、类型和链接闭合，不能证明 syscall 返回序列、真实
inode mode、`CLONE_FS` runtime共享或 GCC/ld 最终输出权限。

**Resolution:** 2026-07-27 用户确认 focused KUnit、LTP `umask01`、guest ABI matrix 与
原始 GCC/ld 直接执行全部通过，GCC 产物无需额外 `chmod`。没有剩余失败信号需要转入
register或继续追踪，本问题关闭。

## Risk / Follow-up

- 当前没有 POSIX default ACL；未来加入 ACL owner 时，parent default ACL 分支必须在
  inode creation policy 中替代直接 umask stripping，不能叠加两次。
- `openat2`、`mknodat` / FIFO、pathname UNIX socket `bind` 与 POSIX `mq_open` 未来落地
  时必须复用 task filesystem-context umask。legacy `open/creat/mkdir` libc入口应复用
  asm-generic `openat/mkdirat`，不能重复应用 mask。
- `mknod` / FIFO 缺口继续由现有 register issue 跟踪；本轮不因 umask owner 已就绪而
  声称该 syscall 已支持。
- CHG-001 已由用户运行证据关闭，本记录状态为 Completed；未来新增 named-object
  creator 的接线义务不构成本轮残留缺陷。

## Links

- Biweekly devlog: [2026-07-20 至 2026-08-02](../2026-07-20_to_2026-08-02.md)
- Current contract: None；本轮是局部 syscall/task/VFS ABI 修复，不提取新的跨 RFC contract
- Register / limitations: [mknod / FIFO 与 legacy readdir issue](../../register/open-issues.md#ane-20260527-ltp-mknod-legacy-readdir)
- RFC / transaction: None
- External source evidence: `xref:linux-6.6.32:include/linux/fs_struct.h#fs_struct`；`xref:linux-6.6.32:kernel/sys.c#SYSCALL_DEFINE1(umask)`；`xref:linux-6.6.32:fs/fs_struct.c#copy_fs_struct`；`xref:linux-6.6.32:fs/namei.c#mode_strip_umask`
- Issue / PR / commit: current workspace diff；no commit created
