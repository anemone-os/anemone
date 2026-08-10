# ANE-CHG-20260808-ramfs-namespace-single-mutex

**Type:** Cleanup / concurrency simplification
**Status:** Completed
**Date:** 2026-08-08
**Authors:** doruche, Codex
**Area:** ramfs namespace / inode cache synchronization

## Problem / Context

`RamfsSb` 原先用 superblock 级 `RwLock<()>` 包围 namespace transaction：create、make-node、symlink、
link、unlink、rmdir 与 rename 取 write guard，lookup 取 read guard。实际只有 lookup 使用读共享；
readdir 本来只通过 directory-local `RamfsDir::children` 读取容器，因此该 read/write 区分不代表
完整的 namespace reader/writer 并行协议。

transaction 本身仍有 correctness 义务：lookup 的 dirent-to-`iget()` 路径不能与 unindex 交错；
create/make-node 需要在 final metadata 形成后串行化 inode-cache 与 dirent publication；跨目录
rename 需要连续更新新旧 dirent、`..`、parent link count 与 inode index。因此本轮不删除
superblock transaction owner，只删除没有足够收益的读写锁区分，并避免在 allocation、container
mutation 和 inode-cache operation 跨度内持有 spin-based guard。

## Decision

- `RamfsSb` 继续唯一拥有 backend namespace transaction，但改用一把 sleepable exclusive `Mutex<()>`。
- `read_tx()` / `write_tx()` 合并为 `with_tx()`；所有 lookup 与 mutation 经同一入口，不允许
  guard-held helper 递归取得该 Mutex。
- `RamfsDir::children` 继续只拥有 directory-local container synchronization；不把它提升为跨目录
  transaction owner，也不引入多目录锁序、retry 或 rollback 协议。

## Implementation Boundary

**Target:** 用一个 sleepable exclusive Mutex 取代 ramfs superblock transaction 的 spin-based RwLock，
保持现有 namespace、inode-cache、link-count 与 dirent publication 线性化边界。

**Owners / handoff:** ramfs superblock 继续拥有跨目录 backend transaction；`RamfsDir` 拥有本地
container；generic `SuperBlock` 拥有 resident inode index/ghosts；VFS 继续在 backend 成功后拥有
dentry materialization/invalidation，不新增跨 owner handoff。

**Failure / cleanup:** Mutex guard 通过 lexical lifetime 释放；既有 duplicate、type mismatch、nonempty directory、
unindex 与 failed-create 处理不变。不新增 poisoning、retry、fallback、bypass 或临时双路径。

**Protected surface:** 不改变 public Rust API、syscall ABI/errno、visible pathname/rename 语义、
register limitation 或 current contract。保持
[`VFS-MAKE-NODE-001`](../../contracts/vfs/make-node.md#vfs-make-node-001--make-node-admission-与-backend-publication-只有一个-handoff)
的 backend publication serialization。

**Contract Impact / Cutover:** `None`。这是 ramfs owner-local locking implementation cutover，现有 effective
rule、owner、handoff 与可见语义均不变。

**Stop conditions:** 任一 ramfs transaction 需要在 hwirq、IRQ-disabled、preempt-disabled、active-wait 或
early-init no-sleep context 取锁；发现 recursive acquisition/inverse lock order；或需要删除 superblock owner、
引入 per-directory transaction、改变 failure/cleanup、ABI、visible semantics、contract 或 validation claim。

## Change

- `RamfsSb::tx_lock` 改为 `Mutex<()>`，并注释 superblock transaction 与 directory-local container lock 的
  责任分工。
- lookup 与七个 mutation callsite 全部收敛到 `with_tx()`，不更改各 operation 的 transaction span。
- 不修改 `RamfsDir::children`、inode-number allocation、inode metadata、VFS dentry 路径或现有 KUnit。

## Validation

- `just fmt kernel --check`、`git diff --check` 与 `mdbook build docs` 通过。首次 format check 只命中
  Git-ignored generated `anemone-kernel/src/boot_defs.rs` 的旧格式；经 repository-owned
  `just fmt kernel` 规范化后复查通过，没有产生额外 tracked diff。
- source-shape audit 确认 ramfs 中 `read_tx()` / `write_tx()` 均为零，一个 lookup 和七个
  mutation entry 全部经 `with_tx()`；guard-held `ramfs_lookup_locked()` / `ramfs_remove_locked()` 不递归
  获取 Mutex。`ramfs_mount()` 的 early construction/root seeding 不进入该 transaction，未引入 early-boot
  bypass 或 inverse owner call。
- `just build --preset competition-final-rv64-release --bind smp=8 --bind memory=8G` 通过 discovery/final
  两遍 release compile，symbol table 验证为 5035 entries，postbuild 完成。
- `./scripts/run-user-test-rv64.sh etc/preliminary/images/sdcard-rv.img
  build/ramfs-namespace-single-mutex-rv64.log` 完成 single-HART RV64 guest：585/585 KUnit 通过，
  ramfs make-node/special-open 与三组 directory-rename KUnit 5/5 通过；无 Mutex context/recursive
  assertion 或 panic，并完成 filesystem/network/device orderly shutdown 与 PowerOff。既有 `socket` profile
  的 glibc/musl 合计 6/6 case 通过，但它只是本轮未改测试选择下的相邻 runtime 证据，
  不作为 ramfs semantics acceptance。
- **Not Run:** LA64 build/runtime、final harness、full/focused filesystem LTP、SMP runtime/concurrent namespace stress、
  lookup contention/performance A/B、实体硬件与fault injection。

## Remaining Risk / Links

- 专用 Mutex 会串行化原先可并行的 ramfs lookup；本轮不声明 contention/performance 改善。
- [`ANE-20260528-RAMFS-RENAME-STAGE1`](../../register/current-limitations.md#ane-20260528-ramfs-rename-stage1)
  保持 Active；本轮不修正 generic VFS live dentry/`PathRef` relocation 或扩展 rename flags。
- RFC / transaction：None。
