# ANE-CHG-20260818-lwext4-create-publication

**Type:** Correctness repair / API boundary
**Status:** Completed
**Date:** 2026-08-18
**Authors:** doruche, Codex
**Area:** ext4 / lwext4-rust / inode creation / resource lifecycle

## Problem / Context

`lwext4-rust` 原先只暴露一个按 inode type 参数化的 `create()`，但这个方法在 parent dirent 发布后才设置 mode，创建
directory 时也在发布后才写入 `.`、`..`。kernel symlink 路径还必须先调用 `create()` 发布空 inode，再调用独立的
`set_symlink()` 写入 target；后一步失败会留下已经可 lookup、但内容不完整的 symlink。

失败资源的 owner 也不完整：`create()` 在取得 parent reference 前先分配 child inode，任何后续错误均没有统一回滚；
`make_node()` 只释放 nlink 为零的 inode，不能覆盖已为 directory 或 long symlink 分配的数据块。继续让 kernel 组合
allocate、initialize、publish 和 payload mutation，会把同一个 backend-local create protocol 分散到边界两侧。

本轮只修复无 journal 前提下的正常资源准备、最终发布和可实施回滚。ext4 filesystem Mutex 已有的串行化不变；VFS
backend commit 到 inode/dentry materialization 的跨 owner window、lwext4 任意内部 I/O partial failure、crash/
power-loss、journal/durability，以及通用 `InodeRef::Drop` writeback failure 均不在本轮。

## Decision

- `lwext4-rust` 是 backend-local create protocol 的唯一 owner。public facade 用
  `create_regular()`、`create_directory()`、`create_symlink()` 和既有 `make_node()` 表达完整语义操作，删除允许 kernel
  发布后再补写 symlink 的 generic `create()` / `set_symlink()` 组合。
- name limit、目标不存在和 parent reference acquisition 在 child allocation 前完成。目标存在返回 `EEXIST`，错误仍由
  kernel 既有 `map_ext4_error()` 投影，不新增 Linux ABI 或 errno policy。
- child 的 final type-specific metadata、directory `.`/`..` 和 symlink payload 全部先准备；parent 中的 named dirent
  是最后一个 fallible namespace mutation。directory 准备期间对 parent nlink 的增量由同一 operation 持有原值，
  最终发布失败时恢复该原值。
- 未发布 inode 的统一清理先要求 nlink 为零；regular/directory/symlink 若已有 representation size，则先
  `truncate(0)` 释放数据块，再释放 inode allocator entry。cleanup failure 优先返回，因为此时不能诚实声称资源已经
  回收。
- 不建立 KUnit ext4 ramdisk fixture。当前 KUnit 使用共享 live kernel/rootfs 且没有 case-local filesystem rollback；
  blank RamDisk 也不提供 formatted ext4/mkfs lifecycle。为本轮增加 mkfs、mount isolation、fault injection 或
  production validation hook 是独立能力，不能提高本轮源码顺序证明的 oracle。

## Implementation Boundary

**Target:** 让 regular create、mkdir、symlink 和 make-node 在 `lwext4-rust` 内形成
`prevalidate/acquire -> allocate -> fully prepare -> publish` 的单一 lifecycle；所有已确认尚未成功发布的失败资源按
其实际 representation 回收。

**Owners / handoff:** kernel ext4 adapter只提交 parent/name 与最终 type-specific facts，并消费 inode number 或
`Ext4Error`；`lwext4-rust` 独占 C inode/directory reference、allocation、pre-publication state、named-dirent commit
point 与 rollback。ext4 filesystem Mutex 继续拥有既有调用串行化，本轮不重新设计并发协议。

**Failure / cleanup:** prevalidation 或 parent acquisition 失败时没有 child allocation；child preparation 失败时清理
其 blocks/inode；directory named-dirent publication 失败时先恢复 parent nlink，再清理 child。lwext4
`ext4_dir_add_entry()` 在任意内部 I/O error 下是否已形成 partial block mutation仍由 accepted limitation承接，本轮
不宣称 journal-like all-or-none。

**Protected surface / Contract Impact:** `lwext4-rust` 的 owner-facing Rust facade 有意从可分步组合的 generic API
收窄为四个 semantic operations，唯一 production consumer在同一 checkpoint 切换；raw FFI visibility、kernel
public API、Linux ABI、visible success semantics和 effective current contract不变。Contract Impact: `None`。

**Acceptance:** 以 production source lifecycle audit 为语义 oracle，逐操作确认所有可预判失败先于 allocation、最终
named dirent发布顺序及每个 fallible edge的 cleanup；host tests和双架构 build只证明 facade、bindings与 consumer
机械闭合，不外推 runtime、fault或crash语义。

## Change

- wrapper把 duplicate lookup与 parent acquisition收回 backend create boundary；bindgen仅补充内部 `EEXIST` 常量。
- regular file和make-node在 final mode/owner/device identity完成后调用共用的 `publish_new_inode()`；该 helper断言 child
  nlink仍为零，并在最终 dirent添加失败时执行 data-aware cleanup。
- directory先设置 mode并建立 `.`、`..`，最后添加 parent named dirent；各准备失败清零 child nlink后截断/释放，最终
  发布失败还恢复精确的原 parent nlink。
- symlink在 inode allocation前检查 target上限，在未发布 child上写入 inline或block-backed payload，写入失败时利用
  lwext4 append已经记录的 inode size执行 `truncate(0)`，成功后才发布 parent dirent。
- kernel ext4删除 create后调用 `set_symlink()` 以及三个手工 duplicate-check组合，改为一次调用对应 semantic facade。

## Validation

- source lifecycle audit确认四条路径均只有一个最终 named-dirent publication：regular与make-node没有发布后的 metadata
  mutation；directory发布前已有 mode、`.`、`..`，失败恢复 parent/child link count并回收目录块；symlink发布前已有完整
  target，long-target write失败可由已扩展的 inode size定位并截断数据块。
- residual search确认 kernel不再调用 generic `fs.create()`/`fs.set_symlink()`，wrapper不再公开对应分步能力；新的
  semantic methods只有 ext4 production consumer。
- `just test lwext4`通过，2/2 existing host block-callback tests成功；这些测试只作为 wrapper compile/link证据。
- `just build --preset qemu-virt-rv64-release` 与
  `just build --preset qemu-virt-la64-release` 串行通过。输出只有既有 unused warnings。
- `just fmt kernel --check`、`git diff --check`与`mdbook build docs`通过。
- Architecture Friction Scan以 live path确认：没有新增第二份持久状态、owner穿透、raw representation泄漏、架构/测试
  特判、临时桥或无 consumer抽象。`original_parent_nlink`只在单次未发布 operation内服务精确 rollback，不参与后续
  行为；两个 private helper分别由四条 admission路径和三个 leaf publication路径共同消费。没有未关闭的
  Apollyon、Keter或Euclid finding。
- **Not Run:** KUnit、QEMU/runtime、LTP、SMP/concurrency stress、fault/crash/power-loss injection、journal recovery、
  remount/`e2fsck`及实体硬件。

## Remaining Risk / Links

- [lwext4 make-node atomicity limitation](../../register/current-limitations.md#ane-20260801-vfs-make-node-lwext4-atomicity)
  保持 Active：本轮只闭合正常 wrapper lifecycle，不保证 `ext4_dir_add_entry()` 任意内部 I/O failure或 crash下物理
  all-or-none，也不引入 journal。
- [VFS create publication atomicity](../../register/open-issues.md#ane-20260801-vfs-create-publication-atomicity) 保持 Open：
  backend成功后到 VFS inode/dentry materialization 的 failure/concurrency window不属于 backend-local rollback。
- common-create后续写入的 VFS-owned uid/gid/resident metadata、通用 inode reference Drop错误和 rename/link/unlink
  lifecycle均未改变；若后续单独处理，必须重新建立各自 Implementation Boundary。
- 若未来多项 ext4测试共同需要 disposable formatted filesystem，可以独立设计可复用的 KUnit fixture；不能把本轮缺少
  fixture误写为 production create protocol需要的能力。
- Current contract：[`VFS Creation 与 Make Node`](../../contracts/vfs/make-node.md)，本轮无正文变化。
- RFC / transaction：None；本记录和同一 Git change持有本轮决策与证据。
