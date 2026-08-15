# ANE-CHG-20260813-proc-filesystems

**Type:** Small iteration / procfs ABI projection
**Status:** Completed
**Date:** 2026-08-13
**Authors:** doruche, Codex
**Area:** VFS filesystem registry / procfs

## Problem / Context

VFS已经用`register_filesystem()`与`FileSystemOps::name`唯一拥有filesystem type publication和canonical identity，
`FileSystemMountOps`也唯一表达no-device / block-device requirement，但procfs没有Linux-compatible
`/proc/filesystems`条目。让每个backend另行向procfs发布会形成第二份容易随initcall顺序、条件编译或注册失败漂移的列表。

此前mount fstype/source compatibility小迭代明确把该条目留给真实consumer后续设计，并未预留projection API。本轮新增
该用户可见投影，但不改变mount admission、backend lifecycle或fstype alias边界。

## Decision and Implementation Boundary

Target是新增只读`/proc/filesystems`，逐项展示已经成功进入VFS registry的canonical filesystem type，并从现有
`FileSystemMountOps` tag派生Linux `nodev`列。VFS registry继续是publication唯一owner；procfs只持短生命周期
`Arc<FileSystem>` snapshot，释放registry lock后格式化，不要求各backend调用procfs专用接口。

本轮不支持runtime unregister/module，不发布syscall-only scoring alias，不改变省略`mount -t`的probe、mount source
admission、superblock/mount lifecycle或registry ordering contract。`KERNEL_FS`只表达用户mount拒绝，仍是已注册type，
因此和Linux内部filesystem一样参与展示。读取分配失败沿用bounded natural allocation的kernel-fatal policy；没有新增
持久状态、回滚或cleanup路径。

若实现需要第二个publication owner、扩大公开Rust API、建立动态register/unregister观察协议或改变mount ABI/acceptance，
本小迭代停止并重新分级。

## Change and Contract Cutover

- VFS新增fs-owner-private registry snapshot入口，在registry read lock内只clone当前`Arc<FileSystem>`列表；
- procfs新增静态`filesystems` PDE；每次读取在lock外按registration order格式化，NoDevice输出
  `nodev\t<name>`，BlockDevice输出`\t<name>`；
- backend注册调用保持不变：`register_filesystem()`成功本身就是对该条目的publication；
- owner-local KUnit用真实filesystem type fixture覆盖NoDevice、BlockDevice、canonical name和`KERNEL_FS`不被过滤；
- `VFS-MOUNT-ADMISSION-004`与代码、测试在同一commit原子cut over，`-001..003`语义保持不变。

## Validation

- `just fmt kernel --check`通过；`git diff --check`与`mdbook build docs`通过；
- `just build --preset qemu-virt-rv64-release`与`just build --preset qemu-virt-la64-release`通过；两者均包含
  production feature set与KUnit，但LA64只形成compile/link证据；
- `./scripts/run-final-test-rv64.sh etc/final/images/sdcard-rv.img build/proc-filesystems-final-rv64.log`完成final target
  构建并启动8c/8G guest，636/636 KUnit通过，focused
  `filesystems_projection_uses_mount_kind_and_canonical_name`通过；
- 同一RV64 guest以明确marker直接执行`cat /proc/filesystems`，观察到`ext4`无`nodev`，`ramfs`、`devpts`、
  `anonymous`、`devfs`和`proc`均带`nodev`，与本次启动中成功注册的完整type集合一致；未出现scoring alias；
- final镜像中的BusyBox shell没有`poweroff` applet，因此取得oracle后通过QEMU monitor终止host进程。该运行证明boot、
  全量KUnit与直接procfs读取，不声明final userspace suite正常完成；读取时相邻BusyBox `cat`触发未实现syscall 223日志，
  但文件读取与完整输出成功，本轮未将其归因为procfs失败或扩入修复；
- LA64 runtime、完整final userspace suite、preliminary LTP、实体硬件与runtime register/unregister均为Not Run。

## Remaining Risk / Links

- registration order是当前实现的稳定输出形状，但不是本轮发布的用户ABI；若未来引入runtime module/unregister，必须独立定义
  snapshot一致性与open/read/seek可见性，不能把当前一次read生成字符串的行为自然外推。
- Current contract：[VFS Mount Admission](../../contracts/vfs/mount-admission.md)。
- Prior decision：[mount fstype/source compatibility](./2026-07-24-mount-fstype-source-compat.md)。
- Linux source：`xref:linux-6.6.32:fs/filesystems.c#filesystems_proc_show`。
