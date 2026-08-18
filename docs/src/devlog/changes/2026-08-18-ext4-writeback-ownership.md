# ANE-CHG-20260818-ext4-writeback-ownership

**Type:** Correctness repair
**Status:** Completed
**Date:** 2026-08-18
**Authors:** doruche, Codex
**Area:** VFS inode metadata / ext4 / lwext4 writeback

## Problem / Context

ext4 adapter 原先把 inode load 时取得的 `meta.size` 同时用于所有 inode kind 的 `stat` 和后续 sync。这个 snapshot 对
regular file 是 resident inode/address space 拥有的 logical size；但 directory size 随 lwext4 dirent/HTree mutation
变化，symlink 与 special inode size 也描述 backend representation。把这些 backend-owned size 当成 VFS-owned
writeback truth，会让 stale directory snapshot 在 inode sync/eviction 时调用 `set_len()`，从而截断 lwext4 仍在使用的
目录块；`stat` 也可能报告已经过期的非 regular size。

另一条独立问题位于 flush 边界：lwext4 allocation 会更新其内存 `ext4_fs.sb` 中的 free block/inode summary，而
`ext4_block_cache_flush()` 只提交 block cache。原 `Ext4Filesystem::flush()` 因此可以在 inode、bitmap 和 group metadata
已经落盘后，仍把 superblock summary 留在内存；orderly shutdown 的 filesystem sync 不能保证这些计数和 checksum
一并持久化。

## Decision

- resident regular-file size 继续由 VFS inode/address space 拥有，并在 inode sync 时写回 lwext4。
- directory、symlink 和 special-inode size 由 lwext4 backend 拥有；sync 不用 load-time VFS snapshot 覆盖它们，
  `stat` 在持有 ext4 filesystem guard 时读取 live backend attr。
- ext4 flush 先提交 block cache 中的 inode/bitmap/group metadata，再用 lwext4 的 `ext4_sb_write()` 提交内存
  superblock summary 和 metadata checksum。
- flush 保持 mounted `ERROR_FS` 状态；只有 `ext4_fs_fini()` 可以在真实 filesystem finalization 时写入
  `VALID_FS`。本轮不把 sync 扩大成 unmount 或新的 crash-consistency/journaling guarantee。

## Implementation Boundary

**Target:** 消除 non-regular inode size 的双重 behavioral authority，并使既有 ext4 flush 同步提交 cache metadata 与
对应 superblock summary；保持 syscall ABI、errno、VFS regular-file size、lwext4 serialization、shutdown handoff 和
既有 durability 边界。

**Owners / handoff:** VFS inode/address space 唯一拥有 resident regular logical size；lwext4 filesystem 唯一拥有
non-regular representation size、allocation summary 和 superblock checksum；ext4 adapter 只在现有 filesystem Mutex
内选择正确 owner 并按 metadata-before-summary 顺序提交。

**Failure / cleanup:** backend attr、cache flush 或 superblock write 的错误继续映射到既有 `SysError`/`Ext4Error`，不
隐藏 partial failure，不新增 retry、rollback、fallback 或双路径。superblock write 失败时 flush 返回错误，filesystem
仍保持 mounted/error 状态；Drop/finalization 生命周期不变。

**Protected surface / Contract Impact:** 不扩大 public Rust API、visibility、syscall ABI 或 shared contract；不改变
regular-file truncate/cache semantics，也不改变 `SYSTEM-POWER-ORDERLY-001` 的 shutdown handoff。Contract Impact:
`None`。

## Change

- `2a35a50a`：inode sync 仅对 regular file 比较并写回 logical size；保留 non-regular backend size/block ownership。
- `ac9474e1`：regular `stat` 继续读取 VFS snapshot；non-regular `stat` 从同一次 lwext4 `FileAttr` 读取 live size 和
  device identity。
- `4e66ef64`：lwext4 Rust facade 暴露既有 `ext4_sb_write()`，`flush()` 按 cache metadata、superblock summary/checksum
  的顺序持久化。

没有新增 KUnit。三个问题都依赖一张可 mutation、sync、drop/reopen 并由 `e2fsck` 检查的真实 formatted ext4 image；
仓库当前没有 disposable ext4 KUnit fixture。为 private predicate、FFI 调用次数或 test-only filesystem lifecycle
建立 helper/probe 只能证明测试替身，不会触达会截断目录块或遗漏 superblock summary 的 production path，因此本轮以
源码 owner/调用链证明、真实镜像重复启动和离线 `e2fsck` 作为对应验证。

## Validation

- 每个代码 patch 都分别通过 `just fmt kernel --check`、`git diff --check`、
  `just build --preset qemu-virt-rv64-release` 和 `just build --preset qemu-virt-la64-release`。
- lwext4 facade patch 通过 `just test lwext4`，host callback tests 为 2/2。
- RV64 single-HART `run-user-test-rv64.sh` 在 preliminary 4 GiB ext4 image 的 worktree-local copy 上完成 rootfs、kernel
  build、QEMU boot、802/802 KUnit、glibc/musl `signalfd` profile 6/6，以及 orderly filesystem/network/device shutdown
  后 PowerOff。
- 第一次关机后，对运行副本执行 `e2fsck -fn`，inode/block/size、directory structure、connectivity、reference count
  和 group summary 五个阶段全部通过。
- 不覆盖运行副本，使用同一 image 再次冷启动；802/802 KUnit、6/6 LTP 和 orderly filesystem shutdown 再次通过。
  第二次关机后的 `e2fsck -fn` 仍为 exit 0，报告 `6862/262144 files`、`813928/1048576 blocks`。
- Architecture Friction Scan 未发现第二份 behavioral truth、owner 穿透、private representation 泄漏、public API
  扩大、architecture/test 特判、无退出条件临时桥、隐含 cleanup 顺序或无 consumer 抽象。change review 没有
  Apollyon、Keter 或 Euclid finding。
- **Not Run:** LA64 runtime、VisionFive 2 实体硬件、final harness、full LTP、SMP、I/O fault injection、非正常断电、
  journal recovery 和 durability/performance A/B。

## Remaining Risk / Links

- [`ANE-20260801-VFS-MAKE-NODE-LWEXT4-ATOMICITY`](../../register/current-limitations.md#ane-20260801-vfs-make-node-lwext4-atomicity)
  保持 Active；本轮修复 writeback ownership，不新增跨多个 backend mutation 的 rollback/atomicity。
- [`ANE-20260726-SYSTEM-POWER-BEST-EFFORT-BOUNDARIES`](../../register/current-limitations.md#ane-20260726-system-power-best-effort-boundaries)
  保持 Active；本轮让已调用的 ext4 flush 覆盖 superblock summary，但不改变 shutdown best-effort aggregation 或
  block-device flush capability。
- [`ANE-20260523-EXT4-TRUNCATE-CACHE-INVALIDATION`](../../register/current-limitations.md#ane-20260523-ext4-truncate-cache-invalidation)
  保持 Active；regular-file truncate/address-space coherency 不在本轮边界内。
- Current contract / RFC / transaction：None。运行日志位于本地 build artifact，长期结论由本记录和三个 Git patch
  持有。
