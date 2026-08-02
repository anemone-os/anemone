# ANE-CHG-20260802-lwext4-rust-safe-facade

**Type:** Cleanup / FFI safety boundary
**Status:** Completed
**Date:** 2026-08-02
**Authors:** doruche, Codex
**Area:** ext4 / lwext4-rust / FFI / block I/O / filesystem adapter

## Problem / Context

`lwext4-rust` 原先公开整个 bindgen `ffi` module，并从 crate root glob re-export
filesystem 与 inode modules。bindgen input 还包含 MBR、mkfs 和 `fs_test` header；kernel
虽然只需要 typed inode-oriented capability，却可以依赖任意 C representation 或 release
function。

这个过宽 surface 已经包含两个具体 correctness 缺陷：

- `DirReader::current(&self)` 从共享借用返回内部持有 mutable C dirent view 的
  `DirEntry`，safe caller 可以同时取得两个指向同一 entry 的可写 alias；
- `BlockDevice::{read_blocks,write_blocks}` 返回 completed block count，允许 short
  success，但 C callback 丢弃这个 count 并向 lwext4 返回 `EOK`。

constructor 与 Drop 还同时调用 block-device finalization；filesystem、dynamic bcache、
inode reference、directory result/iterator 与 writeback guard 的 acquisition/release owner
没有被一个完整的 Rust lifetime/cleanup model 表达。header invalidation 只覆盖部分 C build
inputs，也可能让 header-only change 复用 stale bindings。

本轮只收口 wrapper facade 与 kernel adapter。lwext4 journal/transaction、compound namespace
operation 的 commit/rollback、crash/power-loss atomicity、VFS handoff、errno projection 和 ext4
支持矩阵均保持原有边界。

## Decision

`lwext4-rust` 是 raw C ABI 与 Anemone kernel ext4 driver 之间唯一的 typed safety owner：

- bindgen `ffi` 只在 crate 内可见；crate root 只显式导出 production consumer 所需能力；
- `Ext4Filesystem` 的独占借用产生 private `FilesystemAccess<'fs>` token，所有 inode、lookup
  和 iterator guard 都绑定到同一 filesystem lifetime；
- directory entry 是只读 borrow-bound view；rename 更新 `..` 只能调用 wrapper-private
  `set_entry_inode()`，kernel 无法取得 raw dirent mutation；
- 一次 C block callback 是 exact full-or-error request。Rust transport 返回 `Result<()>`，
  zero-count 不触发 transport，range、byte count、null buffer或 transport failure全部在 C
  boundary fail closed；
- resource acquisition state只服务 constructor/cleanup protocol，不缓存 C behavior。
  filesystem、bcache 与 block device各有唯一 cleanup owner，guard不能越过 filesystem Drop。

kernel ext4 继续拥有 VFS policy、`fs_lock`/higher-level transaction serialization、resident
metadata/page cache、Linux ABI 与 errno projection。wrapper 不反向 import kernel object，也不
声明新的 journal、rollback 或 durability guarantee。

最终 public facade inventory如下；每项都有真实 kernel consumer或 exported signature义务：

| Facade | Production obligation |
| --- | --- |
| `BlockDevice`, `EXT4_DEV_BSIZE` | kernel block transport adapter与 mount block-size admission |
| `Ext4Filesystem`, `FsConfig`, `StatFs` | filesystem construction、inode/data/namespace operations、sync与 statfs |
| `FileAttr`, `InodeType` | on-disk inode metadata/type到 VFS 的 typed projection |
| `InodeRef` | `with_inode_ref` 的 borrow-bound metadata write capability |
| `DirLookupResult`, `DirReader`, `DirEntry` | lookup/readdir 的 borrow-bound typed result/view |
| `Ext4Error`, `Ext4Result` | wrapper failure与 kernel既有 errno projection 的 handoff |

## Change

- `ffi` 改为 private module，crate root移除 glob exports；wrapper header只 include实际使用的
  bcache、blockdev、directory、errno、filesystem与 inode headers。
- bindgen按 type/function/constant allowlist生成 production bindings；vendored lwext4 C tree
  作为递归 rebuild/regeneration invalidation boundary。
- `BlockDevice` read/write改为 `Ext4Result<()>`。callbacks使用 checked block range与 byte-count
  计算，拒绝 null non-empty buffer，并记录 direction、block range、byte count和 transport
  error；owner-local tests覆盖 exact success、transport failure、zero request、越界与 overflow。
- `DirEntry` 改为只读 view；raw dirent type和 mutation保持 wrapper-private。iterator step、
  lookup result destroy与 filesystem Drop都不能和活跃 safe view/guard重叠。
- 引入 private `FilesystemAccess<'fs>` token，把 `InodeRef`、`DirLookupResult` 与 `DirReader`
  生命周期绑定到 filesystem独占借用；删除没有真实 behavior obligation 的
  `SystemHal`/`Ext4Hal` 空抽象及残留 TODO。
- filesystem constructor记录 fs/bcache acquisition protocol state。失败与正常 Drop按
  `fs_fini -> bcache cleanup/fini -> block-device fini`单一顺序释放；block fini只由
  `Ext4BlockDevice`拥有。lookup/iterator/writeback/block close的不可传播 cleanup error不再静默
  丢弃。
- 删除 `FileAttr` 中无 production consumer 的 device、block size和allocated-block字段，并把
  wrapper-only inode helpers收窄到 crate visibility。
- kernel `Ext4Disk`适配 exact unit-result contract，继续使用原 `BlockDev` full-buffer behavior；
  ext4 namespace operation顺序、error mapping和 VFS handoff未改变。
- 新增仓库拥有的 `just test lwext4` suite；它为 host test显式提供可由调用者覆盖的
  `cc`/`c++`/`ar`与 sysroot默认值，不改变 cross/production toolchain selection。

## Validation

- `just test lwext4`通过，2/2 owner-local callback tests成功；`just test xtask`通过
  65/65 tests。
- final production tree串行通过：
  `just build --preset qemu-virt-rv64-release --bind smp=1 --bind memory=1G`和 LA64对应命令。
  RV64首次 sandbox build在 lwext4 C编译命中 host seccomp `SIGSYS`；完全相同命令在 sandbox
  外通过，因此按环境限制归类。
- RV64使用显式 preliminary测试盘运行 repository wrapper：334项 KUnit全部通过；temporary
  ext4 probe完成 stat/statfs/readdir、regular create/read/write/truncate/fsync、mkdir/link/
  rename/unlink/rmdir、symlink/readlink、character-node kind/`rdev` reload以及两次真实
  unmount/remount，打印 `lwext4-facade-probe: passed`。长期 `sys` profile的 glibc/musl共
  4/4 cases通过，随后完成orderly filesystem/network/device shutdown与 PowerOff。
- LA64在显式 preliminary测试盘运行同一 matrix：339项 KUnit、probe和长期 profile 4/4均
  通过；orderly shutdown后因当前平台无 poweroff handler进入 halt，terminal PASS之后由 host
  退出 QEMU。
- runtime取证后完整删除 temporary probe，`profile.txt`保持长期 `sys`状态。residual search
  确认 production tree不含 probe dispatch、`SystemHal`/`Ext4Hal`或旧 Hal TODO。
- source/type audit确认 crate外无法命名 raw FFI，最新 host/RV64/LA64 bindings均不含
  `fs_test`、MBR或 mkfs API；kernel不调用 raw C type、dirent mutation或 release function。
  public facade每个 item均对应上述 consumer/signature obligation。
- lifecycle audit逐项闭合 block init/fini、fs init/fini、dynamic bcache init/cleanup/fini、inode
  get/put/free-unlinked、directory result/iterator与 writeback guard；没有第二份 mounted、dirty
  或 writeback behavior truth。
- `just fmt kernel --check`、`git diff --check`与`mdbook build docs`通过；mdBook只报告既有
  large search-index warning。一位独立 agent review发现并推动删除残留 Hal TODO；冻结实现
  复核最终无 Apollyon、Keter、Euclid或 Safe finding。

## Remaining Risk / Links

- arbitrary lwext4 I/O failure、journal/rollback、crash/power-loss atomicity与 strict durability
  不在本轮 target；现有 accepted limitation保持不变。
- `map_ext4_error()` 对部分 lwext4 errno的 projection、ext4 truncate/page-cache、file-backed
  mmap与 VFS create publication继续由各自 owner/条目承担，本轮未修改或关闭。
- Hardware、final harness、完整 LTP profile与 fault/crash injection均 Not Run；双架构 QEMU
  matrix不能外推这些证据。
- Register / limitations：
  [lwext4 make-node atomicity](../../register/current-limitations.md#ane-20260801-vfs-make-node-lwext4-atomicity)、
  [VFS create publication atomicity](../../register/open-issues.md#ane-20260801-vfs-create-publication-atomicity)
- Current contract: None；本轮未改变 effective shared contract、Linux ABI或 accepted target。
- RFC / transaction: None
- Issue / PR / commit: this change's Git commit
