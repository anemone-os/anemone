# ANE-CHG-20260818-ext4-link-remove

**Type:** Correctness repair / API boundary
**Status:** Completed
**Date:** 2026-08-18
**Authors:** doruche, Codex
**Area:** VFS / ext4 / lwext4-rust / link / unlink / rmdir / symlinkat

## Problem / Context

kernel ext4 adapter原先把 `unlink` 同时用于普通文件删除与 `rmdir`，在调用前后自行 lookup、判断类型并猜测
resident nlink增减；hard link也在backend返回后另行递增resident nlink。backend dirent/nlink mutation与VFS投影不在同一
ext4 filesystem Mutex窗口内，失败路径还可能让可达目录先被truncate。wrapper内部为取得第二个inode reference使用
`expect()`，把可报告的backend错误提升为kernel panic。

本轮只处理无journal前提下的正常 `link`、`unlink`、`rmdir` semantic operation及其resident nlink投影。symlink create
沿用已完成的prepare-before-publication路径，本轮只做回归验证。任意lwext4 I/O partial failure、crash/power-loss、
journal/durability、generic dentry post-commit failure以及open-unlink inode retirement均不在本轮实现范围。

## Decision

- `lwext4-rust`分别暴露 `link`、`unlink`、`rmdir`，由wrapper唯一拥有name/type/empty-directory validation、raw dirent与
  nlink mutation。成功结果返回精确raw nlink，不让kernel按操作类型猜delta。
- parent dirent removal是删除的namespace commit point。空目录检查先完成，目录仍可达时不truncate；commit后才处理
  nlink、data与inode allocator cleanup。
- ext4 adapter在同一filesystem Mutex内完成backend commit与已存在resident inode的nlink投影/退索引。为保持这一锁域，
  mkdir与既有rename的resident nlink bookkeeping也移入原Mutex窗口，但不改变rename语义。
- hard-link admission拒绝已unlink inode、directory与达到 `EXT4_LINK_MAX` 的inode；`EMLINK`经独立
  `SysError::TooManyHardLinks`投影。Linux目录hard-link的`EPERM`由generic VFS admission统一拥有。
- `symlinkat`的absolute `linkpath`按Linux忽略`newdirfd`；syscall adapter保留raw fd，只在relative branch解析，避免
  参数转换提前返回`EBADF`。
- dirent commit后的inode回收不能无journal回滚namespace。回收错误记录日志并保留已提交outcome，使kernel仍能投影精确
  nlink；truncate失败时不继续free仍有data的inode。失败orphan可能泄漏storage，继续由既有partial-I/O limitation承接。
- 不新增ext4 KUnit ramdisk fixture，也不以并发KUnit或stress证明并发正确性。锁域与唯一owner以production source audit
  证明；runtime只验证可见语义和回归。

## Implementation Boundary

**Target:** 在现有ext4 superblock Mutex内形成backend-local `link`、`unlink`、`rmdir`语义操作，并把成功提交的raw nlink
精确投影到resident VFS inode；修正目录hard-link errno、symlinkat absolute-path dirfd admission与unlink/rmdir类型、
空目录及正常cleanup顺序。

**Owners / handoff:** generic VFS拥有Linux admission、mount/path与dentry cache；ext4 adapter拥有同一Mutex内的backend调用
及resident投影；`lwext4-rust`拥有raw inode/directory reference、dirent commit、raw nlink和allocator cleanup。

**Failure / cleanup:** commit前validation与lookup错误不改变namespace；dirent mutation内部任意partial I/O及commit后
truncate/free失败不保证all-or-none。成功outcome必须与raw nlink一致；已提交cleanup失败不能伪装成未提交事务。

**Protected surface / Contract Impact:** Linux成功语义有意补齐，directory hard link errno从错误的`EISDIR`修正为`EPERM`，
absolute symlinkat不再提前验证未使用的dirfd；`EMLINK`获得准确投影。`lwext4-rust` owner-facing facade有意新增typed
semantic outcomes，kernel public Rust API、shared contract与current contract不变。Contract Impact: `None`。

**Acceptance:** production source audit确认唯一ext4 Mutex span、raw mutation与resident projection顺序、类型/empty-dir/
link-limit admission及每个正常cleanup edge；existing host test/build证明机械闭合；focused RV64 glibc/musl LTP验证当前可运行
的link/symlink cases并如实分类rmdir/unlink的test-setup blocker。LA64保持Not Run。

## Change

- wrapper删除fallible inode-reference clone中的`expect()`，并新增typed create/link/unlink/rmdir outcomes。
- `unlink`与`rmdir`共用private remove lifecycle，但保留独立public语义入口；rename只复用private `Any`模式。
- kernel ext4不再二次lookup或手工猜测unlink/rmdir nlink；mkdir、link、unlink、rmdir、rename bookkeeping均与backend mutation
  位于同一ext4 Mutex窗口。
- VFS统一拒绝directory hard link为`EPERM`；ext4映射`EMLINK`并保留errno matrix覆盖。
- symlinkat adapter改为delayed raw-dirfd resolution，使absolute linkpath忽略无效fd。
- user-test注册focused `ext4-namespace` LTP group，作为本轮及后续namespace回归入口。

## Validation

- source owner/lock audit确认所有production `Ext4Fs` access仍只经`Ext4Sb::with_fs()`；新增resident更新只使用
  non-loading `try_iget()`与`unindex_inode()`，不会在ext4 Mutex内触发`load_inode`或递归取得backend lock。
- `just fmt kernel --check`与`just fmt user-test --check`通过；`just test lwext4`通过existing host callback tests 2/2。
- `just build --preset qemu-virt-rv64-release`通过。LA64未构建。
- 最终RV64 single-HART运行完成804/804 KUnit并orderly shutdown。glibc与musl结果相同：`linkat01` 22/22 TPASS、
  `symlink01` 5/5 TPASS、`symlinkat01` 10/10 TPASS；其中directory hard link的`EPERM`和absolute linkpath忽略无效
  dirfd均由先失败后通过的LTP case直接证明。
- 同一final run中`rmdir01/02/03`、`symlink04`、`unlink05/07/08`与`unlinkat01`均在执行filesystem syscall前因既有
  `/proc/meminfo`格式不足TBROK；两套libc各为3 case pass、8 case setup-broken，不作为ext4失败或成功证据。
- 更早的扩展matrix中`linkat02`创建大规模hard link时触发约4.1 MiB内核分配失败并panic；因此
  `EXT4_LINK_MAX`/`EMLINK`只有source、bindings与build证据，未获得runtime上限证明。
- 自审逐项检查owner/lock order、dirent commit point、type/empty-dir/link-limit admission、post-commit cleanup、exact
  resident projection、errno与open-unlink边界；修正了truncate failure丢失committed outcome的问题。Architecture
  Friction Scan未发现第二状态真相、owner穿透、private representation泄漏、caller/架构/test特判或无退出条件临时桥。
- `git diff --check`与`mdbook build docs`通过。
- **Not Run:** LA64 build/runtime、SMP/concurrency stress、ext4 KUnit ramdisk fixture、fault/crash/power-loss injection、
  journal recovery、remount/`e2fsck`、实体硬件与final harness。

## Remaining Risk / Links

- [`ANE-20260818-VFS-EXT4-UNLINKED-INODE-LIFECYCLE`](../../register/open-issues.md#ane-20260818-vfs-ext4-unlinked-inode-lifecycle)
  记录open-unlink后lwext4立即truncate/free与generic VFS ghost/final-close handoff缺失；本轮不建立ext4私有retirement协议。
- [`ANE-20260801-VFS-MAKE-NODE-LWEXT4-ATOMICITY`](../../register/current-limitations.md#ane-20260801-vfs-make-node-lwext4-atomicity)
  继续承接无journal下dirent mutation与cleanup任意I/O failure/crash atomicity；本轮不扩大其保证。
- `linkat02`的hard-link-count上限runtime证明受当前kernel memory/index规模阻塞；`EXT4_LINK_MAX` admission与`EMLINK`映射目前
  只有source/bindings/build证据，不能写成LTP已证明。
- RFC / transaction / current contract change: None。本轮是一份小迭代、一个closure checkpoint。
